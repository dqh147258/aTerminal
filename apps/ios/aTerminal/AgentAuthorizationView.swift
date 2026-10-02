import SwiftUI

struct AgentPendingCard: View {
    @ObservedObject var model: AssistantModel
    let item: AgentPending
    @State private var answer = ""
    @FocusState private var answering: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(item.title, systemImage: item.kind == "approval" ? "hand.raised" : "questionmark.bubble").font(.headline)
            Text("目标 Session：" + item.session).font(.caption).textSelection(.enabled)
            if !item.reason.isEmpty { Text(item.reason).font(.callout) }
            if !item.actionable { Text("请求状态：" + item.state).foregroundColor(WorkspaceStyle.muted) }
            if item.kind == "approval" {
                Text(item.tool + " · " + item.cwd).font(.caption).textSelection(.enabled)
                Text(model.authorization.detailText[item.id] ?? item.arguments).font(.system(.callout, design: .monospaced)).textSelection(.enabled)
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
                    ForEach(Array(item.options.enumerated()), id: \.offset) { index, option in
                        Button(option) { answer = option }.buttonStyle(.bordered).disabled(!canRespond)
                            .accessibilityIdentifier("authorization.option.\(index)")
                    }
                    TextField("输入或补充答复", text: $answer).textFieldStyle(.roundedBorder).focused($answering)
                        .accessibilityIdentifier("authorization.answer").disabled(!canRespond)
                    Button("发送答复") {
                        submitAnswer()
                    }.buttonStyle(.borderedProminent).disabled(!canRespond || answer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityIdentifier("authorization.answer.send")
                }
            }
        }.frame(maxWidth: .infinity, alignment: .leading).padding(12).background(WorkspaceStyle.surface).cornerRadius(8)
            .accessibilityElement(children: .contain).accessibilityIdentifier("authorization.pending." + item.id)
            .toolbar {
                ToolbarItemGroup(placement: .keyboard) {
                    if answering {
                        Button("发送答复") { submitAnswer() }
                            .disabled(answer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !model.authorization.canAct)
                            .accessibilityIdentifier("authorization.answer.keyboard")
                        Spacer()
                        Button("收起键盘") { answering = false }
                    }
                }
            }
    }
    private var canRespond: Bool { model.authorization.canAct && model.agentConnectionReason == nil }
    private func submitAnswer() {
        answering = false
        Task { await model.authorization.resolve(item, answer: answer) }
    }
    private func action(_ title: String, decision: String) -> some View {
        Button(title) { Task { await model.authorization.resolve(item, decision: decision) } }
            .frame(minHeight: 44).disabled(!canRespond || (decision != "deny" && !model.authorization.canApprove(item))).accessibilityIdentifier("authorization.resolve." + decision)
    }
}

struct AgentAuthorizationView: View {
    @ObservedObject var model: AssistantModel
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        NavigationView {
            Form {
                Section(header: Text("当前 Agent 对话")) {
                    Picker("操作模式", selection: Binding(get: { model.authorization.mode }, set: { mode in Task { await model.authorization.setPermissions(mode: mode) } })) {
                        Text("按需授权").tag("ask")
                        Text("只读").tag("read_only")
                    }.accessibilityIdentifier("authorization.mode")
                    Toggle("完全授权直到手动关闭", isOn: Binding(get: { model.authorization.full }, set: { full in Task { await model.authorization.setPermissions(full: full) } }))
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
                            Button("撤销规则", role: .destructive) { Task { await model.authorization.revoke(rule) } }
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
