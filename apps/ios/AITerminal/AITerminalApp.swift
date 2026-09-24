import SwiftUI
import os.signpost

@main
struct AITerminalApp: App {
    init() { UITextView.appearance().backgroundColor = .clear }
    var body: some Scene { WindowGroup { WorkspaceScreen().defaultAppStorage(WorkspacePreferences.defaults).preferredColorScheme(.dark) } }
}

@MainActor
final class TerminalModel: ObservableObject {
    @Published var screen: RenderFrame?
    @Published var sessions: [RemoteSession] = []
    @Published var devices: [AccountDevice] = []
    @Published private(set) var sessionSnapshots: [String: [RemoteSession]] = [:]
    @Published var username = ""
    @Published var server = ""
    @Published var busy = false
    @Published var connected = false
    @Published var error: String?
    @Published var deviceID = ""
    @Published var status = "登录后选择 Desktop"
    @Published var hasControl = false
    @Published var desktopAttached = false
    @Published var sessionExited = false
    @Published var history = ""
    @Published var historyLoading = false
    private var accountBusy = false
    private var sessionsRefreshInFlight = false
    @Published private(set) var generation = 0
    private let performanceLog = OSLog(subsystem: "dev.aiterminal", category: .pointsOfInterest)
    let core = RemoteTerminal()
    private let account = Account()
    private let worker = DispatchQueue(label: "dev.aiterminal.account")
    // Read and written only on worker, including assistant calls and channel changes.
    private let channelState = WorkerChannelState()
    @Published private(set) var selected: String?
    var displayCore: RemoteTerminal? { selected == nil || !connected ? nil : core }
    var currentSession: RemoteSession? { sessions.first { $0.id == selected } }
    var readOnlyReason: String {
        if !connected { return "Desktop 已断开，只能查看当前画面" }
        if sessionExited { return "会话已结束，只能查看历史" }
        if !desktopAttached { return "Desktop 已离开；在桌面重新 --attach 后可输入" }
        return "当前设备只有只读权限"
    }
    var deviceName: String { devices.first { $0.id == deviceID }?.name ?? "Desktop" }
    var identity: ChatIdentity? { server.isEmpty || username.isEmpty ? nil : ChatIdentity(server: server, account: username) }
    var chatScope: ChatScope? {
        guard let identity, let selected, !deviceID.isEmpty else { return nil }
        return ChatScope(identity: identity, device: deviceID, session: selected)
    }
    var fixture = false
    init() {
        #if DEBUG
        if ProcessInfo.processInfo.arguments.contains("--login-fixture") { return }
        if ProcessInfo.processInfo.arguments.contains("--workspace-fixture") {
            fixture = true; status = "本地 UI 验证 · 未连接"
            if ProcessInfo.processInfo.arguments.contains("--devices-fixture") {
                devices = [AccountDevice(id: "online", name: "Online Desktop", platform: "desktop", online: true, current: false),
                           AccountDevice(id: "offline", name: "Old iPhone", platform: "ios", online: false, current: false)]
            }
            if ProcessInfo.processInfo.arguments.contains("--pending-connection-fixture") {
                busy = true
                let version = generation
                worker.asyncAfter(deadline: .now() + 10) { [weak self] in
                    self?.failed(ChatFailure.message("stale-operation-error"), version: version)
                }
            }
            if let url = Bundle.main.url(forResource: "screen", withExtension: "pb"), let data = try? Data(contentsOf: url) {
                let replica = TerminalReplica()
                if (try? replica.applySnapshot(bytes: data)) != nil { screen = try? replica.frame() }
            }
            return
        }
        if ProcessInfo.processInfo.arguments.contains("--logout-fixture") {
            worker.async { [weak self] in guard let self else { return }; if let value = PairingStore.loadAccount() { try? self.account.restore(value: value); try? self.account.logout() }; do { try PairingStore.clearAccount() } catch { self.failed(error) } }
            return
        }
        if ProcessInfo.processInfo.arguments.contains("--render-fixture"),
           let url = Bundle.main.url(forResource: "screen", withExtension: "pb"), let data = try? Data(contentsOf: url) {
            let replica = TerminalReplica()
            if (try? replica.applySnapshot(bytes: data)) != nil { screen = try? replica.frame(); status = "本地画面验证"; if let screen { TerminalBenchmark.run(screen) }; return }
        }
        if let path = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_ACCOUNT_FILE"], let data = try? Data(contentsOf: URL(fileURLWithPath: path)), let config = (try? JSONSerialization.jsonObject(with: data)) as? [String: String], let server = config["server"], let username = config["username"], let password = config["password"], let session = config["session"] {
            worker.async { [weak self] in guard let self else { return }; do {
                try self.account.login(server: server, username: username, password: password, name: "iOS Simulator", platform: "ios", caPem: "")
                try self.persist()
                let until = Date().addingTimeInterval(10)
                var desktop: AccountDevice?
                repeat { desktop = try self.account.devices().first(where: { $0.platform == "desktop" && $0.online }); if desktop == nil { Thread.sleep(forTimeInterval: 0.1) } } while desktop == nil && Date() < until
                guard let desktop else { throw CocoaError(.fileNoSuchFile) }
                try self.account.connect(deviceId: desktop.id, terminal: self.core)
                if ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_RELAY_ONLY"] == "1" { try self.core.useRelay() }
                try self.loadDevices(); let sessions = try self.core.sessions()
                self.channelState.device = desktop.id
                DispatchQueue.main.async { self.deviceID = desktop.id; self.sessions = sessions; self.select(session, control: true) }
            } catch { self.failed(error) } }
            return
        }
        if let path = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_INVITATION_FILE"], let value = try? String(contentsOfFile: path, encoding: .utf8) { legacyConnect(value); return }
        if ProcessInfo.processInfo.arguments.contains("--connect-fixture") { legacyConnect(""); return }
        #endif
        busy = true; accountBusy = true
        worker.async { [weak self] in
            guard let self else { return }
            do { if let value = PairingStore.loadAccount() { try self.account.restore(value: value); try self.loadDevices() } else { DispatchQueue.main.async { self.accountBusy = false; self.busy = false } } }
            catch { self.failed(error) }
        }
    }
    nonisolated private func persist() throws {
        let value = try account.export()
        if value.isEmpty { try PairingStore.clearAccount() } else { try PairingStore.saveAccount(value) }
    }
    nonisolated private func loadDevices() throws {
        defer { do { try persist() } catch { failed(error) } }
        let name = account.username()
        let exported = try account.export()
        let object = (try? JSONSerialization.jsonObject(with: Data(exported.utf8))) as? [String: Any]
        let server = object?["server"] as? String ?? ""
        DispatchQueue.main.async { self.username = name; self.server = server }
        let devices = try account.devices()
        DispatchQueue.main.async {
            self.devices = devices; self.username = name; self.accountBusy = false; self.busy = false
            if !self.connected { self.status = "选择在线 Desktop"; self.restoreLastTerminal() }
        }
    }
    nonisolated private func failed(_ error: Error, version: Int? = nil) {
        DispatchQueue.main.async {
            guard version == nil || self.generation == version else { return }
            if version == nil { self.accountBusy = false }
            self.status = terminalError(error); self.error = terminalError(error); self.busy = false
        }
    }
    func login(server: String, username: String, password: String) {
        guard !busy else { return }; busy = true; accountBusy = true; error = nil; status = "登录中…"
        let deviceName = UIDevice.current.name
        worker.async { [weak self] in
            guard let self else { return }
            do {
                #if DEBUG
                let testCA = WorkspacePreferences.serviceTest ? ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_CA_PEM"] : nil
                #else
                let testCA: String? = nil
                #endif
                let ca = testCA ?? Bundle.main.url(forResource: "server-ca", withExtension: "pem").flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
                try self.account.login(server: server, username: username, password: password, name: deviceName, platform: "ios", caPem: ca)
                try self.persist(); try self.loadDevices()
            } catch { self.failed(error) }
        }
    }
    func refreshDevices() { guard !busy else { return }; busy = true; accountBusy = true; worker.async { [weak self] in do { try self?.loadDevices() } catch { self?.failed(error) } } }
    func resume() { if !username.isEmpty, !connected, !busy { refreshDevices() } }
    private func restoreLastTerminal() {
        guard !busy, !connected, let identity, let last = RecentTerminal.load(identity) else { return }
        guard devices.contains(where: { $0.id == last.device && $0.platform == "desktop" && $0.online }) else { status = "上次的 Desktop 当前离线"; return }
        connectDevice(last.device, sessionID: last.session)
    }
    func presence(_ scope: ChatScope) -> String {
        guard scope.identity == identity else { return "待确认" }
        if sessionSnapshots[scope.device]?.contains(where: { $0.id == scope.session && $0.exited }) == true { return "已关闭" }
        if scope.device == deviceID && connected {
            guard let session = sessions.first(where: { $0.id == scope.session }) else { return "已关闭" }
            return session.exited ? "已关闭" : "在线"
        }
        if devices.first(where: { $0.id == scope.device })?.online == false { return "离线" }
        return "待确认"
    }
    func connectDevice(_ id: String, sessionID: String? = nil) {
        guard !busy else { return }; busy = true; error = nil; connected = false; deviceID = id; sessions = []
        generation += 1; let version = generation; selected = nil; screen = nil; hasControl = false; desktopAttached = false; sessionExited = false; status = "连接中…"
        worker.async { [weak self] in
            guard let self else { return }
            defer { do { try self.persist() } catch { self.failed(error, version: version) } }
            do {
                self.channelState.device = ""; try self.core.disconnect(); try self.account.connect(deviceId: id, terminal: self.core); self.channelState.device = id
                let sessions = try self.core.sessions()
                DispatchQueue.main.async {
                    guard self.generation == version else { return }
                    self.sessions = sessions; self.sessionSnapshots[id] = sessions; self.busy = false; self.connected = true; self.status = "已连接 · 选择会话"
                    if let sessionID, sessions.contains(where: { $0.id == sessionID && !$0.exited }) {
                        self.select(sessionID, control: true)
                    } else if let first = sessions.first(where: { !$0.exited }) {
                        if sessionID != nil { self.status = "上次会话已关闭，已恢复在线终端" }
                        self.select(first.id, control: true)
                    } else if sessionID != nil {
                        self.error = "历史对应的终端已关闭，对话仅供查阅"
                    }
                }
            } catch { self.failed(error, version: version) }
        }
    }
    func revoke(_ id: String) {
        guard !busy else { return }; pause(); busy = true; accountBusy = true
        worker.async { [weak self] in guard let self else { return }; defer { do { try self.persist() } catch { self.failed(error) } }; do { try self.account.revoke(deviceId: id); if self.account.username().isEmpty { DispatchQueue.main.async { self.username = ""; self.server = ""; self.devices = []; self.sessions = []; self.deviceID = ""; self.accountBusy = false; self.busy = false } } else { try self.loadDevices() } } catch { self.failed(error) } }
    }
    func logout() {
        guard !busy else { return }; pause(); busy = true; accountBusy = true; username = ""; server = ""; devices = []; sessions = []; sessionSnapshots = [:]; deviceID = ""
        worker.async { [weak self] in guard let self else { return }
            do { try self.account.logout() } catch { self.failed(error) }
            do { try self.persist() } catch { self.failed(error) }
            DispatchQueue.main.async { self.username = ""; self.devices = []; self.sessions = []; self.screen = nil; self.accountBusy = false; self.busy = false }
        }
    }
    func changePassword(current: String, next: String) {
        guard !busy else { return }; pause(); busy = true; accountBusy = true
        worker.async { [weak self] in guard let self else { return }; defer { do { try self.persist() } catch { self.failed(error) } }; do { try self.account.changePassword(current: current, newPassword: next); DispatchQueue.main.async { self.username = ""; self.server = ""; self.devices = []; self.sessions = []; self.accountBusy = false; self.busy = false; self.status = "密码已更新，请重新登录" } } catch { self.failed(error) } }
    }
    func legacyConnect(_ invitation: String) {
        guard !busy else { return }; busy = true; error = nil
        generation += 1; let version = generation; selected = nil; hasControl = false; desktopAttached = false; sessionExited = false
        worker.async { [weak self] in guard let self else { return }
            do {
                guard self.account.username().isEmpty else { throw CocoaError(.userCancelled) }
                let value = invitation.isEmpty ? PairingStore.load() ?? "" : invitation
                try self.core.connect(invitation: value)
                #if DEBUG
                if ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_RELAY_ONLY"] == "1" { try self.core.useRelay() }
                #endif
                try PairingStore.save(value); let sessions = try self.core.sessions()
                DispatchQueue.main.async { guard self.generation == version else { return }; self.sessions = sessions; self.busy = false; self.connected = true; self.status = "旧版配对 · 选择会话"
                    #if DEBUG
                    let expected = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_SESSION_ID"]
                    if ProcessInfo.processInfo.arguments.contains("--connect-fixture"), let first = sessions.first(where: { expected == nil || $0.id == expected }) { self.select(first.id, control: ProcessInfo.processInfo.arguments.contains("--input-fixture")) }
                    #endif
                }
            } catch { self.failed(error, version: version) }
        }
    }
    func select(_ id: String, control: Bool) {
        guard !busy else { return }; busy = true; error = nil
        generation += 1; let version = generation; selected = nil; hasControl = false; desktopAttached = false; sessionExited = false; screen = nil; history = ""
        worker.async { [weak self] in guard let self else { return }
            do { let frame = try self.core.select(id: id, takeControl: control)
                #if DEBUG
                if ProcessInfo.processInfo.arguments.contains("--input-fixture") {
                    let marker = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_INPUT_MARKER"] ?? "SIMULATOR_INPUT_OK"
                    guard marker.count <= 64, !marker.isEmpty, marker.utf8.allSatisfy({ (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0) || $0 == 95 }) else { throw CocoaError(.validationMissingMandatoryProperty) }
                    try self.core.sendText(text: "printf '\(marker)\\n'", submit: true)
                }
                #endif
                let controlled = self.core.hasControl()
                let attached = self.core.desktopAttached(); let exited = self.core.sessionExited()
                DispatchQueue.main.async {
                    guard self.generation == version else { return }
                    self.selected = id; self.screen = frame; self.hasControl = controlled; self.desktopAttached = attached; self.sessionExited = exited; self.connected = true; self.busy = false
                    if let identity = self.identity, !self.deviceID.isEmpty, self.currentSession?.exited != true { RecentTerminal(device: self.deviceID, session: id).save(identity) }
                }
            } catch { self.failed(error, version: version) }
        }
    }
    // These FFI methods only enqueue bounded work; they never wait for a network acknowledgement.
    func text(_ value: String) -> Bool { os_signpost(.event, log: performanceLog, name: "InputEnqueue"); do { try core.sendText(text: value, submit: false); return true } catch { status = terminalError(error); return false } }
    func key(_ value: String) { os_signpost(.event, log: performanceLog, name: "InputEnqueue"); do { try core.sendKey(key: value) } catch { status = terminalError(error) } }
    func displayStatus(_ frame: RenderFrame?, _ path: String, _ controlled: Bool, _ error: String?) {
        if hasControl != controlled { hasControl = controlled }
        let attached = core.desktopAttached(); let exited = core.sessionExited()
        if desktopAttached != attached { desktopAttached = attached }
        if sessionExited != exited { sessionExited = exited }
        if error != nil { connected = false; hasControl = false }
        let availability = exited ? "会话已结束 · 只读历史" : (!attached ? "Desktop 已离开 · 只读历史" : (controlled ? "可输入" : "只读权限"))
        let next = error ?? ((path == "direct" ? "直连" : "中转") + " · " + availability)
        if status != next { status = next }
        #if DEBUG
        if let frame { recordTestStatus(frame, path: path) }
        #endif
    }
    func pause() {
        generation += 1; busy = accountBusy; selected = nil; hasControl = false; desktopAttached = false; sessionExited = false; connected = false; screen = nil; history = ""; historyLoading = false; status = "已断开，选择设备恢复连接"
        worker.async { [weak self] in self?.channelState.device = ""; try? self?.core.disconnect() }
    }
    func create(_ path: String) {
        guard connected, !busy else { return }; busy = true; let version = generation
        worker.async { [weak self] in guard let self else { return }; do {
            let created = try self.core.createSession(cwd: path); let sessions = try self.core.sessions()
            DispatchQueue.main.async { guard self.generation == version else { return }; self.sessions = sessions; self.sessionSnapshots[self.deviceID] = sessions; self.busy = false; self.select(created.id, control: true) }
        } catch { self.failed(error, version: version) } }
    }
    func closeSelected() {
        guard selected != nil, connected, !busy else { return }; busy = true; let version = generation
        worker.async { [weak self] in guard let self else { return }; do {
            try self.core.closeSelected(); let sessions = try self.core.sessions()
            DispatchQueue.main.async {
                guard self.generation == version else { return }
                if let identity = self.identity, RecentTerminal.load(identity)?.session == self.selected { RecentTerminal.remove(identity) }
                self.generation += 1; self.selected = nil; self.screen = nil; self.hasControl = false; self.desktopAttached = false; self.sessionExited = false; self.sessions = sessions; self.sessionSnapshots[self.deviceID] = sessions; self.busy = false; self.status = "会话已关闭"
            }
        } catch { self.failed(error, version: version) } }
    }
    func refreshSessions() {
        guard connected, !busy, !sessionsRefreshInFlight else { return }
        sessionsRefreshInFlight = true
        let version = generation; let device = deviceID
        worker.async { [weak self] in guard let self else { return }
            do {
                let sessions = try self.core.sessions()
                DispatchQueue.main.async {
                    self.sessionsRefreshInFlight = false
                    guard self.generation == version, self.connected, self.deviceID == device else { return }
                    self.sessions = sessions; self.sessionSnapshots[device] = sessions
                    if let selected = self.selected, sessions.first(where: { $0.id == selected })?.exited == true { self.status = "当前会话已关闭" }
                }
            } catch {
                DispatchQueue.main.async {
                    self.sessionsRefreshInFlight = false
                    if self.generation == version { self.error = terminalError(error) }
                }
            }
        }
    }
    func readHistory() {
        let version = generation
        history = ""; historyLoading = true
        worker.async { [weak self] in guard let self else { return }
            do { let text = try self.core.readHistory().joined(separator: "\n"); DispatchQueue.main.async { if self.generation == version { self.history = text; self.historyLoading = false } } }
            catch { DispatchQueue.main.async { if self.generation == version { self.status = terminalError(error); self.history = terminalError(error); self.historyLoading = false } } }
        }
    }
    func assistant(scope: ChatScope, json: String) async throws -> AssistantResponse {
        guard connected, chatScope == scope else { throw ChatFailure.message("终端连接已变化") }
        return try await withCheckedThrowingContinuation { continuation in
            worker.async { [weak self] in
                guard let self else { continuation.resume(throwing: ChatFailure.message("连接已关闭")); return }
                do {
                    let exported = try self.account.export()
                    let object = (try? JSONSerialization.jsonObject(with: Data(exported.utf8))) as? [String: Any]
                    guard self.channelState.device == scope.device,
                          self.account.username() == scope.identity.account,
                          object?["server"] as? String == scope.identity.server else { throw ChatFailure.message("账号或设备连接已变化") }
                    let value = try self.core.assistant(sessionId: scope.session, requestJson: json)
                    continuation.resume(returning: try JSONDecoder().decode(AssistantResponse.self, from: Data(value.utf8)))
                } catch { continuation.resume(throwing: error) }
            }
        }
    }
    #if DEBUG
    private func recordTestStatus(_ screen: RenderFrame, path: String) {
        guard let file = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_STATUS_FILE"], let marker = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_INPUT_MARKER"] else { return }
        let lines = (0..<Int(screen.rows)).map { row in screen.cells[(row * Int(screen.cols))..<((row + 1) * Int(screen.cols))].filter { $0.width > 0 }.map(\.text).joined().trimmingCharacters(in: .whitespaces) }
        let value: [String: Any] = ["marker": marker, "observed": lines.contains(marker), "path": path, "session": selected ?? "", "controlled": hasControl]
        if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) { try? data.write(to: URL(fileURLWithPath: file), options: .atomic) }
    }
    #endif
}


func terminalError(_ error: Error) -> String { if case CoreError.InvalidFrame(let reason) = error { return reason }; return error.localizedDescription }

private final class WorkerChannelState { var device = "" }
