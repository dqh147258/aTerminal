import SwiftUI

private enum WorkspacePanel: String { case settings, chat, devices, account, terminalHistory, chatHistory }

struct WorkspaceScreen: View {
    @StateObject private var model = TerminalModel()
    @StateObject private var assistant = AssistantModel()
    @Environment(\.scenePhase) private var phase
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @AppStorage("terminal.fontSize") private var fontSize = 16.0
    @AppStorage("terminal.overlayOpacity") private var opacity = 88.0
    @State private var drawer = false
    @State private var historyTab = false
    @State private var search = ""
    @State private var panel: WorkspacePanel?
    @State private var inputVisible = false
    @State private var creating = false
    @State private var directory = ""
    @State private var closing = false
    @State private var selectedHistory: ChatArchive?
    @State private var revokeDevice: AccountDevice?
    @State private var logoutConfirm = false
    @State private var terminalDraft = ""
    @State private var continueScope: ChatScope?
    private var workspace: Bool { !model.username.isEmpty || model.connected || model.screen != nil || model.fixture }

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                WorkspaceStyle.background.ignoresSafeArea()
                if workspace { terminalWorkspace } else { LoginScreen(model: model) }
                if drawer {
                    Color.black.opacity(0.4).ignoresSafeArea().onTapGesture { drawer = false }.accessibilityHidden(true)
                    drawerView.frame(width: min(340, geometry.size.width - 28)).frame(maxHeight: .infinity)
                        .background(WorkspaceStyle.surface).transition(.move(edge: .leading)).accessibilityAddTraits(.isModal)
                }
                if let panel {
                    let height = panelHeight(panel, available: geometry.size.height)
                    Color.black.opacity(0.08).ignoresSafeArea().onTapGesture { self.panel = nil }.accessibilityHidden(true)
                    panelContent(panel, height: height).frame(maxWidth: 620)
                        .frame(height: height)
                        .background(WorkspaceStyle.surface.opacity(min(96, max(60, opacity)) / 100))
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(WorkspaceStyle.line))
                        .cornerRadius(8).padding(12).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                        .accessibilityAddTraits(.isModal)
                }
            }.frame(width: geometry.size.width, height: geometry.size.height)
        }
        .foregroundColor(WorkspaceStyle.foreground).tint(WorkspaceStyle.accent)
        .alert("新建会话", isPresented: $creating) {
            TextField("Desktop 工作目录（留空使用默认）", text: $directory)
            Button("创建") { model.create(directory); directory = ""; drawer = false }
            Button("取消", role: .cancel) { directory = "" }
        }
        .alert("关闭当前会话？", isPresented: $closing) {
            Button("关闭会话", role: .destructive) { model.closeSelected() }
            Button("取消", role: .cancel) {}
        } message: { Text("这将结束该终端的 Shell。AI 对话历史仍保留。") }
        .alert("移除设备？", isPresented: Binding(get: { revokeDevice != nil }, set: { if !$0 { revokeDevice = nil } })) {
            Button("移除", role: .destructive) { if let device = revokeDevice { model.revoke(device.id) }; revokeDevice = nil }
            Button("取消", role: .cancel) { revokeDevice = nil }
        } message: { Text("该设备的登录和远程连接将失效，桌面 Shell 会保留。") }
        .alert("退出登录？", isPresented: $logoutConfirm) {
            Button("退出登录", role: .destructive) { panel = nil; drawer = false; assistant.stop(); model.logout(); syncChat() }
            Button("取消", role: .cancel) {}
        }
        .onChange(of: model.identity) { _ in
            syncChat()
            if model.username.isEmpty { panel = nil; drawer = false; selectedHistory = nil; terminalDraft = ""; continueScope = nil }
            else if model.deviceID.isEmpty { panel = .devices }
        }
        .onChange(of: model.chatScope) { scope in
            syncChat(); terminalDraft = ""
            if let scope, scope == continueScope { continueScope = nil; panel = .chat }
        }
        .onChange(of: model.connected) { connected in syncChat(); if connected && panel == .devices { panel = nil } }
        .onChange(of: panel) { value in assistant.setVisible(value == .chat, core: model.core) }
        .onChange(of: phase) { value in
            if value == .background { assistant.stop(); model.pause(); panel = nil; terminalDraft = "" }
            if value == .active { model.resume() }
        }
        .onAppear {
            fontSize = min(24, max(12, fontSize)); opacity = min(96, max(60, opacity))
            syncChat()
            #if DEBUG
            if model.fixture {
                let args = ProcessInfo.processInfo.arguments
                if args.contains("--show-settings") { panel = .settings }
                if args.contains("--show-drawer") { drawer = true }
                if args.contains("--show-chat") { panel = .chat }
            }
            #endif
        }
    }

    private func syncChat() {
        assistant.context(identity: model.identity, scope: model.chatScope, title: model.currentSession?.displayName ?? "终端", device: model.deviceName, connected: model.connected, core: model.core, terminal: model)
    }
    private func panelHeight(_ panel: WorkspacePanel, available: CGFloat) -> CGFloat {
        let fraction: CGFloat = panel == .settings ? 0.62 : 0.74
        return max(80, min(available * fraction, available - 56))
    }

    private var terminalWorkspace: some View {
        VStack(spacing: 0) {
                HStack(spacing: 8) {
                    ToolButton(symbol: "sidebar.left", label: "工作空间") { drawer = true }.accessibilityIdentifier("workspace.drawer")
                    VStack(alignment: .leading, spacing: 2) {
                        Text(model.currentSession?.displayName ?? "AI Terminal").font(.system(size: 16, weight: .semibold)).lineLimit(1).accessibilityAddTraits(.isHeader)
                        Text(model.deviceID.isEmpty ? "工作空间" : model.deviceName).font(.caption2).foregroundColor(WorkspaceStyle.muted).lineLimit(1)
                    }
                    Spacer()
                    if model.busy { ProgressView() }
                    Circle().fill(model.connected ? WorkspaceStyle.success : WorkspaceStyle.muted).frame(width: 6, height: 6).accessibilityLabel(model.connected ? "终端已连接" : "终端未连接")
                    ToolButton(symbol: "desktopcomputer", label: "选择设备") { panel = .devices }
                }.padding(.horizontal, 8).background(WorkspaceStyle.background)
            Divider().overlay(WorkspaceStyle.line)
            if let error = model.error {
                HStack(alignment: .top) {
                    Text(error).font(.caption).foregroundColor(WorkspaceStyle.danger).fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 4)
                    Button { model.error = nil } label: { Image(systemName: "xmark").frame(width: 32, height: 32) }.accessibilityLabel("关闭错误")
                }.padding(.horizontal, 16).padding(.vertical, 6).background(WorkspaceStyle.surface)
            }
            ZStack(alignment: .bottomTrailing) {
                if let frame = model.screen {
                    TerminalSurface(frame: frame, zoom: min(24, max(12, fontSize)) / 15, generation: model.generation, core: model.displayCore, onStatus: model.displayStatus, onOpenWorkspace: { drawer = true })
                } else {
                    VStack {
                        Spacer()
                        EmptyWorkspace(symbol: "terminal", title: model.busy ? "正在连接工作空间" : (model.connected ? "选择或新建终端" : "尚未连接 Desktop"))
                        Button(model.connected ? "打开工作空间" : "选择设备") { if model.connected { drawer = true } else { panel = .devices } }.frame(minHeight: 44)
                        Spacer()
                    }.frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle()).gesture(
                        DragGesture(minimumDistance: 20).onEnded { value in
                            if value.startLocation.x < 24, value.translation.width > 60 { drawer = true }
                        }
                    )
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity).overlay(alignment: .bottomTrailing) {
                VStack(spacing: 8) {
                    ToolButton(symbol: "slider.horizontal.3", label: "终端设置") { panel = .settings }.background(WorkspaceStyle.surface.opacity(opacity / 100)).cornerRadius(6).accessibilityIdentifier("workspace.settings")
                    ToolButton(symbol: "bubble.left", label: "AI 对话", active: true) { panel = .chat }.accessibilityIdentifier("workspace.chat")
                }.padding(16).opacity(panel == nil ? 1 : 0)
            }
            if inputVisible { TerminalComposer(draft: $terminalDraft, enabled: model.hasControl && model.connected && !model.busy, send: model.text, key: model.key) }
            VStack(spacing: 0) {
                HStack(spacing: 4) {
                    ToolButton(symbol: inputVisible ? "keyboard.chevron.compact.down" : "keyboard", label: inputVisible ? "隐藏输入" : "显示输入") { inputVisible.toggle() }
                    Toggle("接管输入", isOn: Binding(get: { model.hasControl }, set: model.control)).font(.caption).fixedSize().disabled(model.selected == nil || !model.connected || model.busy)
                    Spacer(minLength: 4)
                    ToolButton(symbol: "clock", label: "终端历史") { model.readHistory(); panel = .terminalHistory }.disabled(model.selected == nil)
                    ToolButton(symbol: "xmark.square", label: "关闭会话") { closing = true }.disabled(model.selected == nil || !model.connected || model.busy)
                }
                HStack(alignment: .top) {
                    Text(model.status).lineLimit(2)
                    Spacer(minLength: 8)
                    Text(model.screen.map { "\($0.cols) 列 · UTF-8" } ?? "UTF-8").font(.system(size: 11, design: .monospaced)).fixedSize()
                }.font(.caption).foregroundColor(WorkspaceStyle.muted).padding(.horizontal, 8).padding(.bottom, 8)

            }.padding(.horizontal, 8).background(WorkspaceStyle.surface)
        }.accessibilityHidden(drawer || panel != nil)
    }

    private var drawerView: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack { Text("工作空间").font(.title3.weight(.semibold)); Spacer(); ToolButton(symbol: "xmark", label: "关闭工作空间") { drawer = false } }.padding(.horizontal, 16).padding(.top, 8)
            Picker("工作空间视图", selection: $historyTab) { Text("终端").tag(false); Text("AI 历史").tag(true) }.pickerStyle(.segmented).padding(16)
            HStack { Image(systemName: "magnifyingglass"); TextField(historyTab ? "搜索对话内容" : "终端名称或路径", text: $search).textInputAutocapitalization(.never).autocorrectionDisabled(); if !search.isEmpty { Button { search = "" } label: { Image(systemName: "xmark.circle.fill") }.accessibilityLabel("清除搜索") } }.padding(12).background(WorkspaceStyle.control).cornerRadius(8).padding(.horizontal, 16)
            if !historyTab {
                HStack {
                    Button { drawer = false; panel = .devices } label: { Label(model.deviceID.isEmpty ? "选择设备" : model.deviceName, systemImage: "desktopcomputer").lineLimit(1) }
                    Spacer()
                    ToolButton(symbol: "arrow.clockwise", label: "刷新会话") { model.refreshSessions() }.disabled(!model.connected || model.busy)
                    ToolButton(symbol: "plus", label: "新建会话") { creating = true }.disabled(!model.connected || model.busy)
                }.font(.subheadline).padding(.horizontal, 16)
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if historyTab {
                        let archives = assistant.archives.filter { search.isEmpty || $0.title.localizedCaseInsensitiveContains(search) || $0.messages.contains { $0.content.localizedCaseInsensitiveContains(search) } }
                        if archives.isEmpty { EmptyWorkspace(symbol: "bubble.left.and.bubble.right", title: search.isEmpty ? "暂无 AI 对话" : "没有匹配的对话") }
                        ForEach(archives) { archive in
                            Button {
                                selectedHistory = archive; drawer = false; panel = .chatHistory
                            } label: {
                                VStack(alignment: .leading, spacing: 8) {
                                    HStack { Image(systemName: "bubble.left"); Text(archive.title).fontWeight(.medium).lineLimit(2); Spacer(minLength: 0) }
                                    Text(archive.messages.last?.content ?? archive.status).lineLimit(2).font(.caption).foregroundColor(WorkspaceStyle.muted)
                                    HStack { Text(model.presence(archive.scope)); Spacer(); Text(archive.updated, style: .date) }.font(.caption2).foregroundColor(model.presence(archive.scope) == "在线" ? WorkspaceStyle.success : WorkspaceStyle.muted)
                                }.padding(16).frame(maxWidth: .infinity, alignment: .leading)
                            }.buttonStyle(.plain)
                            Divider().padding(.horizontal, 16)
                        }
                    } else {
                        let sessions = model.sessions.filter { search.isEmpty || $0.cwd.localizedCaseInsensitiveContains(search) || $0.id.localizedCaseInsensitiveContains(search) }
                        if sessions.isEmpty { EmptyWorkspace(symbol: "terminal", title: search.isEmpty ? "暂无终端会话" : "没有匹配的会话") }
                        ForEach(sessions, id: \.id) { session in
                            Button {
                                model.select(session.id, control: false); drawer = false
                            } label: {
                                HStack(alignment: .top, spacing: 12) {
                                    Image(systemName: "terminal").foregroundColor(session.id == model.selected ? WorkspaceStyle.accent : WorkspaceStyle.muted).padding(.top, 2)
                                    VStack(alignment: .leading, spacing: 6) {
                                        Text(session.displayName).lineLimit(2)
                                        Text(session.cwd).font(.system(size: 12, design: .monospaced)).foregroundColor(WorkspaceStyle.muted).lineLimit(2)
                                        Text(session.exited ? "已关闭 · \(session.exitCode)" : (model.connected ? "在线" : "待确认")).font(.caption).foregroundColor(!session.exited && model.connected ? WorkspaceStyle.success : WorkspaceStyle.muted)
                                    }
                                    Spacer(minLength: 0)
                                    if session.id == model.selected { Image(systemName: "checkmark").foregroundColor(WorkspaceStyle.accent) }
                                }.padding(16).frame(maxWidth: .infinity, alignment: .leading).background(session.id == model.selected ? WorkspaceStyle.control : Color.clear)
                            }.buttonStyle(.plain).disabled(!model.connected || model.busy).accessibilityIdentifier("session.select." + session.id)
                        }
                    }
                }
            }.padding(.top, 8)
            Divider()
            HStack(spacing: 12) {
                Image(systemName: "person.crop.circle").font(.system(size: 28)).foregroundColor(WorkspaceStyle.accent)
                VStack(alignment: .leading, spacing: 4) { Text(model.username.isEmpty ? (model.fixture ? "本地 UI 验证" : "旧版配对") : model.username).lineLimit(1); Text(model.server).font(.caption).foregroundColor(WorkspaceStyle.muted).lineLimit(1) }
                Spacer(minLength: 0)
                ToolButton(symbol: "person.crop.circle.badge.gearshape", label: "账号管理") { drawer = false; panel = .account }.disabled(model.username.isEmpty)
                ToolButton(symbol: "rectangle.portrait.and.arrow.right", label: "退出登录") { logoutConfirm = true }.disabled(model.username.isEmpty || model.busy)
            }.padding(16)
        }.simultaneousGesture(DragGesture(minimumDistance: 30).onEnded { value in if value.translation.width < -70, abs(value.translation.height) < 80 { drawer = false } })
    }

    @ViewBuilder private func panelContent(_ value: WorkspacePanel, height: CGFloat) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Image(systemName: panelSymbol(value)).foregroundColor(WorkspaceStyle.accent)
                VStack(alignment: .leading, spacing: 4) {
                    Text(panelTitle(value)).font(.system(size: 20, weight: .semibold))
                    if value == .chat && height < 400 { Text(model.fixture ? "本地 UI 验证 · 未连接模型" : assistant.status).font(.caption2).foregroundColor(WorkspaceStyle.muted).lineLimit(1).accessibilityIdentifier("chat.status") }
                    else if verticalSizeClass != .compact && height >= 350 { Text(panelSubtitle(value)).font(.caption).foregroundColor(WorkspaceStyle.muted).lineLimit(2) }
                }
                Spacer(minLength: 4)
                ToolButton(symbol: "xmark", label: "关闭\(panelTitle(value))") { panel = nil }
            }.padding(.leading, 16).padding(.trailing, 4).padding(.vertical, height < 350 ? 0 : 6).background(WorkspaceStyle.surface)
            Divider().overlay(WorkspaceStyle.line)
            switch value {
            case .settings: settingsPanel
            case .chat: ChatPanel(model: assistant, core: model.core, fixture: model.fixture, compact: height < 400)
            case .devices: devicesPanel
            case .account: AccountPanel(model: model, logout: { logoutConfirm = true })
            case .terminalHistory:
                if model.historyLoading { ProgressView("正在读取历史").padding() }
                ScrollView([.horizontal, .vertical]) {
                    Text(model.history.isEmpty ? "暂无终端历史" : model.history).font(.system(size: fontSize, design: .monospaced)).textSelection(.enabled).padding(16).frame(maxWidth: .infinity, alignment: .leading)
                }
            case .chatHistory:
                if let archive = selectedHistory {
                    ChatMessages(messages: archive.messages)
                    HStack {
                        ToolButton(symbol: "arrow.left", label: "返回 AI 历史") { panel = nil; historyTab = true; drawer = true }
                        PrimaryButton(title: "继续对话") {
                            continueScope = archive.scope
                            if model.chatScope == archive.scope { continueScope = nil; panel = .chat }
                            else if model.connected && model.deviceID == archive.scope.device { model.select(archive.scope.session, control: false) }
                            else { panel = .devices }
                        }.disabled(model.busy || (model.deviceID == archive.scope.device && model.sessions.first(where: { $0.id == archive.scope.session })?.exited == true))
                    }.padding(16)
                }
            }
        }
    }
    private func panelTitle(_ panel: WorkspacePanel) -> String {
        switch panel { case .settings: return "终端设置"; case .chat: return "AI Agent"; case .devices: return "设备"; case .account: return "账号管理"; case .terminalHistory: return "终端历史"; case .chatHistory: return selectedHistory?.title ?? "对话记录" }
    }
    private func panelSubtitle(_ panel: WorkspacePanel) -> String {
        switch panel { case .settings: return "显示偏好"; case .chat: return model.currentSession?.displayName ?? "未选择终端"; case .devices: return model.server; case .account: return model.username; case .terminalHistory: return model.currentSession?.cwd ?? ""; case .chatHistory: return selectedHistory?.deviceName ?? "" }
    }
    private func panelSymbol(_ panel: WorkspacePanel) -> String {
        switch panel { case .settings: return "slider.horizontal.3"; case .chat: return "sparkles"; case .devices: return "desktopcomputer"; case .account: return "person.crop.circle"; case .terminalHistory, .chatHistory: return "clock" }
    }
    private var settingsPanel: some View {
        VStack(spacing: 0) {
          ScrollView {
            VStack(spacing: 24) {
                VStack(spacing: 12) {
                    HStack { Text("文字大小"); Spacer(); Text("\(Int(fontSize)) px").foregroundColor(WorkspaceStyle.accent).monospacedDigit() }.padding(12).background(WorkspaceStyle.surface)
                    Slider(value: $fontSize, in: 12...24, step: 1).accessibilityLabel("文字大小").accessibilityValue("\(Int(fontSize)) px")
                    HStack { Text("12 px").padding(.horizontal, 4).background(WorkspaceStyle.surface); Spacer(); Text("24 px").padding(.horizontal, 4).background(WorkspaceStyle.surface) }.font(.caption).foregroundColor(WorkspaceStyle.muted)
                }
                VStack(spacing: 12) {
                    HStack { Text("浮窗不透明度"); Spacer(); Text("\(Int(opacity))%").foregroundColor(WorkspaceStyle.accent).monospacedDigit() }.padding(12).background(WorkspaceStyle.surface)
                    Slider(value: $opacity, in: 60...96, step: 1).accessibilityLabel("浮窗不透明度").accessibilityValue("\(Int(opacity))%")
                    HStack { Text("通透").padding(.horizontal, 4).background(WorkspaceStyle.surface); Spacer(); Text("清晰").padding(.horizontal, 4).background(WorkspaceStyle.surface) }.font(.caption).foregroundColor(WorkspaceStyle.muted)
                }
            }.padding(24)
          }
          Divider()
          HStack { Button { fontSize = 16; opacity = 88 } label: { Label("恢复默认", systemImage: "arrow.counterclockwise") }.frame(minHeight: 44); Spacer(); Text("自动保存").font(.caption).foregroundColor(WorkspaceStyle.muted) }.padding(.horizontal, 20).padding(.vertical, 8).background(WorkspaceStyle.surface)
        }
    }
    private var devicesPanel: some View {
        VStack(spacing: 0) {
            HStack { Text(model.busy ? "正在连接" : "已登录设备").font(.subheadline).foregroundColor(WorkspaceStyle.muted); Spacer(); if model.busy { ProgressView() }; ToolButton(symbol: "arrow.clockwise", label: "刷新设备") { model.refreshDevices() }.disabled(model.busy) }.padding(.horizontal, 16)
            ScrollView {
                LazyVStack(spacing: 0) {
                    if model.devices.isEmpty { EmptyWorkspace(symbol: "desktopcomputer", title: "暂无可用设备") }
                    ForEach(model.devices, id: \.id) { device in
                        HStack(spacing: 12) {
                            Button {
                                let target = continueScope?.device == device.id ? continueScope?.session : nil
                                if target == nil { continueScope = nil }
                                model.connectDevice(device.id, sessionID: target); panel = nil
                            } label: {
                                HStack(spacing: 12) {
                                    Image(systemName: device.platform == "desktop" ? "desktopcomputer" : "iphone").font(.title3).foregroundColor(WorkspaceStyle.accent)
                                    VStack(alignment: .leading, spacing: 6) { Text(device.name).lineLimit(2); Text(device.current ? "本机" : (device.online ? "在线" : "离线")).font(.caption).foregroundColor(device.online ? WorkspaceStyle.success : WorkspaceStyle.muted) }
                                    Spacer(minLength: 0)
                                }.frame(maxWidth: .infinity, alignment: .leading).frame(minHeight: 56)
                            }.buttonStyle(.plain).disabled(device.platform != "desktop" || !device.online || model.busy).accessibilityIdentifier("device.connect." + device.id)
                            ToolButton(symbol: "trash", label: "移除设备 \(device.name)") { revokeDevice = device }.disabled(model.busy)
                        }.padding(.horizontal, 20).padding(.vertical, 8)
                        Divider().padding(.leading, 20)
                    }
                }
            }
        }
    }
}

private struct TerminalComposer: View {
    @Binding var draft: String
    let enabled: Bool
    let send: (String) -> Bool
    let key: (String) -> Void
    var body: some View {
        VStack(spacing: 4) {
            HStack {
                TextField(enabled ? "终端输入" : "接管输入后可发送", text: $draft).textInputAutocapitalization(.never).autocorrectionDisabled().font(.system(.body, design: .monospaced)).padding(.leading, 12).disabled(!enabled).accessibilityIdentifier("terminal.draft")
                ToolButton(symbol: "arrow.up", label: "发送文字") { if send(draft) { draft = "" } }.disabled(!enabled || draft.isEmpty).accessibilityIdentifier("terminal.send")
            }.background(WorkspaceStyle.control).cornerRadius(6)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 4) {
                    ForEach([("return", "回车", "enter"), ("stop", "Ctrl-C", "ctrl_c"), ("arrow.right.to.line", "Tab", "tab"), ("escape", "Esc", "escape"), ("arrow.up", "向上", "up"), ("arrow.down", "向下", "down"), ("arrow.left", "向左", "left"), ("arrow.right", "向右", "right")], id: \.2) { symbol, label, value in
                        ToolButton(symbol: symbol, label: label) { key(value) }.accessibilityIdentifier("terminal.key." + value)
                    }
                }
            }.disabled(!enabled)
        }.padding(8).background(WorkspaceStyle.surface)
    }
}

private struct AccountPanel: View {
    @ObservedObject var model: TerminalModel
    let logout: () -> Void
    @State private var current = ""
    @State private var next = ""
    @State private var confirmation = ""
    @State private var error: String?
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text(model.server).font(.caption).foregroundColor(WorkspaceStyle.muted).textSelection(.enabled)
                FieldShell(title: "当前密码", symbol: "lock") { SecureField("当前密码", text: $current).textContentType(.password) }
                FieldShell(title: "新密码", symbol: "key") { SecureField("至少 12 字节", text: $next).textContentType(.newPassword) }
                FieldShell(title: "确认新密码", symbol: "key") { SecureField("再次输入新密码", text: $confirmation).textContentType(.newPassword) }
                if let error = error ?? model.error { Text(error).foregroundColor(WorkspaceStyle.danger).font(.subheadline) }
                PrimaryButton(title: "更新密码", symbol: "checkmark", busy: model.busy) {
                    guard !current.isEmpty, next.utf8.count >= 12 else { error = "请输入当前密码，新密码至少 12 字节"; return }
                    guard next == confirmation else { error = "两次输入的新密码不一致"; return }
                    error = nil; model.changePassword(current: current, next: next); current = ""; next = ""; confirmation = ""
                }.disabled(model.busy)
                Divider()
                Button(role: .destructive, action: logout) { Label("退出登录", systemImage: "rectangle.portrait.and.arrow.right").frame(minHeight: 44) }.disabled(model.busy)
            }.padding(20)
        }
    }
}
