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
                                Image(systemName: message.role == "assistant" ? "sparkles" : "person").font(.system(size: 16))
                                Text(message.role == "assistant" ? "AI Agent" : "你").fontWeight(.medium)
                                Spacer()
                                Text(message.date, style: .time).font(.caption2).foregroundColor(WorkspaceStyle.muted)
                                Button { UIPasteboard.general.string = message.content } label: { Image(systemName: "doc.on.doc").font(.system(size: 16)).frame(width: 44, height: 44) }.accessibilityLabel("复制消息")
                            }.font(.caption).foregroundColor(WorkspaceStyle.accent).dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                            Text(message.content).font(.body).lineSpacing(5).textSelection(.enabled)
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
    @ScaledMetric(relativeTo: .body) private var editorLineHeight: CGFloat = 24
    @FocusState private var composing: Bool
    @State private var importingImages = false
    @State private var imageDestination: ChatScope?
    @State private var pendingNavigationActive = false
    @State private var pendingJump = 0
    @State private var firstRender = true
    @State private var latestBottomVisible = false
    @State private var historyBottomVisible = false
    @State private var discardingDelivery = false
    @State private var discardCandidate: AgentSendSnapshot?
    var body: some View {
        VStack(spacing: compact || composing ? 4 : 8) {
            if !compact {
            HStack(spacing: 8) {
                Circle().fill(model.agentConnectionReason == nil && model.desktopConnected ? WorkspaceStyle.success : WorkspaceStyle.muted).frame(width: 6, height: 6)
                    .accessibilityElement().accessibilityLabel("Agent 连接状态")
                    .accessibilityValue(model.agentConnectionReason == nil && model.desktopConnected ? "在线" : "离线")
                    .accessibilityIdentifier("chat.connection")
                Text(model.agentConnectionReason ?? model.status).font(.caption).foregroundColor(WorkspaceStyle.muted).lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("chat.connection.status")
                Spacer()
                ToolButton(symbol: "slider.horizontal.3", label: "对话设置") { model.settingsVisible = true }
                Button { pendingNavigationActive = true; model.cancel(core) } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "stop.circle").font(.system(size: 16))
                        Text("停止").font(.caption).fixedSize()
                    }.frame(minWidth: 44, minHeight: 44)
                }
                    .disabled(!model.canCancel || model.submitting).accessibilityIdentifier("chat.stop")
            }.dynamicTypeSize(...DynamicTypeSize.xxxLarge).padding(.horizontal, 4)
            if !composing {
                Picker("查看", selection: $model.browsing) { Text("对话").tag(false); Text("历史").tag(true) }.pickerStyle(.segmented).onChange(of: model.browsing) { _ in model.reset() }
            }
            }
            ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    if model.items.isEmpty && model.liveText.isEmpty && !model.loading {
                        EmptyWorkspace(symbol: "sparkles", title: model.browsing ? "暂无对话记录" : "一起完成下一件事", detail: model.browsing ? "已有的对话会保存在这里。" : "描述你的任务，也可以添加图片。")
                    }
                    ForEach(model.items) { item in
                        AgentMessageCard(item: item, record: model.record)
                    }
                    if !model.browsing && !model.liveText.isEmpty && model.items.last?.text != model.liveText {
                        VStack(alignment: .leading, spacing: 10) {
                            HStack(spacing: 8) {
                                Image(systemName: "sparkles").font(.system(size: 16))
                                Text(model.liveAwaitingHistory ? "AI Agent · 已收到的回复" : "AI Agent · 正在回复")
                                    .font(.caption.weight(.semibold)).accessibilityIdentifier("chat.live.status")
                            }.foregroundColor(WorkspaceStyle.accent).dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                            AgentMessageText(text: model.liveText)
                        }.padding(16).frame(maxWidth: .infinity, alignment: .leading).background(WorkspaceStyle.surface).cornerRadius(16)
                    }
                    if !model.browsing {
                        ForEach(model.authorization.pending) { item in
                            AgentPendingCard(model: model, item: item, destination: model.target).id(item.id)
                        }
                        Color.clear.frame(height: 1).id("chat.bottom").onAppear { latestBottomVisible = true; model.markGlobalRead() }.onDisappear { latestBottomVisible = false }
                    }
                    if model.browsing && model.hasMore {
                        Button(model.loading ? "加载中…" : "加载更早的 50 条") { model.load() }.disabled(model.loading)
                            .onAppear { historyBottomVisible = true }.onDisappear { historyBottomVisible = false }
                    }
                    if model.browsing { Button("返回最新历史") { model.reset() } }
                }.padding(.vertical, composing ? 4 : 10).padding(.horizontal, 2)
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
                .onChange(of: model.target) { _ in firstRender = true; pendingNavigationActive = false; discardingDelivery = false; discardCandidate = nil }
            }
            .simultaneousGesture(DragGesture().onEnded { gesture in if model.browsing && historyBottomVisible && gesture.translation.height < 0 { model.load() } })
            if !model.historyCacheWarning.isEmpty {
                Text(model.historyCacheWarning).font(.caption).foregroundColor(WorkspaceStyle.danger).accessibilityIdentifier("chat.cache.warning")
            }
            if model.pendingDelivery != nil || !model.deliveryError.isEmpty { deliveryStatus }
            if !model.attachments.isEmpty { attachmentStrip }
            HStack {
                Button { model.authorizationVisible = true } label: {
                    HStack(spacing: 6) {
                        Image(systemName: model.authorization.full ? "lock.open" : "lock.shield").font(.system(size: 16))
                        Text(model.authorization.permissions == nil ? "授权状态待同步" : model.authorization.full ? "当前对话完全授权" : model.authorization.mode == "read_only" ? "只读模式" : "按需授权")
                            .fixedSize(horizontal: false, vertical: true)
                    }.frame(minHeight: 44)
                }.accessibilityIdentifier("authorization.open").layoutPriority(1)
                Spacer()
                if model.authorization.pending.contains(where: { $0.actionable }) {
                    Button { pendingJump += 1 } label: { Text("待处理请求").fixedSize().frame(minHeight: 44) }.accessibilityIdentifier("authorization.pending.jump")
                    Button { pendingNavigationActive = true; model.cancel(core) } label: { Text("停止").fixedSize().frame(minWidth: 44, minHeight: 44) }.disabled(!model.canCancel).accessibilityIdentifier("authorization.stop")
                }
            }.font(.caption).dynamicTypeSize(...DynamicTypeSize.xxxLarge)
            if !model.authorization.error.isEmpty {
                Text(model.authorization.error).font(.caption).foregroundColor(WorkspaceStyle.danger)
                    .fixedSize(horizontal: false, vertical: true).dynamicTypeSize(...DynamicTypeSize.xxxLarge).accessibilityIdentifier("authorization.error")
            }
            composerView
        }.padding(compact ? 6 : composing ? 8 : 12).accessibilityElement(children: .contain).accessibilityIdentifier(model.global ? "chat.global" : "chat.session")
        .toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                if composing {
                    Button("发送任务") { model.send(core) }.font(.callout).dynamicTypeSize(...DynamicTypeSize.xxxLarge).disabled(!model.canSend).accessibilityIdentifier("chat.send.keyboard")
                    Spacer()
                    Button("收起键盘") { composing = false }.font(.callout).dynamicTypeSize(...DynamicTypeSize.xxxLarge).accessibilityIdentifier("chat.keyboard.dismiss")
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
        .sheet(item: Binding(get: { model.interactions.presentation }, set: { if $0 == nil { model.interactions.dismiss() } })) { presentation in
            if presentation.kind == .question {
                AgentQuestionView(model: model, presentation: presentation)
            } else {
                NavigationView {
                    ScrollView { Text(model.authorization.detailText[presentation.item.id] ?? presentation.item.arguments).font(.system(.callout, design: .monospaced)).textSelection(.enabled).padding() }
                        .navigationTitle("完整操作详情").toolbar {
                            Button("关闭") { model.interactions.dismiss() }.accessibilityIdentifier("authorization.details.close")
                        }
                }
            }
        }
        .sheet(isPresented: $model.authorizationVisible) { AgentAuthorizationView(model: model) }
        .fullScreenCover(isPresented: $model.settingsVisible) { AgentSettingsView(model: model) }
        .confirmationDialog("停止重试这条消息？", isPresented: $discardingDelivery, titleVisibility: .visible) {
            Button("停止重试", role: .destructive) { if let delivery = discardCandidate { model.discardPendingDelivery(delivery) }; discardCandidate = nil }
            Button("返回", role: .cancel) {}
        } message: { Text("这条消息可能已经送达。停止重试会移除待确认记录，当前草稿会保留。") }
    }

    private var composerView: some View {
        VStack(spacing: 4) {
            ZStack(alignment: .topLeading) {
                TextEditor(text: $model.draft).font(.body).lineSpacing(4)
                    .modifier(ChatEditorSurface())
                    .frame(height: max(compact || composing ? 52 : 88, editorLineHeight + 16)).focused($composing)
                    .accessibilityLabel("任务草稿").accessibilityIdentifier("chat.draft")
                if model.draft.isEmpty {
                    Text("发送任务或追加消息…").font(.body).foregroundColor(WorkspaceStyle.muted).padding(.horizontal, 5).padding(.top, 8).allowsHitTesting(false).accessibilityHidden(true)
                }
            }
            HStack(spacing: 4) {
                if compact || composing {
                    Menu {
                        Button("对话") { model.browsing = false; model.reset() }
                        Button("历史") { model.browsing = true; model.reset() }
                        Button("设置") { model.settingsVisible = true }
                        Button("停止") { pendingNavigationActive = true; model.cancel(core) }.disabled(!model.canCancel || model.submitting).accessibilityIdentifier(compact ? "chat.stop" : "chat.stop.menu")
                    } label: { Image(systemName: "ellipsis").font(.system(size: 16)).frame(width: 44, height: 44) }.accessibilityLabel("Agent 操作").accessibilityIdentifier("chat.actions")
                }
                ToolButton(symbol: "photo.on.rectangle", label: "添加图片") { imageDestination = model.target; importingImages = true }
                Text(model.isSendingMessage ? "发送中，可继续编辑" : model.agentConnectionReason != nil ? "离线草稿已保留" : model.modelRetryStatus ?? "支持多行输入")
                    .font(.caption2).foregroundColor(WorkspaceStyle.muted).fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("chat.composer.hint")
                Spacer(minLength: 4)
                Button { model.draft = "" } label: { Image(systemName: "xmark.circle").font(.system(size: 18)).frame(width: 44, height: 44) }
                    .disabled(model.draft.isEmpty).accessibilityLabel("清空任务草稿").accessibilityIdentifier("chat.draft.clear")
                Button { model.send(core) } label: {
                    Image(systemName: "arrow.up").font(.system(size: 18, weight: .semibold)).frame(width: 42, height: 42)
                        .foregroundColor(model.canSend ? WorkspaceStyle.background : WorkspaceStyle.muted)
                        .background(model.canSend ? WorkspaceStyle.accent : WorkspaceStyle.line).clipShape(Circle())
                        .frame(width: 44, height: 44).contentShape(Rectangle())
                }.buttonStyle(.plain).disabled(!model.canSend).accessibilityLabel("发送").accessibilityIdentifier("chat.send")
            }.dynamicTypeSize(...DynamicTypeSize.xxxLarge)
        }.padding(.horizontal, 12).padding(.vertical, compact || composing ? 6 : 8).background(WorkspaceStyle.control)
            .overlay(RoundedRectangle(cornerRadius: 18).stroke(WorkspaceStyle.line)).cornerRadius(18)
    }

    private var attachmentStrip: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 10) {
                ForEach(model.attachments) { picture in
                    ZStack(alignment: .topTrailing) {
                        if let image = UIImage(data: picture.data) {
                            Image(uiImage: image).resizable().scaledToFill().frame(width: 80, height: 64).clipped().cornerRadius(10)
                        }
                        Button { model.removeImage(picture.id) } label: {
                            Image(systemName: "xmark.circle.fill").symbolRenderingMode(.palette).foregroundStyle(.white, WorkspaceStyle.background).frame(width: 44, height: 44)
                        }.accessibilityLabel("移除图片")
                    }
                }
            }.padding(.vertical, 4)
        }
    }

    private var deliveryStatus: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                if model.isSendingMessage { ProgressView().scaleEffect(0.8) }
                Text(model.isSendingMessage ? "正在发送 · 可继续编辑草稿" : model.pendingDelivery == nil ? model.deliveryError : "有一条消息待确认")
                    .font(.caption.weight(.medium)).accessibilityIdentifier("chat.delivery.status")
                Spacer(minLength: 4)
                if let delivery = model.pendingDelivery, !model.submitting {
                    Button("重试发送") { model.retrySend(delivery) }.disabled(!model.canRetrySend).accessibilityIdentifier("chat.delivery.retry")
                    Menu {
                        Button("复制原消息") { UIPasteboard.general.string = delivery.composer.text }
                        Button("停止重试", role: .destructive) { discardCandidate = delivery; discardingDelivery = true }
                    } label: { Image(systemName: "ellipsis").frame(width: 44, height: 44) }.accessibilityLabel("待确认消息操作")
                }
            }
            if let delivery = model.pendingDelivery {
                Text(delivery.composer.text.isEmpty ? "图片消息（\(delivery.composer.images.count) 张）" : delivery.composer.text)
                    .font(.caption).lineLimit(2).foregroundColor(WorkspaceStyle.muted).accessibilityIdentifier("chat.delivery.message")
                if !model.deliveryError.isEmpty && !model.submitting { Text(model.deliveryError).font(.caption2).foregroundColor(WorkspaceStyle.muted) }
            }
        }.font(.caption).dynamicTypeSize(...DynamicTypeSize.xxxLarge).padding(10).background(WorkspaceStyle.control).cornerRadius(12)
    }
}

// The OS branch is stable for the lifetime of the editor. Neither path replaces
// its UITextView or touches text, selection, or first-responder state.
private struct ChatEditorSurface: ViewModifier {
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 16.0, *) {
            content.scrollContentBackground(.hidden)
        } else {
            content.background(ChatEditorBackground())
        }
    }
}

private struct ChatEditorBackground: UIViewRepresentable {
    func makeUIView(context: Context) -> Probe { Probe() }
    func updateUIView(_ view: Probe, context: Context) { view.scheduleUpdate() }

    final class Probe: UIView {
        private weak var editor: UITextView?
        private var scheduled = false

        init() {
            super.init(frame: .zero)
            isUserInteractionEnabled = false
            isAccessibilityElement = false
            backgroundColor = .clear
        }
        required init?(coder: NSCoder) { return nil }
        override func didMoveToSuperview() { super.didMoveToSuperview(); scheduleUpdate() }
        override func didMoveToWindow() { super.didMoveToWindow(); scheduleUpdate() }
        override func layoutSubviews() { super.layoutSubviews(); scheduleUpdate() }

        func scheduleUpdate() {
            guard !scheduled else { return }
            scheduled = true
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.scheduled = false
                guard self.window != nil else { return }
                if self.editor == nil {
                    // Limit the lookup to the editor's nearby containers and its
                    // identifier; other text editors never receive this styling.
                    var container = self.superview
                    for _ in 0..<6 {
                        guard let candidate = container else { break }
                        if let editor = self.findEditor(in: candidate) { self.editor = editor; break }
                        container = candidate.superview
                    }
                }
                if let editor = self.editor {
                    if editor.backgroundColor != .clear { editor.backgroundColor = .clear }
                    if editor.isOpaque { editor.isOpaque = false }
                }
            }
        }

        private func findEditor(in view: UIView) -> UITextView? {
            if let editor = view as? UITextView, editor.accessibilityIdentifier == "chat.draft" { return editor }
            for child in view.subviews {
                if let editor = findEditor(in: child) { return editor }
            }
            return nil
        }
    }
}

private struct AgentMessageCard: View {
    let item: AgentItem
    let record: (String) -> Void
    private var isUser: Bool { item.kind == "你" }
    var body: some View {
        HStack(spacing: 0) {
            if isUser { Spacer(minLength: 28) }
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 8) {
                    Image(systemName: item.interrupted ? "exclamationmark.circle" : isUser ? "person.fill" : "sparkles").font(.system(size: 16))
                    Text(item.kind).font(.caption.weight(.semibold)).accessibilityIdentifier(item.interrupted ? "chat.interrupted" : "chat.message.role")
                    Spacer()
                    Button { UIPasteboard.general.string = item.text } label: { Image(systemName: "doc.on.doc").font(.system(size: 16)).frame(width: 44, height: 44) }
                        .accessibilityLabel("复制消息").accessibilityIdentifier("chat.message.copy")
                }.font(.caption).foregroundColor(item.interrupted ? WorkspaceStyle.danger : WorkspaceStyle.accent).dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                AgentMessageText(text: item.text)
                if item.interrupted { Text("已收到的内容已保留，可继续对话。").font(.caption2).foregroundColor(WorkspaceStyle.muted) }
                ForEach(item.records, id: \.self) { id in
                    Button(item.interrupted ? "查看回复记录" : "查看证据") { record(id) }.font(.caption)
                }
            }.padding(.horizontal, 14).padding(.vertical, 10).frame(maxWidth: .infinity, alignment: .leading)
                .background(isUser ? WorkspaceStyle.accent.opacity(0.1) : WorkspaceStyle.surface)
                .overlay(RoundedRectangle(cornerRadius: 16).stroke(item.interrupted ? WorkspaceStyle.danger.opacity(0.5) : WorkspaceStyle.line))
                .cornerRadius(16)
            if !isUser { Spacer(minLength: 8) }
        }
    }
}

private struct AgentMessageText: View {
    let text: String
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(text.components(separatedBy: "```").enumerated()), id: \.offset) { index, part in
                if index.isMultiple(of: 2) {
                    Text((try? AttributedString(markdown: part, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(part))
                        .font(.body).lineSpacing(5).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                } else {
                    ScrollView(.horizontal, showsIndicators: true) {
                        Text(part).font(.system(.callout, design: .monospaced)).lineSpacing(4).textSelection(.enabled).padding(12)
                    }.background(WorkspaceStyle.background).cornerRadius(10)
                }
            }
        }.frame(maxWidth: .infinity, alignment: .leading)
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
