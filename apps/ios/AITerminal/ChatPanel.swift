import SwiftUI

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
    @StateObject private var speech = SpeechDraft()
    @FocusState private var focused: Bool
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    private var fixtureMessages: [ChatMessage] {
        #if DEBUG
        if fixture && ProcessInfo.processInfo.arguments.contains("--long-chat") {
            return [ChatMessage(id: "layout-user", role: "user", content: "本地布局验证：检查长消息换行。"), ChatMessage(id: "layout-assistant", role: "assistant", content: "本地布局样本，未发送模型请求。\n" + String(repeating: "LongUnbrokenTerminalPath", count: 20) + "\n" + String(repeating: "这是一段用于检查窄屏排版与滚动的本地文本。", count: 12))]
        }
        #endif
        return []
    }
    private var dense: Bool { compact || verticalSizeClass == .compact }
    var body: some View {
        VStack(spacing: 0) {
            if !compact {
                HStack(spacing: 10) {
                    if model.requesting && !model.monitoring { ProgressView().scaleEffect(0.8) }
                    Text(fixture ? "本地 UI 验证 · 未连接模型" : model.status)
                        .font(.caption).foregroundColor(model.available ? WorkspaceStyle.accent : WorkspaceStyle.muted)
                        .fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("chat.status")
                    Spacer(minLength: 0)
                    if model.canCancel { cancelTaskButton }
                    ToolButton(symbol: "arrow.clockwise", label: "查询 AI 状态") { model.refresh(core) }.disabled(model.requesting || fixture)
                }.padding(.horizontal, 12).padding(.vertical, 4).background(WorkspaceStyle.control)
            }
            if let error = model.storageError {
                Text(error).font(.caption).foregroundColor(WorkspaceStyle.danger).lineLimit(dense ? 1 : nil)
                    .padding(.horizontal, 16).background(WorkspaceStyle.surface)
            }
            ChatMessages(messages: fixtureMessages.isEmpty ? model.current?.messages ?? [] : fixtureMessages)
            Divider().overlay(WorkspaceStyle.line)
            VStack(alignment: .leading, spacing: dense ? 0 : 8) {
                if !dense {
                    HStack(spacing: 12) {
                        Toggle("允许操作", isOn: $model.allowInput)
                        Toggle("监控当前终端", isOn: $model.monitor)
                    }.font(.caption).disabled(model.optionsFrozen)
                }
                // Keep the editor at the same structural position as keyboard space changes.
                HStack(spacing: 8) {
                    draftEditor.focused($focused)
                        .frame(minHeight: dense ? 44 : 64, maxHeight: dense ? 44 : 88)
                        .padding(dense ? 0 : 4).background(WorkspaceStyle.background)
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(WorkspaceStyle.line)).cornerRadius(8)
                        .accessibilityLabel("发送给 AI").accessibilityIdentifier("chat.draft")
                        .overlay(alignment: .topLeading) {
                            if model.draft.isEmpty { Text(dense ? "发送给 AI" : "你想在终端中做什么？").font(.subheadline).foregroundColor(WorkspaceStyle.muted).padding(10).allowsHitTesting(false) }
                        }
                    if dense {
                        voiceButton
                        if speech.recording || speech.authorizing { ToolButton(symbol: "xmark", label: "取消录音") { speech.cancel() } }
                        if model.canCancel { cancelTaskButton }
                        Menu {
                            Toggle("允许操作", isOn: $model.allowInput).disabled(model.optionsFrozen)
                            Toggle("监控当前终端", isOn: $model.monitor).disabled(model.optionsFrozen)
                            Text("\(model.draft.unicodeScalars.count) / 4000")
                            if !speech.feedback.isEmpty { Text(speech.feedback) }
                        } label: { Image(systemName: "ellipsis").frame(width: 44, height: 44) }.accessibilityLabel("消息选项")
                        sendButton
                    }
                }
                if !dense {
                    HStack(spacing: 6) {
                        voiceButton
                        if speech.recording || speech.authorizing { ToolButton(symbol: "xmark", label: "取消录音") { speech.cancel() } }
                        Text("\(model.draft.unicodeScalars.count) / 4000").font(.caption2)
                            .foregroundColor(model.draft.unicodeScalars.count > 4000 ? WorkspaceStyle.danger : WorkspaceStyle.muted).monospacedDigit()
                        Spacer(minLength: 0)
                        sendButton
                    }
                    if !speech.feedback.isEmpty { Text(speech.feedback).font(.caption).foregroundColor(WorkspaceStyle.muted).fixedSize(horizontal: false, vertical: true) }
                }
            }.padding(.horizontal, dense ? 12 : 16).padding(.vertical, dense ? 4 : 16).background(WorkspaceStyle.surface)
        }.onDisappear { speech.cancel(restoringDraft: false) }
    }
    private var voiceButton: some View {
        ToolButton(symbol: speech.recording ? "stop.fill" : "mic", label: speech.recording ? "结束录音" : "语音输入", active: speech.recording) {
            focused = false
            if speech.recording { speech.finish() } else { speech.start(draft: model.draft) { model.draft = $0 } }
        }.disabled(speech.authorizing)
    }
    private var cancelTaskButton: some View {
        ToolButton(symbol: "stop.circle", label: model.monitoring ? "停止监控" : "取消 AI 请求") { model.cancel(core) }.accessibilityIdentifier("chat.cancel")
    }
    private var sendButton: some View {
        ToolButton(symbol: "arrow.up", label: "发送消息", active: true) { focused = false; model.send(core) }
            .disabled(!model.canSend || speech.recording || speech.authorizing || model.draft.unicodeScalars.count > 4000).accessibilityIdentifier("chat.send")
    }
    @ViewBuilder private var draftEditor: some View {
        if #available(iOS 16, *) { TextEditor(text: $model.draft).scrollContentBackground(.hidden) }
        else { TextEditor(text: $model.draft) }
    }
}
