import SwiftUI
import Combine

private enum WorkspacePanel: String { case settings, chat, globalList, devices, account, terminalHistory, chatHistory, remoteScreens }

private struct WorkspaceEntry: Identifiable {
    let archive: ChatArchive
    let session: RemoteSession?
    let path: String
    var id: String { archive.scope.key }
}

struct WorkspaceScreen: View {
    @StateObject private var model = TerminalModel()
    @StateObject private var assistant = AssistantModel()
    @StateObject private var remoteScreens = RemoteScreenModel()
    @Environment(\.scenePhase) private var phase
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @AppStorage("terminal.fontSize", store: WorkspacePreferences.defaults) private var fontSize = 16.0
    @AppStorage("terminal.overlayOpacity", store: WorkspacePreferences.defaults) private var opacity = 88.0
    @State private var drawer = false
    @State private var search = ""
    @State private var panel: WorkspacePanel?
    @State private var settingsSection: String?
    @State private var accountFromSettings = false
    @State private var inputVisible = false
    @State private var keysVisible = false
    @State private var landscape = false
    @State private var creating = false
    @State private var closing = false
    @State private var selectedHistory: ChatArchive?
    @State private var revokeDevice: AccountDevice?
    @State private var logoutConfirm = false
    @State private var continueScope: ChatScope?
    private var workspace: Bool { !model.username.isEmpty || model.connected || model.screen != nil || model.fixture }

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                WorkspaceStyle.background.ignoresSafeArea()
                if model.preparingWorkspace {
                    VStack(spacing: 16) {
                        ProgressView()
                        Text("正在连接终端…").font(.subheadline).foregroundColor(WorkspaceStyle.muted)
                    }.frame(maxWidth: .infinity, maxHeight: .infinity).accessibilityIdentifier("workspace.preparing")
                } else if workspace { terminalWorkspace } else { LoginScreen(model: model) }
                if keysVisible && !model.preparingWorkspace {
                    Color.clear.contentShape(Rectangle()).onTapGesture { keysVisible = false }
                    TerminalSpecialKeys(enabled: model.canInput,
                        keyboardOpen: inputVisible, dismiss: { keysVisible = false },
                        key: { value in keysVisible = false; model.key(value) },
                        paste: { keysVisible = false; if let text = UIPasteboard.general.string { _ = model.paste(text) } },
                        history: { keysVisible = false; model.readHistory(); panel = .terminalHistory },
                        keyboard: { keysVisible = false; inputVisible.toggle() })
                        .frame(width: min(232, geometry.size.width - 24), height: min(240, max(100, geometry.size.height - 24)))
                        .background(WorkspaceStyle.surface.opacity(opacity / 100))
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(WorkspaceStyle.line)).cornerRadius(8)
                        .padding(12).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .trailing)
                        .accessibilityIdentifier("terminal.specialKeys")
                }
                if drawer && !model.preparingWorkspace {
                    Color.black.opacity(0.4).ignoresSafeArea().onTapGesture { drawer = false }.accessibilityHidden(true)
                    drawerView.frame(width: min(340, geometry.size.width - 28)).frame(maxHeight: .infinity)
                        .background(WorkspaceStyle.surface).transition(.identity).accessibilityAddTraits(.isModal)
                }
                if let panel, !model.preparingWorkspace {
                    let height = panelHeight(panel, available: geometry.size.height)
                    Color.black.opacity(0.08).ignoresSafeArea().onTapGesture { self.panel = nil }.accessibilityHidden(true)
                    panelContent(panel, height: height).frame(maxWidth: .infinity)
                        .frame(height: height)
                        .background(WorkspaceStyle.surface.opacity(min(100, max(0, opacity)) / 100))
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(WorkspaceStyle.line))
                        .cornerRadius(0).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                        .accessibilityAddTraits(.isModal)
                }
            }.frame(width: geometry.size.width, height: geometry.size.height)
                .safeAreaInset(edge: .top, spacing: 0) {
                    if model.reconnecting || model.authenticationRequired { connectionBanner }
                }
                .onAppear { updateOrientation() }
                .onChange(of: geometry.size) { _ in updateOrientation() }
        }
        .statusBarHidden(landscape)
        .foregroundColor(WorkspaceStyle.foreground).tint(WorkspaceStyle.accent)
        .fullScreenCover(isPresented: Binding(get: { settingsSection != nil }, set: { if !$0 { settingsSection = nil } })) {
            if let settingsSection { AgentSettingsView(model: assistant, section: settingsSection) }
        }
        .sheet(isPresented: $creating) {
            CreateTerminalSheet(model: model) { creating = false; drawer = false }
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
        .onChange(of: model.generation) { _ in syncChat() }
        .onChange(of: model.deviceID) { _ in creating = false }
        .onChange(of: model.sessions.map { [$0.id, $0.cwd] }) { _ in syncChat() }
        .onChange(of: model.identity) { _ in
            creating = false
            syncChat()
            if model.username.isEmpty { keysVisible = false; panel = nil; drawer = false; selectedHistory = nil; inputVisible = false; continueScope = nil }

        }
        .onChange(of: model.preparingWorkspace) { value in if value { drawer = false; panel = nil } }
        .onChange(of: model.chatScope) { scope in
            syncChat(); inputVisible = false; keysVisible = false
            if let scope, scope == continueScope { continueScope = nil; panel = .chat }
        }
        .onChange(of: model.connected) { _ in syncChat() }
        .onChange(of: model.remoteScreenConnection) { _ in syncRemoteScreens() }
        .onChange(of: drawer) { value in
            if value { keysVisible = false; model.refreshSessions(); assistant.loadArchives(); updateWorkspaceSearch() }
            else { assistant.cancelHistorySearch() }
        }
        .onChange(of: search) { _ in updateWorkspaceSearch() }
        .onChange(of: workspaceCandidates.map(\.id)) { _ in updateWorkspaceSearch() }
        .onChange(of: assistant.historyCacheRevision) { _ in updateWorkspaceSearch(refresh: true) }
        .onChange(of: panel) { value in if value != .terminalHistory { model.closeHistory() }; assistant.setVisible(value == .chat, core: model.core); if value != nil { inputVisible = false; keysVisible = false }; syncRemoteScreens() }
        .onChange(of: model.hasControl) { value in if !value { inputVisible = false; keysVisible = false } }
        .onReceive(Timer.publish(every: 3, on: .main, in: .common).autoconnect()) { _ in
            if drawer { model.refreshSessions(); assistant.loadArchives() }
        }
        .onReceive(Timer.publish(every: 10, on: .main, in: .common).autoconnect()) { _ in
            if phase == .active { model.heartbeat() }
        }
        .onChange(of: phase) { value in
            if value != .active { remoteScreens.stop() }
            if value == .background { assistant.stop(preservingContext: true); model.suspend(); inputVisible = false; keysVisible = false }
            if value == .active { model.resume() }
            syncRemoteScreens()
        }
        .onDisappear { remoteScreens.stop() }
        .onAppear {
            fontSize = min(24, max(6, fontSize)); opacity = min(100, max(0, opacity))
            syncChat()
            syncRemoteScreens()
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

    private var connectionBanner: some View {
        HStack(spacing: 10) {
            if model.busy { ProgressView().scaleEffect(0.8) }
            Image(systemName: model.authenticationRequired ? "lock.shield" : "wifi.slash")
            Text(model.authenticationRequired ? "连接授权失效，请检查账号与设备" : "连接中断 · 页面和草稿已保留，正在重连")
                .font(.caption).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
            Button(model.authenticationRequired ? "账号与设备" : "重试") {
                if model.authenticationRequired { accountFromSettings = false; panel = .devices }
                else { model.retryConnection() }
            }.font(.caption).disabled(model.busy).accessibilityIdentifier("workspace.reconnect.retry")
        }.padding(.horizontal, 12).padding(.vertical, 8).background(WorkspaceStyle.surface)
            .accessibilityElement(children: .contain).accessibilityIdentifier("workspace.reconnect")
    }

    private func syncChat() {
        assistant.context(identity: model.identity, scope: model.chatScope ?? model.identity.map { ChatScope(identity:$0,device:model.deviceID,session:"") }, title: model.currentSession?.displayName ?? "终端", device: model.deviceName, connected: model.connected, core: model.core, terminal: model)
        for (device, sessions) in model.sessionSnapshots {
            let name = model.devices.first(where: { $0.id == device })?.name ?? assistant.archives.first(where: { $0.scope.device == device })?.deviceName ?? "Desktop"
            assistant.rememberSessions(identity: model.identity, device: device, deviceName: name, sessions: sessions)
        }
        assistant.rememberSessions(identity: model.identity, device: model.deviceID, deviceName: model.deviceName, sessions: model.sessions)
    }
    private func syncRemoteScreens() {
        remoteScreens.configure(source: model.remoteScreenSource, connection: model.remoteScreenConnection,
                                visible: panel == .remoteScreens, active: phase == .active)
    }
    private func updateOrientation() {
        let bounds = UIScreen.main.bounds
        landscape = bounds.width > bounds.height
    }
    private func panelHeight(_ panel: WorkspacePanel, available: CGFloat) -> CGFloat {
        if [.chat, .globalList, .settings, .devices, .account, .remoteScreens].contains(panel) { return available }
        let fraction: CGFloat = panel == .settings ? 0.62 : 0.74
        return max(80, min(available * fraction, available - 56))
    }

    private var terminalWorkspace: some View {
        VStack(spacing: 0) {
            if !landscape {
                HStack(spacing: 8) {
                    ToolButton(symbol: "sidebar.left", label: "工作空间") { drawer = true }.accessibilityIdentifier("workspace.drawer")
                    VStack(alignment: .leading, spacing: 2) {
                        Text(model.currentSession?.displayName ?? "aTerminal").font(.system(size: 16, weight: .semibold)).lineLimit(1).accessibilityAddTraits(.isHeader)
                        Text(model.deviceID.isEmpty ? "工作空间" : model.deviceName).font(.caption2).foregroundColor(WorkspaceStyle.muted).lineLimit(1)
                    }
                    Spacer()
                    if model.busy { ProgressView() }
                    Circle().fill(model.connected ? WorkspaceStyle.success : WorkspaceStyle.muted).frame(width: 6, height: 6).accessibilityLabel(model.connected ? "终端已连接" : "终端未连接")
                    ToolButton(symbol: "desktopcomputer", label: "选择设备") { panel = .devices }
                }.padding(.horizontal, 8).background(WorkspaceStyle.background)
                Divider().overlay(WorkspaceStyle.line)
            }
            if let error = model.error {
                HStack(alignment: .top) {
                    Text(error).font(.caption).foregroundColor(WorkspaceStyle.danger).fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 4)
                    Button { model.error = nil } label: { Image(systemName: "xmark").frame(width: 32, height: 32) }.accessibilityLabel("关闭错误")
                }.padding(.horizontal, 16).padding(.vertical, 6).background(WorkspaceStyle.surface)
            }
            ZStack(alignment: .bottomTrailing) {
                if let frame = model.screen {
                    TerminalSurface(frame: frame, zoom: min(24, max(6, fontSize)) / 15, generation: model.generation, core: model.displayCore,
                        canInput: model.canInput && panel == nil && !drawer, keyboardRequested: inputVisible,
                        onText: model.text, onPaste: model.paste, onKey: model.key, onKeyboardChange: { inputVisible = $0 },
                        onReadOnly: { model.status = model.readOnlyReason },
                        onStatus: model.displayStatus, onOpenWorkspace: { drawer = true })
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
            }.frame(maxWidth: .infinity, maxHeight: .infinity).overlay(alignment: .trailing) {
                GeometryReader { region in
                ScrollView(.vertical, showsIndicators: false) {
                    VStack(spacing: 2) {
                        if landscape {
                            ToolButton(symbol: "sidebar.left", label: "工作空间") { drawer = true }.accessibilityIdentifier("workspace.drawer")
                            ToolButton(symbol: "desktopcomputer", label: "选择设备") { panel = .devices }
                        }
                        ToolButton(symbol: "slider.horizontal.3", label: "终端设置") { panel = .settings }.accessibilityIdentifier("workspace.settings")
                        ToolButton(symbol: "bubble.left", label: "AI 对话") { assistant.openSession(); panel = .chat }.disabled(model.selected == nil && !model.fixture).accessibilityIdentifier("workspace.chat")
                        ToolButton(symbol: "sparkles", label: "全局AI助手") { panel = .globalList }.accessibilityIdentifier("workspace.global")
                        ToolButton(symbol: "display", label: "远程屏幕") { panel = .remoteScreens }.accessibilityIdentifier("workspace.screens")
                        ToolButton(symbol: "keyboard", label: "特殊按键") { keysVisible.toggle() }.accessibilityIdentifier("workspace.keys")
                    }.padding(2).background(WorkspaceStyle.surface.opacity(opacity / 100)).cornerRadius(8)
                }.frame(width: 48, height: min(landscape ? 326 : 234, max(44, region.size.height - 8))).padding(.trailing, 6).opacity(panel == nil && !drawer && !keysVisible ? 1 : 0)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .trailing)
                }
            }
            if !landscape {
                HStack(alignment: .top) {
                    Text(model.status).lineLimit(2)
                    Spacer(minLength: 8)
                    Text(model.screen.map { "\($0.cols) 列 · UTF-8" } ?? "UTF-8").font(.system(size: 11, design: .monospaced)).fixedSize()
                }.font(.caption).foregroundColor(WorkspaceStyle.muted).padding(8).background(WorkspaceStyle.surface)
            }
        }.accessibilityHidden(drawer || panel != nil)
    }

    private func updateWorkspaceSearch(refresh: Bool = false) {
        guard drawer else { return }
        assistant.searchCachedHistory(query: search, scopes: workspaceCandidates.map { $0.archive.scope }, refresh: refresh)
    }
    private var workspaceEntries: [WorkspaceEntry] {
        let query = search.trimmingCharacters(in: .whitespacesAndNewlines)
        return workspaceCandidates.filter { entry in
            query.isEmpty || [entry.archive.title, entry.path, entry.archive.scope.session, entry.archive.deviceName].contains(where: { $0.localizedCaseInsensitiveContains(query) }) || assistant.cachedHistoryMatches(entry.archive.scope, query: query)
        }
    }
    private var workspaceCandidates: [WorkspaceEntry] {
        let identity = model.identity ?? ChatIdentity(server: model.server, account: model.username)
        var entries: [String: WorkspaceEntry] = [:]
        for archive in assistant.archives where archive.scope.identity == identity && !archive.scope.session.isEmpty {
            entries[archive.scope.key] = WorkspaceEntry(archive: archive, session: nil, path: archive.workingDirectory)
        }
        var snapshots = model.sessionSnapshots
        if !model.deviceID.isEmpty { snapshots[model.deviceID] = model.sessions }
        for (device, sessions) in snapshots {
            for session in sessions {
                let scope = ChatScope(identity: identity, device: device, session: session.id)
                let name = model.devices.first(where: { $0.id == device })?.name ?? entries[scope.key]?.archive.deviceName ?? "Desktop"
                let archive = ChatArchive(scope: scope, title: session.displayName, deviceName: name)
                entries[scope.key] = WorkspaceEntry(archive: archive, session: session, path: session.cwd)
            }
        }
        return entries.values.sorted {
            let left = $0.archive.scope.device == model.deviceID && $0.session?.exited == false
            let right = $1.archive.scope.device == model.deviceID && $1.session?.exited == false
            if left != right { return left }
            return $0.archive.title == $1.archive.title ? $0.id < $1.id : $0.archive.title < $1.archive.title
        }
    }
    private func workspaceRow(_ entry: WorkspaceEntry) -> some View {
        let currentDevice = entry.archive.scope.device == model.deviceID
        let desktopOnline = model.connected || model.fixture
        let available = currentDevice && model.connected && entry.session != nil
        let state = !currentDevice || !desktopOnline ? "离线 · 只读" : entry.session == nil ? "已关闭 · 只读" : entry.session?.exited == true ? "已结束 · 只读" : entry.session?.desktopAttached == false ? "Desktop 已离开 · 只读" : "在线"
        return HStack(spacing: 4) {
            Button {
                drawer = false
                if available, let session = entry.session { model.select(session.id, control: !session.exited && session.desktopAttached) }
                else { assistant.openHistory(entry.archive); panel = .chat }
            } label: {
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "terminal").foregroundColor(WorkspaceStyle.accent).padding(.top, 2)
                    VStack(alignment: .leading, spacing: 6) {
                        Text(entry.archive.title).lineLimit(2)
                        Text(entry.path).font(.system(size: 12, design: .monospaced)).foregroundColor(WorkspaceStyle.muted).lineLimit(2)
                        Text(state + " · " + entry.archive.deviceName).font(.caption).foregroundColor(state == "在线" ? WorkspaceStyle.success : WorkspaceStyle.muted)
                    }.frame(maxWidth: .infinity, alignment: .leading)
                    if currentDevice && entry.archive.scope.session == model.selected { Image(systemName: "checkmark").foregroundColor(WorkspaceStyle.accent) }
                }.padding(.vertical, 16).padding(.leading, 16)
            }.buttonStyle(.plain).disabled(model.busy).accessibilityIdentifier("session.select." + entry.archive.scope.session)
            ToolButton(symbol: "bubble.left", label: "AI 历史 · " + entry.archive.title) {
                drawer = false; assistant.openHistory(entry.archive); panel = .chat
            }.accessibilityIdentifier("session.history." + entry.archive.scope.session)
        }.background(currentDevice && entry.archive.scope.session == model.selected ? WorkspaceStyle.control : Color.clear)
            .overlay(alignment: .bottom) { WorkspaceStyle.line.frame(height: 1).padding(.horizontal, 16) }
    }

    private var drawerView: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack { Text("工作空间").font(.title3.weight(.semibold)); Spacer(); ToolButton(symbol: "xmark", label: "关闭工作空间") { drawer = false }.accessibilityIdentifier("workspace.close") }.padding(.horizontal, 16).padding(.top, 8)
            if landscape {
                VStack(alignment: .leading, spacing: 4) {
                    Text(model.currentSession?.displayName ?? "aTerminal").font(.subheadline)
                    Text(model.currentSession?.cwd ?? model.deviceName).font(.caption).lineLimit(2)
                    Text(model.status).font(.caption2).foregroundColor(WorkspaceStyle.muted)
                }.padding(.horizontal, 16)
            }
            HStack {
                ToolButton(symbol: "clock", label: "终端历史") { model.readHistory(); drawer = false; panel = .terminalHistory }.disabled(model.selected == nil)
                ToolButton(symbol: "xmark.square", label: "关闭会话") { closing = true }.disabled(model.selected == nil || !model.connected || model.busy || model.sessionExited || !model.desktopAttached)
                Spacer()
            }.padding(.horizontal, 12)
            HStack { Image(systemName: "magnifyingglass"); TextField("终端、路径或对话正文", text: $search).accessibilityIdentifier("workspace.search").textInputAutocapitalization(.never).autocorrectionDisabled(); if !search.isEmpty { Button { search = "" } label: { Image(systemName: "xmark.circle.fill") }.accessibilityLabel("清除搜索") } }.padding(12).background(WorkspaceStyle.control).cornerRadius(8).padding(.horizontal, 16)
            Group {
                HStack {
                    Button { drawer = false; panel = .devices } label: { Label(model.deviceID.isEmpty ? "选择设备" : model.deviceName, systemImage: "desktopcomputer").lineLimit(1) }
                    Spacer()
                    ToolButton(symbol: "arrow.clockwise", label: "刷新会话") { model.refreshSessions() }.disabled(!model.connected || model.busy)
                    ToolButton(symbol: "plus", label: "新建会话") { creating = true }.disabled(!model.connected || model.busy)
                }.font(.subheadline).padding(.horizontal, 16)
            }
            if assistant.historySearchBusy { ProgressView("正在搜索正文缓存…").font(.caption).padding(8).accessibilityIdentifier("workspace.search.busy") }
            if !assistant.historySearchError.isEmpty { Text(assistant.historySearchError).font(.caption).foregroundColor(WorkspaceStyle.muted).padding(8).accessibilityIdentifier("workspace.search.error") }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if workspaceEntries.isEmpty && !assistant.historySearchBusy { EmptyWorkspace(symbol: "terminal", title: search.isEmpty ? "暂无终端会话" : "没有匹配的会话") }
                    ForEach(workspaceEntries) { entry in workspaceRow(entry) }
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
                    Text(panelTitle(value)).font(.system(size: 20, weight: .semibold)).accessibilityIdentifier(value == .chat && assistant.global ? "global.title" : "panel.title")
                    if value == .chat && height < 400 { Text(model.fixture ? "本地 UI 验证 · 未连接模型" : assistant.status).font(.caption2).foregroundColor(WorkspaceStyle.muted).lineLimit(1).accessibilityIdentifier("chat.status") }
                    else if verticalSizeClass != .compact && height >= 350 { Text(panelSubtitle(value)).font(.caption).foregroundColor(WorkspaceStyle.muted).lineLimit(2) }
                }
                Spacer(minLength: 4)
                ToolButton(symbol: value == .chat && assistant.global ? "arrow.left" : "xmark", label: value == .chat && assistant.global ? "返回全局会话列表" : "关闭\(panelTitle(value))") {
                    if value == .chat && assistant.global { panel = .globalList }
                    else if accountFromSettings && value == .account { panel = .devices }
                    else if accountFromSettings && value == .devices { accountFromSettings = false; panel = .settings }
                    else { panel = nil }
                }.accessibilityIdentifier(value == .remoteScreens ? "screens.close" : value == .settings ? "settings.close" : value == .globalList || value == .chat && assistant.global ? "global.back" : "panel.close")
            }.padding(.leading, 16).padding(.trailing, 4).padding(.vertical, height < 350 ? 0 : 6).background(WorkspaceStyle.surface)
            Divider().overlay(WorkspaceStyle.line)
            switch value {
            case .settings: settingsPanel
            case .globalList: GlobalConversationList(model: assistant) { panel = .chat }
            case .chat: ChatPanel(model: assistant, core: model.core, fixture: model.fixture, compact: height < 400)
            case .devices: devicesPanel
            case .remoteScreens: RemoteScreensPanel(model: remoteScreens) { accountFromSettings = false; panel = .devices; model.refreshDevices() }
            case .account: AccountPanel(model: model, logout: { logoutConfirm = true })
            case .terminalHistory:
                HStack {
                    Text(model.historySummary).font(.caption).foregroundColor(.secondary)
                    Spacer()
                    if model.historyHasMore { Button("加载更早记录") { model.loadEarlierHistory() }.disabled(model.historyLoading) }
                    Button("读取最新历史") { model.readHistory() }.disabled(model.historyLoading)
                }.padding(.horizontal, 16)
                if model.historyLoading { ProgressView("正在读取历史").padding() }
                ScrollView([.horizontal, .vertical]) {
                    Text(model.history.isEmpty ? "暂无终端历史" : model.history).font(.system(size: fontSize, design: .monospaced)).textSelection(.enabled).padding(16).frame(maxWidth: .infinity, alignment: .leading)
                }
            case .chatHistory:
                if let archive = selectedHistory {
                    ChatMessages(messages: archive.messages)
                    HStack {
                        ToolButton(symbol: "arrow.left", label: "返回 AI 历史") { panel = nil; drawer = true }
                        PrimaryButton(title: "继续对话") {
                            continueScope = archive.scope
                            if model.chatScope == archive.scope { continueScope = nil; panel = .chat }
                            else if model.connected && model.deviceID == archive.scope.device { model.select(archive.scope.session, control: true) }
                            else { panel = .devices }
                        }.disabled(model.busy || (model.deviceID == archive.scope.device && model.sessions.first(where: { $0.id == archive.scope.session })?.exited == true))
                    }.padding(16)
                }
            }
        }
    }
    private func panelTitle(_ panel: WorkspacePanel) -> String {
        switch panel { case .remoteScreens: return "远程屏幕"; case .settings: return "设置"; case .globalList: return "全局AI助手"; case .chat: return assistant.global ? assistant.globalTitle : "Session Agent"; case .devices: return accountFromSettings ? "账号与设备" : "设备"; case .account: return "账号管理"; case .terminalHistory: return "终端历史"; case .chatHistory: return selectedHistory?.title ?? "对话记录" }
    }
    private func panelSubtitle(_ panel: WorkspacePanel) -> String {
        switch panel { case .remoteScreens: return model.deviceName; case .settings: return "显示与 Agent 配置"; case .globalList: return model.deviceName; case .chat: return assistant.contextLabel; case .devices: return model.server; case .account: return model.username; case .terminalHistory: return model.currentSession?.cwd ?? ""; case .chatHistory: return selectedHistory?.deviceName ?? "" }
    }
    private func panelSymbol(_ panel: WorkspacePanel) -> String {
        switch panel { case .remoteScreens: return "display"; case .settings: return "slider.horizontal.3"; case .globalList: return "sparkles"; case .chat: return "sparkles"; case .devices: return "desktopcomputer"; case .account: return "person.crop.circle"; case .terminalHistory, .chatHistory: return "clock" }
    }
    private var settingsPanel: some View {
        VStack(spacing: 0) {
          ScrollView {
            VStack(spacing: 24) {
                VStack(spacing: 12) {
                    HStack { Text("文字大小"); Spacer(); Text("\(Int(fontSize)) pt").foregroundColor(WorkspaceStyle.accent).monospacedDigit() }.padding(12).background(WorkspaceStyle.surface)
                    Slider(value: $fontSize, in: 6...24, step: 1).accessibilityLabel("文字大小").accessibilityValue("\(Int(fontSize)) pt")
                    HStack { Text("6 pt").padding(.horizontal, 4).background(WorkspaceStyle.surface); Spacer(); Text("24 pt").padding(.horizontal, 4).background(WorkspaceStyle.surface) }.font(.caption).foregroundColor(WorkspaceStyle.muted)
                }
                VStack(spacing: 12) {
                    HStack { Text("浮窗不透明度"); Spacer(); Text("\(Int(opacity))%").foregroundColor(WorkspaceStyle.accent).monospacedDigit() }.padding(12).background(WorkspaceStyle.surface)
                    Slider(value: $opacity, in: 0...100, step: 1).accessibilityLabel("浮窗不透明度").accessibilityValue("\(Int(opacity))%")
                    HStack { Text("通透").padding(.horizontal, 4).background(WorkspaceStyle.surface); Spacer(); Text("清晰").padding(.horizontal, 4).background(WorkspaceStyle.surface) }.font(.caption).foregroundColor(WorkspaceStyle.muted)
                }
                VStack(alignment: .leading, spacing: 10) {
                    Text("AI 与扩展").font(.subheadline).foregroundColor(WorkspaceStyle.muted)
                    SettingsRow(title: "LLM 大模型", detail: "供应商、模型与默认绑定", symbol: "cpu") { assistant.openSession(); settingsSection = "llm" }.accessibilityIdentifier("settings.llm")
                    SettingsRow(title: "终端读取", detail: "首尾锚点与 TUI 过滤", symbol: "text.alignleft") { assistant.openSession(); settingsSection = "reading" }.accessibilityIdentifier("settings.reading")
                    SettingsRow(title: "MCP", detail: "外部工具服务", symbol: "puzzlepiece.extension") { assistant.openSession(); settingsSection = "mcp" }.accessibilityIdentifier("settings.mcp")
                    SettingsRow(title: "Skills", detail: "技能与完整资源", symbol: "book") { assistant.openSession(); settingsSection = "skills" }.accessibilityIdentifier("settings.skills")
                    Text("账号").font(.subheadline).foregroundColor(WorkspaceStyle.muted).padding(.top, 12)
                    SettingsRow(title: "账号与设备", detail: model.username.isEmpty ? "连接与账号管理" : model.username, symbol: "person.crop.circle") { accountFromSettings = true; panel = .devices; model.refreshDevices() }.accessibilityIdentifier("settings.account")
                }
            }.padding(20)
          }
          Divider()
          HStack { Button { fontSize = 16; opacity = 88 } label: { Label("恢复默认", systemImage: "arrow.counterclockwise") }.accessibilityIdentifier("settings.reset").frame(minHeight: 44); Spacer(); Text("自动保存").font(.caption).foregroundColor(WorkspaceStyle.muted) }.padding(.horizontal, 20).padding(.vertical, 8).background(WorkspaceStyle.surface)
        }.accessibilityElement(children: .contain).accessibilityIdentifier("settings.home")
    }
    private var devicesPanel: some View {
        let onlineDevices = accountFromSettings ? model.devices : model.devices.filter { $0.online && !$0.current }
        return VStack(spacing: 0) {
            if accountFromSettings {
                SettingsRow(title: model.username.isEmpty ? "账号管理" : model.username, detail: model.server, symbol: "person.crop.circle") { panel = .account }.padding(16)
            }
            HStack { Text(model.busy ? "正在连接" : "已登录设备").font(.subheadline).foregroundColor(WorkspaceStyle.muted); Spacer(); if model.busy { ProgressView() }; ToolButton(symbol: "arrow.clockwise", label: "刷新设备") { model.refreshDevices() }.disabled(model.busy) }.padding(.horizontal, 16)
            ScrollView {
                LazyVStack(spacing: 0) {
                    if onlineDevices.isEmpty { EmptyWorkspace(symbol: "desktopcomputer", title: "暂无在线设备") }
                    ForEach(onlineDevices, id: \.id) { device in
                        HStack(spacing: 12) {
                            Button {
                                let target = continueScope?.device == device.id ? continueScope?.session : nil
                                if target == nil { continueScope = nil }
                                model.connectDevice(device.id, sessionID: target); panel = nil
                            } label: {
                                HStack(spacing: 12) {
                                    Image(systemName: device.platform == "desktop" ? "desktopcomputer" : "iphone").font(.title3).foregroundColor(WorkspaceStyle.accent)
                                    VStack(alignment: .leading, spacing: 6) { Text(device.name).lineLimit(2); Text(device.current ? "本机" : device.online ? "在线" : "离线").font(.caption).foregroundColor(WorkspaceStyle.success) }
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

private struct TerminalSpecialKeys: View {
    let enabled: Bool
    let keyboardOpen: Bool
    let dismiss: () -> Void
    let key: (String) -> Void
    let paste: () -> Void
    let history: () -> Void
    let keyboard: () -> Void
    var body: some View {
        ScrollView {
            VStack(spacing: 2) {
                HStack { Text("特殊按键").font(.subheadline); Spacer(); ToolButton(symbol: "xmark", label: "关闭特殊按键", action: dismiss) }
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 2), count: 4), spacing: 2) {
                    ForEach([("return", "回车", "enter"), ("arrow.right.to.line", "Tab", "tab"), ("delete.left", "退格", "backspace"), ("escape", "Esc", "escape"), ("arrow.left", "向左", "left"), ("arrow.up", "向上", "up"), ("arrow.down", "向下", "down"), ("arrow.right", "向右", "right"), ("stop", "Ctrl-C", "ctrl_c")], id: \.2) { symbol, label, value in
                        ToolButton(symbol: symbol, label: label) { key(value) }.disabled(!enabled).accessibilityIdentifier("terminal.key." + value)
                    }
                    ToolButton(symbol: "doc.on.clipboard", label: "粘贴", action: paste).disabled(!enabled)
                    ToolButton(symbol: "clock", label: "终端历史", action: history)
                    ToolButton(symbol: keyboardOpen ? "keyboard.chevron.compact.down" : "keyboard", label: keyboardOpen ? "收起系统键盘" : "显示系统键盘", action: keyboard).disabled(!enabled)
                }
            }.padding(8)
        }
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

/// Uses the same field, row and footer hierarchy as the workspace settings.
private struct CreateTerminalSheet: View {
    @ObservedObject var model: TerminalModel
    let created: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var directory = ""
    @State private var paths: [String] = []
    @State private var loading = true
    @State private var loadError: String?
    @State private var createError: String?
    @State private var uncertainCreation = false
    @State private var submitting = false
    @State private var active = false
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("新建会话").font(.headline)
                Spacer()
                ToolButton(symbol: "xmark", label: "取消") { dismiss() }.disabled(submitting)
            }.padding(.horizontal, 16).padding(.top, 12)
            Divider().background(WorkspaceStyle.line)
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    FieldShell(title: "工作目录", symbol: "folder") {
                        TextField("留空使用 Desktop 默认目录", text: $directory)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                            .accessibilityLabel("Desktop 工作目录")
                    }
                    directoryRow(title: "默认目录", path: "使用 Desktop 的默认工作目录") { directory = ""; createError = nil }
                    Text("最近使用").font(.caption).foregroundColor(WorkspaceStyle.muted)
                    if loading { ProgressView() }
                    else if let loadError { Text(loadError).font(.subheadline).foregroundColor(WorkspaceStyle.muted) }
                    else if paths.isEmpty { Text("暂无最近目录，可输入目录或使用默认目录").font(.subheadline).foregroundColor(WorkspaceStyle.muted) }
                    ForEach(paths, id: \.self) { path in
                        directoryRow(title: (path as NSString).lastPathComponent.isEmpty ? path : (path as NSString).lastPathComponent, path: path) {
                            directory = path; createError = nil
                        }
                    }
                    if let createError { Text(createError).font(.subheadline).foregroundColor(WorkspaceStyle.danger).accessibilityIdentifier("terminal.createError") }
                    if uncertainCreation {
                        Text("创建结果未确认。请取消并检查会话列表，确认没有创建成功后再新建，避免重复创建。")
                            .font(.subheadline).foregroundColor(WorkspaceStyle.danger).accessibilityIdentifier("terminal.createUnconfirmed")
                    }
                }.padding(16).disabled(submitting)
            }
            PrimaryButton(title: "创建", symbol: "plus", busy: submitting) {
                guard !uncertainCreation else { return }
                submitting = true; createError = nil
                model.create(directory) { outcome in
                    guard active else { return }
                    submitting = false
                    uncertainCreation = uncertainCreation || outcome.uncertain
                    if let error = outcome.error { createError = error } else { created() }
                }
            }.disabled(submitting || uncertainCreation || !model.connected).padding(16).accessibilityIdentifier("terminal.create")
        }
        .background(WorkspaceStyle.background).foregroundColor(WorkspaceStyle.foreground).tint(WorkspaceStyle.accent)
        .interactiveDismissDisabled(submitting)
        .onAppear {
            active = true
            model.loadRecentDirectories { values, error in
                guard active else { return }
                paths = values; loadError = error; loading = false
            }
        }
        .onDisappear { active = false }
        .onChange(of: model.generation) { _ in
            loading = false
            if submitting {
                uncertainCreation = true
                createError = "连接已变化，创建结果未确认"
            }
            submitting = false
        }
    }
    private func directoryRow(title: String, path: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: "folder").foregroundColor(WorkspaceStyle.accent)
                VStack(alignment: .leading, spacing: 6) {
                    Text(title).font(.body).foregroundColor(WorkspaceStyle.foreground)
                    Text(path).font(.caption).foregroundColor(WorkspaceStyle.muted).multilineTextAlignment(.leading)
                }.frame(maxWidth: .infinity, alignment: .leading)
                Image(systemName: "chevron.right").foregroundColor(WorkspaceStyle.muted)
            }.padding(12).frame(minHeight: 64).background(WorkspaceStyle.surface)
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(WorkspaceStyle.line)).cornerRadius(8)
        }.buttonStyle(.plain).accessibilityLabel(title + ", " + path)
    }
}
