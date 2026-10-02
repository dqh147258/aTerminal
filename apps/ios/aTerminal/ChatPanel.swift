import SwiftUI
import UniformTypeIdentifiers

struct ChatMessages: View {
    let messages: [ChatMessage]
    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 24) {
                    if messages.isEmpty { EmptyWorkspace(symbol: "sparkles", title: "暂无对话") }
                    ForEach(messages) { message in
                        VStack(alignment: .leading, spacing: 10) {
                            HStack {
                                Image(systemName: message.role == "assistant" ? "sparkles" : "person")
                                Text(message.role == "assistant" ? "AI Agent" : "你").fontWeight(.medium)
                                Spacer()
                                Text(message.date, style: .time).font(.caption2).foregroundColor(WorkspaceStyle.muted)
                                Button { UIPasteboard.general.string = message.content } label: { Image(systemName: "doc.on.doc").frame(width: 36, height: 36) }.accessibilityLabel("复制消息")
                            }.font(.caption).foregroundColor(WorkspaceStyle.accent)
                            Text(message.content).font(.system(size: 15)).lineSpacing(5).textSelection(.enabled)
                                .fixedSize(horizontal: false, vertical: true).frame(maxWidth: .infinity, alignment: .leading)
                            if let kind = message.eventKind {
                                Text(kind == "observation" ? "屏幕观察 · revision \(message.revision ?? 0)" : (kind == "input" ? "终端输入事件" : "AI 事件 · \(kind)"))
                                    .font(.caption2).foregroundColor(WorkspaceStyle.muted)
                            }
                        }.padding(14).background(message.role == "user" ? WorkspaceStyle.control : WorkspaceStyle.surface)
                            .overlay(RoundedRectangle(cornerRadius: 8).stroke(WorkspaceStyle.line)).cornerRadius(8).id(message.id)
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }.padding(16)
            }.accessibilityIdentifier("chat.messages")
                .onChange(of: messages.count) { _ in proxy.scrollTo("bottom", anchor: .bottom) }
        }
    }
}

struct ChatPanel: View {
    @ObservedObject var model: AssistantModel
    let core: RemoteTerminal
    var fixture = false
    var compact = false
    @FocusState private var composing: Bool
    @State private var importingImages = false
    @State private var imageDestination: ChatScope?
    @State private var pendingNavigationActive = false
    @State private var pendingJump = 0
    @State private var firstRender = true
    @State private var latestBottomVisible = false
    @State private var historyBottomVisible = false
    var body: some View {
        VStack(spacing: compact ? 4 : 8) {
            if !compact {
            HStack {
                Text(model.agentConnectionReason ?? model.status).font(.caption).foregroundColor(WorkspaceStyle.muted)
                Spacer()
                Button("设置") { model.settingsVisible = true }
                Button("停止") { pendingNavigationActive = true; model.cancel(core) }.disabled(!model.canCancel || model.submitting).accessibilityIdentifier("chat.stop")
            }
            Picker("查看", selection: $model.browsing) { Text("对话").tag(false); Text("历史").tag(true) }.pickerStyle(.segmented).onChange(of: model.browsing) { _ in model.reset() }
            }
            ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(model.items) { item in
                        VStack(alignment: .leading, spacing: 6) {
                            Text(item.kind).font(.caption).foregroundColor(WorkspaceStyle.accent)
                            Text(item.text).textSelection(.enabled)
                            ForEach(item.records, id: \.self) { id in Button("查看证据") { model.record(id) } }
                        }.padding(10).frame(maxWidth: .infinity, alignment: .leading).background(WorkspaceStyle.surface).cornerRadius(8)
                    }
                    if !model.browsing && !model.liveText.isEmpty && model.items.last?.text != model.liveText {
                        Text(model.liveText).textSelection(.enabled).padding(10).frame(maxWidth: .infinity, alignment: .leading).background(WorkspaceStyle.surface).cornerRadius(8)
                    }
                    if !model.browsing {
                        ForEach(model.authorization.pending) { item in
                            AgentPendingCard(model: model, item: item).id(item.id)
                        }
                        Color.clear.frame(height: 1).id("chat.bottom").onAppear { latestBottomVisible = true; model.markGlobalRead() }.onDisappear { latestBottomVisible = false }
                    }
                    if model.browsing && model.hasMore {
                        Button(model.loading ? "加载中…" : "加载更早的 50 条") { model.load() }.disabled(model.loading)
                            .onAppear { historyBottomVisible = true }.onDisappear { historyBottomVisible = false }
                    }
                    if model.browsing { Button("返回最新历史") { model.reset() } }
                }
            }
                .accessibilityIdentifier("chat.timeline")
                .onChange(of: model.items.last?.id) { _ in
                    if !model.browsing && !model.authorization.pending.contains(where: { $0.actionable }) && (firstRender || latestBottomVisible), !model.items.isEmpty { proxy.scrollTo("chat.bottom", anchor: .bottom); firstRender = false }
                }
                .onChange(of: model.liveText) { _ in if !model.browsing && !model.authorization.pending.contains(where: { $0.actionable }) && latestBottomVisible { proxy.scrollTo("chat.bottom", anchor: .bottom) } }
                .onChange(of: pendingJump) { _ in
                    if let id = model.authorization.pending.first(where: { $0.actionable })?.id { firstRender = false; pendingNavigationActive = true; proxy.scrollTo(id, anchor: .top) }
                }
                .onChange(of: model.authorization.pending.first(where: { $0.actionable })?.id) { id in
                    if !model.browsing, let id, firstRender || latestBottomVisible { pendingNavigationActive = true; proxy.scrollTo(id, anchor: .top) }
                }
                .onChange(of: model.authorization.busy) { busy in
                    if busy && model.authorization.pending.contains(where: { $0.actionable }) { pendingNavigationActive = true }
                }
                .onChange(of: model.authorization.pending.contains(where: { $0.actionable })) { waiting in
                    if !waiting && pendingNavigationActive && !model.browsing {
                        pendingNavigationActive = false
                        DispatchQueue.main.async {
                            if !model.authorization.pending.contains(where: { $0.actionable }) {
                                if let id = model.items.last?.id { proxy.scrollTo(id, anchor: .bottom) }
                                else { proxy.scrollTo("chat.bottom", anchor: .bottom) }
                            }
                        }
                    }
                }
                .onChange(of: model.target) { _ in firstRender = true; pendingNavigationActive = false }
            }
            .simultaneousGesture(DragGesture().onEnded { gesture in if model.browsing && historyBottomVisible && gesture.translation.height < 0 { model.load() } })
            if !model.historyCacheWarning.isEmpty {
                Text(model.historyCacheWarning).font(.caption).foregroundColor(WorkspaceStyle.danger).accessibilityIdentifier("chat.cache.warning")
            }
            if !model.attachments.isEmpty {
                ScrollView(.horizontal) { HStack {
                    ForEach(model.attachments) { picture in
                        VStack { if let image = UIImage(data: picture.data) { Image(uiImage: image).resizable().scaledToFit().frame(width: 72, height: 60) }; Button("移除") { model.removeImage(picture.id) }.disabled(model.submitting) }
                    }
                } }
            }
            HStack {
                Button { model.authorizationVisible = true } label: {
                    Label(model.authorization.permissions == nil ? "授权状态待同步" : model.authorization.full ? "当前对话完全授权" : model.authorization.mode == "read_only" ? "只读模式" : "按需授权", systemImage: model.authorization.full ? "lock.open" : "lock.shield")
                }.accessibilityIdentifier("authorization.open")
                Spacer()
                if model.authorization.pending.contains(where: { $0.actionable }) {
                    Button("待处理请求") { pendingJump += 1 }.accessibilityIdentifier("authorization.pending.jump")
                    Button("停止") { pendingNavigationActive = true; model.cancel(core) }.disabled(!model.canCancel).accessibilityIdentifier("authorization.stop")
                }
            }.font(.caption)
            if !model.authorization.error.isEmpty {
                Text(model.authorization.error).font(.caption).foregroundColor(WorkspaceStyle.danger).accessibilityIdentifier("authorization.error")
            }
            HStack {
                if compact {
                    Menu {
                        Button("对话") { model.browsing = false; model.reset() }
                        Button("历史") { model.browsing = true; model.reset() }
                                Button("设置") { model.settingsVisible = true }
                        Button("停止") { pendingNavigationActive = true; model.cancel(core) }.disabled(!model.canCancel || model.submitting).accessibilityIdentifier("chat.stop")
                    } label: { Image(systemName: "ellipsis").font(.system(size: 16)).frame(width: 32, height: 32) }
                    .accessibilityLabel("Agent 操作")
                }
                ToolButton(symbol: "photo", label: "添加图片") { imageDestination = model.target; importingImages = true }.disabled(model.submitting)
                TextField("发送任务或追加消息", text: $model.draft).accessibilityIdentifier("chat.draft").disabled(model.submitting)
                    .focused($composing).submitLabel(.send).onSubmit { if model.canSend { composing = false; model.send(core) } }
                if !model.draft.isEmpty {
                    Button { model.draft = "" } label: { Image(systemName: "xmark.circle.fill").frame(minWidth: 32, minHeight: 44) }
                        .accessibilityLabel("清空任务草稿").accessibilityIdentifier("chat.draft.clear").disabled(model.submitting)
                }
                Button("发送") { composing = false; model.send(core) }.disabled(!model.canSend).accessibilityIdentifier("chat.send")
            }.padding(compact ? 6 : 10).background(WorkspaceStyle.control).cornerRadius(8)
        }.padding(compact ? 6 : 12).accessibilityElement(children: .contain).accessibilityIdentifier(model.global ? "chat.global" : "chat.session")
        .toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                if composing {
                    Button("发送任务") { composing = false; model.send(core) }.disabled(!model.canSend).accessibilityIdentifier("chat.send.keyboard")
                    Spacer()
                    Button("收起键盘") { composing = false }
                }
            }
        }
        .onChange(of: model.items.last?.id) { _ in if latestBottomVisible { model.markGlobalRead() } }
        .fileImporter(isPresented: $importingImages, allowedContentTypes: [.image], allowsMultipleSelection: true) { result in
            switch result { case .success(let urls): if imageDestination == model.target { model.addImages(urls) }; case .failure(let error): model.status = error.localizedDescription }
        }
        .sheet(isPresented: $model.evidenceVisible) {
            NavigationView { ScrollView { if let image = model.image { Image(uiImage: image).resizable().scaledToFit() } else { Text(model.evidence).textSelection(.enabled).padding() } }.navigationTitle("证据原文").toolbar { Button("关闭") { model.evidenceVisible = false } } }
        }
        .sheet(isPresented: $model.authorizationVisible) { AgentAuthorizationView(model: model) }
        .fullScreenCover(isPresented: $model.settingsVisible) { AgentSettingsView(model: model) }
    }
}

struct GlobalConversationList: View {
    @ObservedObject var model: AssistantModel
    let open: () -> Void
    @State private var active = false
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(model.destinationLabel).font(.caption).foregroundColor(WorkspaceStyle.muted)
                Spacer()
                ToolButton(symbol: "plus", label: "新增会话") { model.createGlobal(isCurrent: { active }, open) }.disabled(!model.desktopConnected || model.globalCreating).accessibilityIdentifier("global.create")
            }.padding(.horizontal, 16)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    Text("独立对话，集中掌握").font(.title2.weight(.semibold))
                    Text("跨终端的任务，在这里继续。").foregroundColor(WorkspaceStyle.muted)
                    Text("\(model.globalRows.count) 个会话 · \(model.globalRows.filter(model.globalUnread).count) 个未读").font(.caption).foregroundColor(WorkspaceStyle.muted)
                    if model.globalRows.isEmpty { EmptyWorkspace(symbol: "bubble.left.and.bubble.right", title: "暂无会话") }
                    ForEach(model.globalRows.indices, id: \.self) { index in
                        let row = model.globalRows[index]
                        let id = (row["scope"] as? [String: Any])?["agent"] as? String ?? ""
                        SettingsRow(title: row["title"] as? String ?? "新会话", detail: (row["preview"] as? String ?? "") + "\n" + model.globalState(row) + (model.globalUnread(row) ? " · 未读" : ""), symbol: "bubble.left") { model.openGlobal(row); open() }.accessibilityIdentifier("global.select." + id)
                    }
                }.padding(16)
            }
            if !model.globalError.isEmpty { Text(model.globalError).font(.caption).foregroundColor(WorkspaceStyle.danger).padding(12) }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("global.list").onAppear { active = true }.onDisappear { active = false }
            .task { while !Task.isCancelled { model.refreshGlobals(); try? await Task.sleep(nanoseconds: 1_500_000_000) } }
    }
}
