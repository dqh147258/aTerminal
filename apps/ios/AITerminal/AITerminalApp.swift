import SwiftUI
import os.signpost

@main
struct AITerminalApp: App { var body: some Scene { WindowGroup { SessionScreen() } } }

@MainActor
final class TerminalModel: ObservableObject {
    @Published var screen: RenderFrame?
    @Published var sessions: [RemoteSession] = []
    @Published var devices: [AccountDevice] = []
    @Published var username = ""
    @Published var status = "登录后选择 Desktop"
    @Published var hasControl = false
    @Published var history = ""
    @Published private(set) var generation = 0
    private let performanceLog = OSLog(subsystem: "dev.aiterminal", category: .pointsOfInterest)
    let core = RemoteTerminal()
    private let account = Account()
    private let worker = DispatchQueue(label: "dev.aiterminal.account")
    private let historyWorker = DispatchQueue(label: "dev.aiterminal.history")
    private var selected: String?
    var displayCore: RemoteTerminal? { selected == nil ? nil : core }
    init() {
        #if DEBUG
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
                DispatchQueue.main.async { self.sessions = sessions; self.select(session, control: true) }
            } catch { self.failed(error) } }
            return
        }
        if let path = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_INVITATION_FILE"], let value = try? String(contentsOfFile: path, encoding: .utf8) { legacyConnect(value); return }
        if ProcessInfo.processInfo.arguments.contains("--connect-fixture") { legacyConnect(""); return }
        #endif
        worker.async { [weak self] in
            guard let self else { return }
            do { if let value = PairingStore.loadAccount() { try self.account.restore(value: value); try self.loadDevices() } }
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
        DispatchQueue.main.async { self.username = name }
        let devices = try account.devices()
        DispatchQueue.main.async { self.devices = devices; self.username = name; self.status = "选择在线 Desktop" }
    }
    nonisolated private func failed(_ error: Error) { DispatchQueue.main.async { self.status = terminalError(error) } }
    func login(server: String, username: String, password: String) {
        status = "登录中…"
        let deviceName = UIDevice.current.name
        worker.async { [weak self] in
            guard let self else { return }
            do {
                let ca = Bundle.main.url(forResource: "server-ca", withExtension: "pem").flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
                try self.account.login(server: server, username: username, password: password, name: deviceName, platform: "ios", caPem: ca)
                try self.persist(); try self.loadDevices()
            } catch { self.failed(error) }
        }
    }
    func refreshDevices() { worker.async { [weak self] in do { try self?.loadDevices() } catch { self?.failed(error) } } }
    func connectDevice(_ id: String) {
        generation += 1; let version = generation; selected = nil; screen = nil; hasControl = false; status = "连接中…"
        worker.async { [weak self] in
            guard let self else { return }
            defer { do { try self.persist() } catch { self.failed(error) } }
            do { try self.core.disconnect(); try self.account.connect(deviceId: id, terminal: self.core); let sessions = try self.core.sessions()
                DispatchQueue.main.async { guard self.generation == version else { return }; self.sessions = sessions; self.status = "已连接 · 选择会话" }
            } catch { self.failed(error) }
        }
    }
    func revoke(_ id: String) {
        pause()
        worker.async { [weak self] in guard let self else { return }; defer { do { try self.persist() } catch { self.failed(error) } }; do { try self.account.revoke(deviceId: id); if self.account.username().isEmpty { DispatchQueue.main.async { self.username = ""; self.devices = [] } } else { try self.loadDevices() } } catch { self.failed(error) } }
    }
    func logout() {
        pause()
        worker.async { [weak self] in guard let self else { return }
            do { try self.account.logout() } catch { self.failed(error) }
            do { try self.persist() } catch { self.failed(error) }
            DispatchQueue.main.async { self.username = ""; self.devices = []; self.sessions = []; self.screen = nil }
        }
    }
    func changePassword(current: String, next: String) {
        pause()
        worker.async { [weak self] in guard let self else { return }; defer { do { try self.persist() } catch { self.failed(error) } }; do { try self.account.changePassword(current: current, newPassword: next); DispatchQueue.main.async { self.username = ""; self.devices = []; self.status = "密码已更新，请重新登录" } } catch { self.failed(error) } }
    }
    func legacyConnect(_ invitation: String) {
        generation += 1; let version = generation; selected = nil; hasControl = false
        worker.async { [weak self] in guard let self else { return }
            do {
                guard self.account.username().isEmpty else { throw CocoaError(.userCancelled) }
                let value = invitation.isEmpty ? PairingStore.load() ?? "" : invitation
                try self.core.connect(invitation: value)
                #if DEBUG
                if ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_RELAY_ONLY"] == "1" { try self.core.useRelay() }
                #endif
                try PairingStore.save(value); let sessions = try self.core.sessions()
                DispatchQueue.main.async { guard self.generation == version else { return }; self.sessions = sessions; self.status = "旧版配对 · 选择会话"
                    #if DEBUG
                    let expected = ProcessInfo.processInfo.environment["AI_TERMINAL_TEST_SESSION_ID"]
                    if ProcessInfo.processInfo.arguments.contains("--connect-fixture"), let first = sessions.first(where: { expected == nil || $0.id == expected }) { self.select(first.id, control: ProcessInfo.processInfo.arguments.contains("--input-fixture")) }
                    #endif
                }
            } catch { self.failed(error) }
        }
    }
    func select(_ id: String, control: Bool) {
        generation += 1; let version = generation; selected = nil; hasControl = false; screen = nil; history = ""
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
                DispatchQueue.main.async { guard self.generation == version else { return }; self.selected = id; self.screen = frame; self.hasControl = controlled }
            } catch { self.failed(error) }
        }
    }
    // These FFI methods only enqueue bounded work; they never wait for a network acknowledgement.
    func text(_ value: String) -> Bool { os_signpost(.event, log: performanceLog, name: "InputEnqueue"); do { try core.sendText(text: value, submit: false); return true } catch { status = terminalError(error); return false } }
    func key(_ value: String) { os_signpost(.event, log: performanceLog, name: "InputEnqueue"); do { try core.sendKey(key: value) } catch { status = terminalError(error) } }
    func control(_ value: Bool) { if let id = selected { select(id, control: value) } }
    func displayStatus(_ frame: RenderFrame?, _ path: String, _ controlled: Bool, _ error: String?) {
        if hasControl != controlled { hasControl = controlled }
        let next = error ?? ((path == "direct" ? "直连" : "中转") + (controlled ? " · 可输入" : " · 只读"))
        if status != next { status = next }
        #if DEBUG
        if let frame { recordTestStatus(frame, path: path) }
        #endif
    }
    func pause() {
        generation += 1; selected = nil; hasControl = false; screen = nil; history = ""; status = "已断开，选择设备恢复连接"
        worker.async { [weak self] in try? self?.core.disconnect() }
    }
    func create(_ path: String) { worker.async { [weak self] in guard let self else { return }; do { _ = try self.core.createSession(cwd: path); let sessions = try self.core.sessions(); DispatchQueue.main.async { self.sessions = sessions } } catch { self.failed(error) } } }
    func readHistory() {
        let version = generation
        history = ""
        historyWorker.async { [weak self] in guard let self else { return }
            do { let text = try self.core.readHistory().joined(separator: "\n"); DispatchQueue.main.async { if self.generation == version { self.history = text } } }
            catch { DispatchQueue.main.async { if self.generation == version { self.status = terminalError(error) } } }
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

struct SessionScreen: View {
    @StateObject private var model = TerminalModel()
    @Environment(\.scenePhase) private var phase
    @State private var server = ""
    @State private var username = ""
    @State private var password = ""
    @State private var invitation = ""
    @State private var legacy = false
    @State private var directory = ""
    @State private var creating = false
    @State private var showingHistory = false
    @State private var changingPassword = false
    @State private var currentPassword = ""
    @State private var newPassword = ""
    @State private var revokeDevice: AccountDevice?
    @State private var zoom = 1.0
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack { Text("AI Terminal").font(.title2.bold()); Spacer(); if !model.username.isEmpty { Menu(model.username) { Button("刷新设备") { model.refreshDevices() }; Button("修改密码") { changingPassword = true }; Button("退出登录") { model.logout() } } } }
            if model.username.isEmpty && model.screen == nil {
                TextField("服务器 https://…", text: $server).textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                TextField("账号", text: $username).textInputAutocapitalization(.never).autocorrectionDisabled().textContentType(.username)
                SecureField("密码", text: $password).textContentType(.password)
                Button("登录") { model.login(server: server, username: username, password: password); password = "" }.disabled(server.isEmpty || username.isEmpty || password.isEmpty)
                DisclosureGroup("旧版配对（迁移用）", isExpanded: $legacy) { SecureField("配对邀请", text: $invitation); Button("连接旧版设备") { model.legacyConnect(invitation); invitation = "" } }
            }
            if !model.devices.isEmpty {
                ScrollView(.horizontal) { HStack { ForEach(model.devices, id: \.id) { device in
                    VStack { Button(device.name + (device.current ? " · 本机" : (device.online ? " · 在线" : " · 离线"))) { model.connectDevice(device.id) }.disabled(device.platform != "desktop" || !device.online)
                        Button("移除设备", role: .destructive) { revokeDevice = device }.font(.caption)
                    }
                } } }.frame(maxHeight: 58)
            }
            HStack {
                Button { creating = true } label: { Image(systemName: "plus") }.accessibilityLabel("新建会话")
                Button { model.readHistory(); showingHistory = true } label: { Image(systemName: "clock") }.accessibilityLabel("历史")
                Toggle("接管输入", isOn: Binding(get: { model.hasControl }, set: { model.control($0) })).disabled(model.screen == nil)
            }
            Text(model.status).font(.caption)
            ScrollView(.horizontal) { HStack { ForEach(model.sessions, id: \.id) { session in Button(session.cwd + (session.exited ? " · 已结束" : "")) { model.select(session.id, control: model.hasControl) } } } }.frame(height: model.sessions.isEmpty ? 0 : 36)
            if let screen = model.screen {
                TerminalSurface(frame: screen, zoom: zoom, generation: model.generation, core: model.displayCore, onStatus: model.displayStatus).frame(maxWidth: .infinity, maxHeight: .infinity)
                Slider(value: $zoom, in: 0.6...1.8).accessibilityLabel("终端字号")
            } else { Spacer() }
            InputComposer(enabled: model.hasControl, send: model.text, key: model.key)
        }.padding()
        .alert("新建会话", isPresented: $creating) { TextField("桌面工作目录", text: $directory); Button("创建") { model.create(directory) }; Button("取消", role: .cancel) {} }
        .alert("修改密码", isPresented: $changingPassword) { SecureField("当前密码", text: $currentPassword); SecureField("新密码（至少 12 字节）", text: $newPassword); Button("保存") { model.changePassword(current: currentPassword, next: newPassword); currentPassword = ""; newPassword = "" }; Button("取消", role: .cancel) { currentPassword = ""; newPassword = "" } }
        .alert("移除设备？", isPresented: Binding(get: { revokeDevice != nil }, set: { if !$0 { revokeDevice = nil } })) { Button("移除", role: .destructive) { if let device = revokeDevice { model.revoke(device.id) }; revokeDevice = nil }; Button("取消", role: .cancel) { revokeDevice = nil } } message: { Text("该设备的登录和远程连接将失效，桌面 Shell 会保留。") }
        .sheet(isPresented: $showingHistory) { ScrollView { Text(model.history).font(.system(.body, design: .monospaced)).textSelection(.enabled).padding() } }
        .onChange(of: phase) { value in if value != .active { model.pause() } }
    }
}
private struct InputComposer: View {
    @State private var text = ""
    let enabled: Bool
    let send: (String) -> Bool
    let key: (String) -> Void
    var body: some View { VStack {
        TextField("输入文字", text: $text).textInputAutocapitalization(.never).autocorrectionDisabled()
        HStack { Button { if send(text) { text = "" } } label: { Image(systemName: "paperplane") }.accessibilityLabel("输入文字")
            ForEach([("回车", "enter"), ("Ctrl-C", "ctrl_c"), ("Tab", "tab"), ("Esc", "escape"), ("↑", "up"), ("↓", "down")], id: \.1) { label, value in Button(label) { key(value) } }
        }.font(.callout).disabled(!enabled)
    } }
}

func terminalError(_ error: Error) -> String { if case CoreError.InvalidFrame(let reason) = error { return reason }; return error.localizedDescription }
