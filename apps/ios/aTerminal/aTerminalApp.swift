import SwiftUI
import os.signpost

@main
struct aTerminalApp: App {
    init() { UITextView.appearance().backgroundColor = .clear }
    var body: some Scene { WindowGroup { WorkspaceScreen().defaultAppStorage(WorkspacePreferences.defaults).preferredColorScheme(.dark) } }
}

@MainActor
final class TerminalModel: ObservableObject {
    @Published var screen: RenderFrame?
    @Published var sessions: [RemoteSession] = []
    @Published var devices: [AccountDevice] = []
    @Published private var sessionSnapshotStore = OwnedSessionSnapshots<RemoteSession>()
    var sessionSnapshots: [String: [RemoteSession]] { sessionSnapshotStore.snapshots(for: identity) }
    @Published var username = ""
    @Published var server = ""
    @Published var busy = false
    @Published var connected = false
    @Published private(set) var preparingWorkspace = false
    @Published private(set) var reconnecting = false
    @Published private(set) var authenticationRequired = false
    private var resumeControl = false
    private var establishedDevice: String?
    private var latestFrame: RenderFrame?
    private var heartbeatBusy = false
    private var foreground = true
    @Published var error: String?
    @Published var deviceID = ""
    @Published var status = "登录后选择 Desktop"
    @Published var hasControl = false
    @Published var desktopAttached = false
    @Published var sessionExited = false
    @Published var history = ""
    @Published var historyLoading = false
    @Published var historyHasMore = false
    @Published var historySummary = ""
    private var historyCursor: TerminalHistoryCursor?
    private var historyVersion = 0
    private let historyWorker = DispatchQueue(label: "terminal.history")
    #if DEBUG
    private var localTestCA: String?
    private var localTestLogin = false
    #endif
    private var accountBusy = false
    private var sessionsRefreshInFlight = false
    @Published private(set) var generation = 0 {
        didSet { channelState.advance(to: generation) }
    }
    private let performanceLog = OSLog(subsystem: "dev.aiterminal", category: .pointsOfInterest)
    let core = RemoteTerminal()
    private let account = Account()
    nonisolated private let accountPersistence = AccountPersistence()
    private let worker = DispatchQueue(label: "dev.aiterminal.account")
    // Read and written only on worker, including assistant calls and channel changes.
    private let channelState = WorkerChannelState()
    @Published private(set) var selected: String?
    var displayCore: RemoteTerminal? {
        WorkspaceRecovery.canPollDisplay(selected: selected, connected: connected, busy: busy, sessionExited: sessionExited) ? core : nil
    }
    var canInput: Bool { connected && !busy && !reconnecting && !authenticationRequired && hasControl && desktopAttached && !sessionExited }
    private var hasWorkspaceContext: Bool { !deviceID.isEmpty && (selected != nil || establishedDevice == deviceID) }
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
        if ProcessInfo.processInfo.arguments.contains("--workspace-fixture") || ProcessInfo.processInfo.arguments.contains("--settings-fixture") {
            fixture = true; status = "本地 UI 验证 · 未连接"
            if ProcessInfo.processInfo.arguments.contains("--settings-fixture") {
                server = "https://fixture.invalid"; username = "fixture"; deviceID = "fixture-desktop"; selected = "fixture-session"
            }
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
            worker.async { [weak self] in guard let self else { return }; do { let value = try PairingStore.loadAccount(); try self.accountPersistence.clear(); if let value { try self.account.restore(value: value); try self.account.logout() } } catch { self.failed(error) } }
            return
        }
        if ProcessInfo.processInfo.arguments.contains("--render-fixture"),
           let url = Bundle.main.url(forResource: "screen", withExtension: "pb"), let data = try? Data(contentsOf: url) {
            let replica = TerminalReplica()
            if (try? replica.applySnapshot(bytes: data)) != nil { screen = try? replica.frame(); status = "本地画面验证"; if let screen { TerminalBenchmark.run(screen) }; return }
        }
        if WorkspacePreferences.serviceTest && ProcessInfo.processInfo.arguments.contains("--local-login-fixture") {
            localTestLogin = true
            do {
                let url = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("local-login-fixture.json")
                let data = try Data(contentsOf: url)
                guard let value = try JSONSerialization.jsonObject(with: data) as? [String: String],
                      let server = value["server"], let username = value["username"], let password = value["password"] else { throw ChatFailure.message("专用测试登录文件格式无效") }
                localTestCA = value["ca_pem"] ?? value["ca"]
                login(server: server, username: username, password: password)
            } catch { self.error = "专用测试登录文件读取失败"; status = "专用测试登录文件读取失败" }
            return
        }
        if let path = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_ACCOUNT_FILE"], let data = try? Data(contentsOf: URL(fileURLWithPath: path)), let config = (try? JSONSerialization.jsonObject(with: data)) as? [String: String], let server = config["server"], let username = config["username"], let password = config["password"], let session = config["session"] {
            let epoch = accountPersistence.epoch
            worker.async { [weak self] in guard let self else { return }; do {
                try self.account.login(server: server, username: username, password: password, name: "iOS Simulator", platform: "ios", caPem: "")
                try self.persist(epoch)
                let until = Date().addingTimeInterval(10)
                var desktop: AccountDevice?
                repeat { desktop = try self.account.devices().first(where: { $0.platform == "desktop" && $0.online }); if desktop == nil { Thread.sleep(forTimeInterval: 0.1) } } while desktop == nil && Date() < until
                guard let desktop else { throw CocoaError(.fileNoSuchFile) }
                try self.account.connect(deviceId: desktop.id, terminal: self.core)
                if ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_RELAY_ONLY"] == "1" { try self.core.useRelay() }
                try self.loadDevices(epoch); let sessions = try self.core.sessions()
                self.channelState.device = desktop.id
                DispatchQueue.main.async { self.deviceID = desktop.id; self.sessions = sessions; self.select(session, control: true) }
            } catch { self.failed(error) } }
            return
        }
        if let path = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_INVITATION_FILE"], let value = try? String(contentsOfFile: path, encoding: .utf8) { legacyConnect(value); return }
        if ProcessInfo.processInfo.arguments.contains("--connect-fixture") { legacyConnect(""); return }
        #endif
        busy = true; accountBusy = true; preparingWorkspace = true; status = "正在恢复工作空间…"
        let epoch = accountPersistence.epoch
        worker.async { [weak self] in
            guard let self else { return }
            do { if let value = try PairingStore.loadAccount() { try self.account.restore(value: value); try self.loadDevices(epoch) } else { DispatchQueue.main.async { self.accountBusy = false; self.busy = false; self.preparingWorkspace = false } } }
            catch { self.failed(error) }
        }
    }
    nonisolated private func persist(_ epoch: Int) throws {
        guard !WorkspacePreferences.fixture else { return }
        let value = try account.export()
        try accountPersistence.commit(epoch) {
            if value.isEmpty { try PairingStore.clearAccount() } else { try PairingStore.saveAccount(value) }
        }
    }
    nonisolated private func loadDevices(_ epoch: Int, version: Int? = nil) throws {
        guard !WorkspacePreferences.fixture else { return }
        guard accountPersistence.epoch == epoch else { return }
        defer { do { try persist(epoch) } catch { failed(error, version: version) } }
        let name = account.username()
        let exported = try account.export()
        let object = (try? JSONSerialization.jsonObject(with: Data(exported.utf8))) as? [String: Any]
        let server = object?["server"] as? String ?? ""
        let owner = name.isEmpty || server.isEmpty ? nil : ChatIdentity(server: server, account: name)
        DispatchQueue.main.async {
            guard self.accountPersistence.epoch == epoch, version == nil || self.generation == version else { return }
            // Clear old-owner presentation before publishing a different account/server.
            self.sessionSnapshotStore.bind(to: owner)
            if self.identity != owner {
                self.generation += 1; self.establishedDevice = nil; self.sessions = []; self.devices = []; self.selected = nil; self.deviceID = ""
                self.screen = nil; self.latestFrame = nil; self.history = ""; self.connected = false; self.reconnecting = false; self.authenticationRequired = false; self.hasControl = false; self.desktopAttached = false; self.sessionExited = false
                self.sessionsRefreshInFlight = false
            }
            if self.username.isEmpty { self.preparingWorkspace = true }
            self.username = name; self.server = server
        }
        let devices = try account.devices()
        DispatchQueue.main.async {
            guard self.accountPersistence.epoch == epoch, self.identity == owner, version == nil || self.generation == version else { return }
            self.devices = devices; self.username = name; self.accountBusy = false; self.busy = false
            guard self.foreground else { return }
            if !self.connected {
                // Recovery belongs to the current desktop/session, never another online device.
                if self.hasWorkspaceContext {
                    self.preparingWorkspace = false
                    guard !self.authenticationRequired else { return }
                    self.reconnecting = true
                    if devices.contains(where: { $0.id == self.deviceID && $0.platform == "desktop" && $0.online }) {
                        self.connectDevice(self.deviceID, sessionID: self.selected, recovering: true)
                    } else { self.status = "Desktop 暂时离线，保留当前页面并等待重连" }
                    return
                }
                self.status = "选择在线 Desktop"
                let desktops = devices.filter { $0.online && !$0.current && $0.platform == "desktop" }
                if desktops.count == 1 { self.connectDevice(desktops[0].id) }
                else {
                    self.restoreLastTerminal()
                    if !self.busy {
                        self.status = desktops.isEmpty ? "暂无在线 Desktop，等待自动连接" : "请选择要连接的 Desktop"
                        self.preparingWorkspace = false
                    }
                }
            }
        }
    }
    nonisolated private func failed(_ error: Error, version: Int? = nil, accountOperation: Bool = false) {
        DispatchQueue.main.async {
            guard version == nil || self.generation == version else { return }
            if version == nil || accountOperation { self.accountBusy = false }
            let message = terminalError(error)
            self.busy = false; self.preparingWorkspace = false
            if WorkspaceRecovery.requiresAuthentication(message) {
                if self.hasWorkspaceContext { self.preserveDisconnectedWorkspace(status: "连接授权失效，请检查账号与设备") }
                else { self.generation += 1 }
                self.authenticationRequired = true; self.reconnecting = false
                self.connected = false; self.hasControl = false
                self.status = "连接授权失效，请检查账号与设备"; self.error = message
            } else if self.hasWorkspaceContext && !self.connected {
                self.reconnecting = true
                self.status = "连接暂时中断，正在后台重试"; self.error = nil
            } else { self.status = message; self.error = message }
        }
    }
    func login(server: String, username: String, password: String) {
        guard !WorkspacePreferences.fixture else { error = "UI fixture 不连接真实服务"; return }
        guard !busy else { return }; busy = true; accountBusy = true; error = nil; status = "正在登录并连接终端…"
        let deviceName = UIDevice.current.name
        #if DEBUG
        let privateCA = localTestCA; let privateLogin = localTestLogin
        #endif
        let epoch = accountPersistence.epoch
        worker.async { [weak self] in
            guard let self else { return }
            do {
                #if DEBUG
                let testCA = WorkspacePreferences.serviceTest ? (privateCA ?? ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_CA_PEM"]) : nil
                #else
                let testCA: String? = nil
                #endif
                let ca = testCA ?? (Bundle.main.url(forResource: "server-ca", withExtension: "pem").flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? WorkspacePreferences.serverCA)
                try self.account.login(server: server, username: username, password: password, name: deviceName, platform: "ios", caPem: ca)
                try self.persist(epoch); try self.loadDevices(epoch)
            } catch {
                #if DEBUG
                if privateLogin { self.failed(ChatFailure.message("专用测试登录或连接失败")); return }
                #endif
                self.failed(error)
            }
        }
    }
    func refreshDevices() {
        guard !WorkspacePreferences.fixture, !busy, !authenticationRequired else { return }
        busy = true; accountBusy = true
        let epoch = accountPersistence.epoch; let version = generation
        worker.async { [weak self] in
            do { try self?.loadDevices(epoch, version: version) }
            catch { self?.failed(error, version: version, accountOperation: true) }
        }
    }
    func resume() {
        foreground = true
        guard !fixture, !username.isEmpty, !connected, !busy, !authenticationRequired else { return }
        preparingWorkspace = !hasWorkspaceContext
        refreshDevices()
    }
    func suspend() {
        foreground = false
        if hasWorkspaceContext {
            preserveDisconnectedWorkspace(status: "连接已暂停，返回后自动重连")
        } else { pause(); preparingWorkspace = !username.isEmpty }
    }
    func retryConnection() {
        guard foreground, !busy, !authenticationRequired else { return }
        refreshDevices()
    }
    private func preserveDisconnectedWorkspace(status: String) {
        resumeControl = WorkspaceRecovery.controlAfterDisconnect(connected: connected, hasControl: hasControl, previous: resumeControl)
        generation += 1; busy = false; accountBusy = false; connected = false; hasControl = false
        // Cursors belong to the failed channel. Keep text, retire in-flight pages,
        // and never release an old cursor through a replacement connection.
        historyVersion += 1; historyLoading = false; historyHasMore = false; historyCursor = nil
        screen = latestFrame ?? screen; preparingWorkspace = false; reconnecting = !authenticationRequired
        self.status = status
        worker.async { [weak self] in self?.channelState.device = ""; try? self?.core.disconnect() }
    }
    func heartbeat() {
        guard foreground, !username.isEmpty, !fixture, !busy, !heartbeatBusy, !preparingWorkspace, !authenticationRequired else { return }
        heartbeatBusy = true
        let discover = !connected; let version = generation
        if discover { busy = true; accountBusy = true }
        let epoch = accountPersistence.epoch
        worker.async { [weak self] in
            guard let self else { return }
            defer { DispatchQueue.main.async {
                self.heartbeatBusy = false
                if self.accountPersistence.epoch == epoch, self.foreground, self.connected, self.selected == nil { self.refreshSessions() }
            } }
            guard self.accountPersistence.epoch == epoch else { return }
            do {
                try self.account.heartbeat()
                if discover { try self.loadDevices(epoch, version: version) } else { try self.persist(epoch) }
            } catch { self.failed(error, version: version, accountOperation: discover) }
        }
    }
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
    func connectDevice(_ id: String, sessionID: String? = nil, recovering: Bool = false) {
        guard !WorkspacePreferences.fixture, !authenticationRequired else { return }
        guard !busy else { return }
        let preserving = WorkspaceRecovery.preservesContext(recovering: recovering, established: hasWorkspaceContext, device: deviceID, session: selected, targetDevice: id, targetSession: sessionID)
        busy = true; preparingWorkspace = !preserving; reconnecting = preserving; error = nil; connected = false
        generation += 1; let version = generation
        if !preserving {
            closeHistory(); establishedDevice = nil; deviceID = id; sessions = []; selected = nil; screen = nil; latestFrame = nil
            hasControl = false; desktopAttached = false; sessionExited = false
        }
        status = preserving ? "正在后台重连，当前页面已保留" : "连接中…"
        let owner = identity
        let epoch = accountPersistence.epoch
        worker.async { [weak self] in
            guard let self, self.channelState.matches(version) else { return }
            defer { do { try self.persist(epoch) } catch { self.failed(error, version: version) } }
            do {
                self.channelState.device = ""; try self.core.disconnect()
                guard self.channelState.matches(version) else { return }
                try self.account.connect(deviceId: id, terminal: self.core)
                guard self.channelState.matches(version) else { return }
                self.channelState.device = id
                let sessions = try self.core.sessions()
                DispatchQueue.main.async {
                    guard self.generation == version, self.identity == owner, self.foreground else { return }
                    self.establishedDevice = id
                    self.sessions = sessions; self.sessionSnapshotStore.record(sessions, device: id, owner: owner)
                    self.busy = false
                    if preserving {
                        if let sessionID, sessions.contains(where: { $0.id == sessionID && !$0.exited }) {
                            self.select(sessionID, control: self.resumeControl, recovering: true)
                        } else {
                            self.connected = true; self.reconnecting = false; self.preparingWorkspace = false
                            self.sessionExited = sessionID != nil; self.hasControl = false; self.desktopAttached = false
                            self.status = sessionID == nil ? "已重新连接 Desktop" : "原会话已结束，保留当前页面供查阅"
                        }
                    } else {
                        self.connected = true; self.status = "已连接 · 选择会话"
                        if let sessionID, sessions.contains(where: { $0.id == sessionID && !$0.exited }) {
                            self.select(sessionID, control: true)
                        } else if let first = sessions.first(where: { !$0.exited }) {
                            if sessionID != nil { self.status = "上次会话已关闭，已恢复在线终端" }
                            self.select(first.id, control: true)
                        } else {
                            self.preparingWorkspace = false; self.status = "已连接，暂无运行中的终端"
                            if sessionID != nil { self.error = "历史对应的终端已关闭，对话仅供查阅" }
                        }
                    }
                }
            } catch { self.failed(error, version: version) }
        }
    }
    func revoke(_ id: String) {
        guard !WorkspacePreferences.fixture else { return }
        guard !busy else { return }; pause(); busy = true; accountBusy = true
        let epoch = accountPersistence.epoch; let owner = identity
        worker.async { [weak self] in guard let self else { return }; defer { do { try self.persist(epoch) } catch { self.failed(error) } }; do { try self.account.revoke(deviceId: id); if self.account.username().isEmpty { DispatchQueue.main.async { guard self.accountPersistence.epoch == epoch, self.identity == owner else { return }; self.sessionSnapshotStore.bind(to: nil); self.username = ""; self.server = ""; self.devices = []; self.sessions = []; self.deviceID = ""; self.accountBusy = false; self.busy = false } } else { try self.loadDevices(epoch) } } catch { self.failed(error) } }
    }
    func logout() {
        guard !WorkspacePreferences.fixture else { return }
        guard !busy else { return }
        do { try accountPersistence.clear() } catch { failed(error); return }
        pause(); preparingWorkspace = false; busy = true; accountBusy = true; username = ""; server = ""; devices = []; sessions = []; sessionSnapshotStore.bind(to: nil); deviceID = ""
        worker.async { [weak self] in guard let self else { return }
            do { try self.account.logout() } catch { self.failed(error) }
            DispatchQueue.main.async { self.username = ""; self.devices = []; self.sessions = []; self.screen = nil; self.accountBusy = false; self.busy = false }
        }
    }
    func changePassword(current: String, next: String) {
        guard !WorkspacePreferences.fixture else { return }
        guard !busy else { return }; pause(); busy = true; accountBusy = true
        let epoch = accountPersistence.epoch; let owner = identity
        worker.async { [weak self] in guard let self else { return }; defer { do { try self.persist(epoch) } catch { self.failed(error) } }; do { try self.account.changePassword(current: current, newPassword: next); DispatchQueue.main.async { guard self.accountPersistence.epoch == epoch, self.identity == owner else { return }; self.sessionSnapshotStore.bind(to: nil); self.deviceID = ""; self.username = ""; self.server = ""; self.devices = []; self.sessions = []; self.accountBusy = false; self.busy = false; self.status = "密码已更新，请重新登录" } } catch { self.failed(error) } }
    }
    func legacyConnect(_ invitation: String) {
        guard !WorkspacePreferences.fixture else { return }
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
    func select(_ id: String, control: Bool, recovering: Bool = false) {
        guard !WorkspacePreferences.fixture, !authenticationRequired, !busy else { return }
        let preserving = recovering && selected == id && hasWorkspaceContext
        if !preserving { closeHistory() }
        busy = true; preparingWorkspace = !preserving; error = nil
        generation += 1; let version = generation
        hasControl = false
        if !preserving { selected = nil; desktopAttached = false; sessionExited = false; screen = nil; latestFrame = nil; history = "" }
        worker.async { [weak self] in guard let self, self.channelState.matches(version) else { return }
            do { let frame = try self.core.select(id: id, takeControl: control)
                #if DEBUG
                if !recovering && ProcessInfo.processInfo.arguments.contains("--input-fixture") {
                    let marker = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_INPUT_MARKER"] ?? "SIMULATOR_INPUT_OK"
                    guard marker.count <= 64, !marker.isEmpty, marker.utf8.allSatisfy({ (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0) || $0 == 95 }) else { throw CocoaError(.validationMissingMandatoryProperty) }
                    try self.core.sendText(text: "printf '\(marker)\\n'", submit: true)
                }
                #endif
                let controlled = self.core.hasControl()
                let attached = self.core.desktopAttached(); let exited = self.core.sessionExited()
                DispatchQueue.main.async {
                    guard self.generation == version else { return }
                    self.selected = id; self.screen = frame; self.latestFrame = frame; self.reconnecting = false; self.resumeControl = controlled; self.hasControl = controlled; self.desktopAttached = attached; self.sessionExited = exited; self.connected = true; self.busy = false; self.preparingWorkspace = false
                    if let identity = self.identity, !self.deviceID.isEmpty, self.currentSession?.exited != true { RecentTerminal(device: self.deviceID, session: id).save(identity) }
                }
            } catch { self.failed(error, version: version) }
        }
    }
    // These FFI methods only enqueue bounded work; they never wait for a network acknowledgement.
    func text(_ value: String) -> Bool { guard canInput else { return false }; os_signpost(.event, log: performanceLog, name: "InputEnqueue"); do { try core.typeText(text: value); return true } catch { status = terminalError(error); return false } }
    func paste(_ value: String) -> Bool { guard canInput else { return false }; do { try core.sendText(text: value, submit: false); return true } catch { status = terminalError(error); return false } }
    func key(_ value: String) { guard canInput else { return }; os_signpost(.event, log: performanceLog, name: "InputEnqueue"); do { try core.sendKey(key: value) } catch { status = terminalError(error) } }
    func displayStatus(_ frame: RenderFrame?, _ path: String, _ controlled: Bool, _ error: String?) {
        // A retained closed-session snapshot has no native selection. Ignore an
        // old display-link callback until SwiftUI has removed its previous core.
        guard displayCore != nil else { return }
        if let frame { latestFrame = frame }
        if let error {
            preserveDisconnectedWorkspace(status: "连接暂时中断，正在后台重试")
            if WorkspaceRecovery.requiresAuthentication(error) {
                authenticationRequired = true; reconnecting = false; self.error = error
                status = "连接授权失效，请检查账号与设备"
            } else { heartbeat() }
            return
        }
        if hasControl != controlled { hasControl = controlled }
        resumeControl = controlled
        let attached = core.desktopAttached(); let exited = core.sessionExited()
        if desktopAttached != attached { desktopAttached = attached }
        if sessionExited != exited { sessionExited = exited }
        let availability = exited ? "会话已结束 · 只读历史" : (!attached ? "Desktop 已离开 · 只读历史" : (controlled ? "可输入" : "只读权限"))
        let next = error ?? ((path == "direct" ? "直连" : "中转") + " · " + availability)
        if status != next { status = next }
        #if DEBUG
        if let frame { recordTestStatus(frame, path: path) }
        #endif
    }
    func pause() {
        generation += 1; busy = false; accountBusy = false; reconnecting = false; authenticationRequired = false; selected = nil; latestFrame = nil; establishedDevice = nil; resumeControl = false; hasControl = false; desktopAttached = false; sessionExited = false; connected = false; screen = nil; history = ""; historyLoading = false; status = "已断开，选择设备恢复连接"
        worker.async { [weak self] in self?.channelState.device = ""; try? self?.core.disconnect() }
    }
    func loadRecentDirectories(completion: @escaping ([String], String?) -> Void) {
        guard connected else { completion([], "Desktop 已断开"); return }
        let version = generation; let device = deviceID
        worker.async { [weak self] in guard let self else { return }
            do {
                guard self.channelState.matches(version), self.channelState.device == device else { return }
                let paths = try self.core.recentDirectories()
                DispatchQueue.main.async {
                    guard self.generation == version, self.deviceID == device, self.connected else { return }
                    completion(paths, nil)
                }
            } catch {
                DispatchQueue.main.async {
                    guard self.generation == version, self.deviceID == device, self.connected else { return }
                    completion([], "最近目录读取失败，可输入目录或使用默认目录")
                }
            }
        }
    }
    func create(_ path: String, completion: @escaping (TerminalCreationOutcome) -> Void) {
        guard connected, !busy else { completion(.unavailable("Desktop 当前不可用，请稍后重试")); return }
        busy = true; let version = generation; let device = deviceID; let owner = identity
        worker.async { [weak self] in guard let self else { return }; do {
            guard self.channelState.matches(version), self.channelState.device == device else { return }
            let created = try self.core.createSession(cwd: path)
            // Refresh failure must not prompt the user to create the same session twice.
            let sessions = try? self.core.sessions()
            DispatchQueue.main.async {
                guard self.generation == version, self.identity == owner, self.deviceID == device else { return }
                self.sessions = sessions ?? ([created] + self.sessions)
                self.sessionSnapshotStore.record(self.sessions, device: device, owner: owner); self.busy = false
                completion(.created); self.select(created.id, control: true)
            }
        } catch {
            DispatchQueue.main.async {
                guard self.generation == version, self.identity == owner, self.deviceID == device else { return }
                self.busy = false
                // The server may have created the session before the response was lost.
                // Do not offer another non-idempotent create from the same sheet.
                completion(.unconfirmed(terminalError(error)))
            }
        } }
    }
    func closeSelected() {
        guard selected != nil, connected, !busy else { return }; busy = true; let version = generation; let owner = identity
        worker.async { [weak self] in guard let self, self.channelState.matches(version) else { return }; do {
            try self.core.closeSelected(); let sessions = try self.core.sessions()
            DispatchQueue.main.async {
                guard self.generation == version, self.identity == owner else { return }
                if let identity = self.identity, RecentTerminal.load(identity)?.session == self.selected { RecentTerminal.remove(identity) }
                self.generation += 1; self.selected = nil; self.screen = nil; self.hasControl = false; self.desktopAttached = false; self.sessionExited = false; self.sessions = sessions; self.sessionSnapshotStore.record(sessions, device: self.deviceID, owner: owner); self.busy = false; self.status = "会话已关闭"
            }
        } catch { self.failed(error, version: version) } }
    }
    func refreshSessions() {
        guard connected, !busy, !sessionsRefreshInFlight else { return }
        sessionsRefreshInFlight = true
        let version = generation; let device = deviceID; let owner = identity
        worker.async { [weak self] in guard let self else { return }
            do {
                let sessions = try self.core.sessions()
                DispatchQueue.main.async {
                    self.sessionsRefreshInFlight = false
                    guard self.generation == version, self.identity == owner, self.connected, self.deviceID == device else { return }
                    self.sessions = sessions; self.sessionSnapshotStore.record(sessions, device: device, owner: owner)
                    if self.foreground, self.selected == nil, let first = sessions.first(where: { !$0.exited }) { self.select(first.id, control: true) }
                    if let selected = self.selected, sessions.first(where: { $0.id == selected })?.exited == true { self.status = "当前会话已关闭" }
                }
            } catch {
                DispatchQueue.main.async {
                    self.sessionsRefreshInFlight = false
                    if self.generation == version {
                        self.error = terminalError(error)
                        self.observeTransportFailure(error, version: version)
                    }
                }
            }
        }
    }
    func closeHistory() {
        historyVersion += 1; historyLoading = false; historyHasMore = false
        let cursor = historyCursor; historyCursor = nil
        let version = generation
        if let cursor { worker.async { [weak self] in
            guard let self, self.channelState.matches(version) else { return }
            try? self.core.releaseHistory(cursor: cursor)
        } }
    }
    func readHistory() {
        guard connected, !reconnecting else { historySummary = "连接中断，已保留历史；重连后可重新读取"; return }
        closeHistory(); history = ""; historySummary = ""
        loadEarlierHistory()
    }
    func loadEarlierHistory() {
        guard connected, !reconnecting, !historyLoading else { return }
        let version = generation; let reading = historyVersion; let cursor = historyCursor
        historyLoading = true
        historyWorker.async { [weak self] in guard let self else { return }
            do {
                let page = try self.core.readHistoryPage(cursor: cursor)
                DispatchQueue.main.async {
                    guard self.generation == version, self.historyVersion == reading else {
                        self.worker.async {
                            guard self.channelState.matches(version) else { return }
                            try? self.core.releaseHistory(cursor: page.cursor)
                        }; return
                    }
                    let text = page.lines.joined(separator: "\n")
                    self.history = cursor == nil ? text : text + "\n" + self.history
                    self.historyCursor = page.cursor; self.historyHasMore = page.hasMore
                    self.historySummary = "已加载 \(page.cursor.offset) / \(page.total) 行" + (page.truncated ? " · 更早记录已超出保留范围" : "")
                    self.historyLoading = false
                }
            } catch {
                DispatchQueue.main.async { if self.generation == version && self.historyVersion == reading {
                    self.historySummary = "历史读取失败，请读取最新历史：" + terminalError(error)
                    self.historyHasMore = false; self.historyLoading = false
                } }
            }
        }
    }

    func agent(scope: ChatScope, json: String, configuration: Bool = false) async throws -> String {
        guard !WorkspacePreferences.fixture else { throw ChatFailure.message("UI fixture 禁止真实 RPC") }
        guard connected, !reconnecting, !authenticationRequired, identity == scope.identity, deviceID == scope.device else { throw ChatFailure.message("账号或设备连接已变化") }
        let version = generation
        return try await withCheckedThrowingContinuation { continuation in
            worker.async { [weak self] in
                guard let self else { continuation.resume(throwing: ChatFailure.message("连接已关闭")); return }
                do {
                    let exported = try self.account.export()
                    let object = (try? JSONSerialization.jsonObject(with: Data(exported.utf8))) as? [String: Any]
                    guard self.channelState.matches(version), self.channelState.device == scope.device, self.account.username() == scope.identity.account,
                        object?["server"] as? String == scope.identity.server else { throw ChatFailure.message("账号或设备连接已变化") }
                    let result = try configuration ? self.core.configuration(requestJson: json) : self.core.agent(sessionId: scope.session, requestJson: json)
                    continuation.resume(returning: result)
                } catch { self.observeTransportFailure(error, version: version); continuation.resume(throwing: error) }
            }
        }
    }
    nonisolated private func observeTransportFailure(_ error: Error, version: Int) {
        let message = terminalError(error)
        guard WorkspaceRecovery.isTransportFailure(message) || WorkspaceRecovery.requiresAuthentication(message) else { return }
        DispatchQueue.main.async {
            guard self.generation == version, self.connected else { return }
            self.preserveDisconnectedWorkspace(status: "连接暂时中断，正在后台重试")
            if WorkspaceRecovery.requiresAuthentication(message) {
                self.authenticationRequired = true; self.reconnecting = false; self.error = message
                self.status = "连接授权失效，请检查账号与设备"
            } else { self.heartbeat() }
        }
    }
    func assistant(scope: ChatScope, json: String) async throws -> AssistantResponse {
        guard !WorkspacePreferences.fixture else { throw ChatFailure.message("UI fixture 禁止真实 RPC") }
        guard connected, !reconnecting, !authenticationRequired, chatScope == scope else { throw ChatFailure.message("终端连接已变化") }
        let version = generation
        return try await withCheckedThrowingContinuation { continuation in
            worker.async { [weak self] in
                guard let self else { continuation.resume(throwing: ChatFailure.message("连接已关闭")); return }
                do {
                    let exported = try self.account.export()
                    let object = (try? JSONSerialization.jsonObject(with: Data(exported.utf8))) as? [String: Any]
                    guard self.channelState.matches(version), self.channelState.device == scope.device,
                          self.account.username() == scope.identity.account,
                          object?["server"] as? String == scope.identity.server else { throw ChatFailure.message("账号或设备连接已变化") }
                    let value = try self.core.assistant(sessionId: scope.session, requestJson: json)
                    continuation.resume(returning: try JSONDecoder().decode(AssistantResponse.self, from: Data(value.utf8)))
                } catch { self.observeTransportFailure(error, version: version); continuation.resume(throwing: error) }
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

private final class WorkerChannelState {
    // Device belongs to worker; the cancellation generation is also checked before queued RPCs.
    var device = ""
    private let lock = NSLock()
    private var version = 0
    func advance(to version: Int) { lock.lock(); defer { lock.unlock() }; self.version = version }
    func matches(_ version: Int) -> Bool { lock.lock(); defer { lock.unlock() }; return self.version == version }
}
