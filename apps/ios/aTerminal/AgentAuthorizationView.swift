import SwiftUI

struct AgentPendingCard: View {
    @ObservedObject var model: AssistantModel
    let item: AgentPending
    let destination: ChatScope?
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(item.title, systemImage: item.kind == "approval" ? "hand.raised" : "questionmark.bubble").font(.headline)
            Text("目标 Session：" + item.session).font(.caption).textSelection(.enabled)
            if !item.reason.isEmpty { Text(item.reason).font(.callout) }
            if !item.actionable { Text("请求状态：" + item.state).foregroundColor(WorkspaceStyle.muted) }
            if item.kind == "approval" {
                Text(item.tool + " · " + item.cwd).font(.caption).textSelection(.enabled)
                Text(item.arguments).font(.system(.callout, design: .monospaced)).lineLimit(8).textSelection(.enabled)
                Button("查看完整操作详情") { model.openApprovalDetails(item, destination: destination) }.accessibilityIdentifier("authorization.details.open")
                    .disabled(item.requiresDetails && !model.authorization.canApprove(item))
                if item.requiresDetails && !model.authorization.canApprove(item) {
                    Text(model.authorization.detailErrors[item.id] ?? "正在取齐完整操作详情，取齐后可以授权；也可以直接拒绝。")
                        .font(.caption).foregroundColor(WorkspaceStyle.muted).accessibilityIdentifier("authorization.details.status")
                }
                if !item.rule.isEmpty { Text("精确规则范围：" + item.rule).font(.caption).textSelection(.enabled) }
                if item.actionable {
                    VStack(alignment: .leading, spacing: 8) {
                        action("授权一次", decision: "once")
                        action("永久授权", decision: "always").disabled(!item.canAlways)
                        if !item.canAlways { Text(item.alwaysUnavailableReason).font(.caption).foregroundColor(WorkspaceStyle.muted) }
                        action("拒绝", decision: "deny")
                    }.buttonStyle(.bordered)
                }
            } else {
                Text(item.question).textSelection(.enabled)
                if item.actionable {
                    Button("回答") { model.openQuestion(item, destination: destination) }.buttonStyle(.borderedProminent).disabled(!canRespond)
                        .accessibilityIdentifier("authorization.question.open")
                }
            }
        }.frame(maxWidth: .infinity, alignment: .leading).padding(12).background(WorkspaceStyle.surface).cornerRadius(8)
            .accessibilityElement(children: .contain).accessibilityIdentifier("authorization.pending." + item.id)
    }
    private var canRespond: Bool { model.authorization.canAct && model.agentConnectionReason == nil }
    private func action(_ title: String, decision: String) -> some View {
        Button(title) {
            guard model.target == destination else { return }
            model.authorization.perform { await $0.resolve(item, decision: decision) }
        }
            .frame(minHeight: 44).disabled(!canRespond || (decision != "deny" && !model.authorization.canApprove(item))).accessibilityIdentifier("authorization.resolve." + decision)
    }
}

struct AgentQuestionView: View {
    @ObservedObject var model: AssistantModel
    let presentation: AgentInteractionPresentation
    private var item: AgentPending { presentation.item }
    private var answer: String {
        get { model.interactions.draft(presentation) }
        nonmutating set { model.interactions.setDraft(newValue, for: presentation) }
    }
    @Environment(\.dismiss) private var dismiss
    @FocusState private var answering: Bool
    private var current: Bool { model.target?.key == presentation.scopeKey && model.interactions.isCurrent(presentation) && model.authorization.pending.contains(where: { $0.id == item.id && $0.actionable }) }
    private var canSubmit: Bool { current && model.authorization.canAct && model.agentConnectionReason == nil && !answer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    var body: some View {
        NavigationView {
            Form {
                Section(header: Text("需要你的答复")) {
                    Text(item.question).textSelection(.enabled)
                    Text("目标 Session：" + item.session).font(.caption).textSelection(.enabled)
                }
                if !item.options.isEmpty {
                    Section(header: Text("选择或补充")) {
                        ForEach(Array(item.options.enumerated()), id: \.offset) { index, option in
                            Button(option) { answer = option }.accessibilityIdentifier("authorization.option.\(index)")
                                .disabled(!current || !model.authorization.canAct)
                        }
                    }
                }
                Section(header: Text("自由答复")) {
                    TextField("输入或补充答复", text: Binding(get: { answer }, set: { answer = $0 })).focused($answering).submitLabel(.send)
                        .onSubmit { if canSubmit { submit() } }.accessibilityIdentifier("authorization.answer")
                        .disabled(!current || !model.authorization.canAct)
                    Button("清空答复") { answer = "" }.accessibilityIdentifier("authorization.answer.clear.form")
                    Button("发送答复") { submit() }.disabled(!canSubmit).accessibilityIdentifier("authorization.answer.send")
                }
                if !model.authorization.error.isEmpty { Text(model.authorization.error).foregroundColor(WorkspaceStyle.danger) }
            }.navigationTitle("回答 Agent").toolbar {
                ToolbarItem(placement: .navigationBarTrailing) { Button("关闭") { model.interactions.dismiss(); dismiss() }.accessibilityIdentifier("authorization.question.close") }
                ToolbarItemGroup(placement: .keyboard) {
                    Button("发送答复") { submit() }.disabled(!canSubmit).accessibilityIdentifier("authorization.answer.keyboard")
                    Spacer()
                    Button("清空") { answer = "" }.accessibilityIdentifier("authorization.answer.clear")
                    Button("完成") { answering = false }
                }
            }
        }
        .onChange(of: model.target) { target in if target?.key != presentation.scopeKey { model.interactions.dismiss(); dismiss() } }
        .onChange(of: model.authorization.pending.map { $0.id + ":" + $0.state }) { _ in
            if model.authorization.permissions != nil && !current { dismiss() }
        }
    }
    private func submit() {
        guard canSubmit else { return }
        answering = false
        let submittedAnswer = answer
        model.authorization.perform { authorization in
            guard model.target?.key == presentation.scopeKey, model.interactions.isCurrent(presentation) else { return }
            await authorization.resolve(item, answer: submittedAnswer)
            if model.target?.key != presentation.scopeKey || (model.authorization.permissions != nil && !current) { dismiss() }
        }
    }
}

struct AgentAuthorizationView: View {
    @ObservedObject var model: AssistantModel
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        NavigationView {
            Form {
                Section(header: Text("当前 Agent 对话")) {
                    Picker("操作模式", selection: Binding(get: { model.authorization.mode }, set: { mode in model.authorization.perform { await $0.setPermissions(mode: mode) } })) {
                        Text("按需授权").tag("ask")
                        Text("只读").tag("read_only")
                    }.accessibilityIdentifier("authorization.mode")
                    Toggle("完全授权直到手动关闭", isOn: Binding(get: { model.authorization.full }, set: { full in model.authorization.perform { await $0.setPermissions(full: full) } }))
                        .disabled(model.authorization.mode == "read_only")
                        .accessibilityIdentifier("authorization.full")
                    Text("开启后，此对话的后续任务及委托无需逐项审批；关闭后尚未执行的操作重新检查。账号、Session、取消和人工控制限制始终有效。").font(.caption)
                }.disabled(!model.authorization.canAct || model.agentConnectionReason != nil)
                if model.authorization.permissions?.canMutate == false {
                    Text("当前设备只有只读权限，可查看请求和规则。").font(.caption).accessibilityIdentifier("authorization.readonly")
                }
                Section(header: Text("永久精确规则"), footer: Text("规则保存在当前账号的 Desktop，仅匹配相同操作、参数、工作目录和相关版本。撤销后后续操作重新审批。")) {
                    if model.authorization.rules.isEmpty { Text("暂无规则").foregroundColor(WorkspaceStyle.muted) }
                    ForEach(model.authorization.rules) { rule in
                        VStack(alignment: .leading, spacing: 8) {
                            Text(rule.preview).font(.callout).textSelection(.enabled)
                            Button("撤销规则", role: .destructive) { model.authorization.perform { await $0.revoke(rule) } }
                                .disabled(!model.authorization.canAct || model.agentConnectionReason != nil)
                                .accessibilityIdentifier("authorization.revoke." + rule.id)
                        }
                    }
                }
                if !model.authorization.error.isEmpty { Text(model.authorization.error).foregroundColor(WorkspaceStyle.danger) }
                Button("刷新 Desktop 状态") { Task { await model.authorization.refresh(includeRules: true) } }
                    .accessibilityIdentifier("authorization.refresh")
            }.navigationTitle("授权管理").toolbar { Button("关闭") { dismiss() }.accessibilityIdentifier("authorization.close") }
        }.task { await model.authorization.refresh(includeRules: true) }
    }
}
