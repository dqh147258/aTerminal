import SwiftUI
import UniformTypeIdentifiers

// A form owns its draft; the shared snapshot advances only after a confirmed RPC or conflict refresh.
@MainActor final class SettingsStore: ObservableObject {
    let model: AssistantModel
    let destination: ChatScope?
    let session: ConfigurationSession
    init(_ model: AssistantModel) {
        self.model = model; destination = model.target
        #if DEBUG
        if SettingsFixture.enabled {
            let fixture = SettingsFixture.shared
            session = ConfigurationSession(identity: { (model.target?.key ?? "fixture") + ":" + String(model.connectionEpoch) }, transport: { try await fixture.request($0) })
            session.changed = { [weak self] in self?.objectWillChange.send() }
            return
        }
        #endif
        session = ConfigurationSession(identity: { (model.target?.key ?? "") + ":" + String(model.connectionEpoch) }, transport: { command in
            do { return try await model.request(command, configuration: true) }
            catch { throw SettingsFailure.message(terminalError(error)) }
        })
        session.changed = { [weak self] in self?.objectWillChange.send() }
    }
    var snapshot: [String: Any] { session.snapshot }
    var busy: Bool { session.busy }
    var loaded: Bool { session.loaded }
    var error: String { get { session.error } set { session.error = newValue } }
    var config: [String: Any] { snapshot["config"] as? [String: Any] ?? [:] }
    var revision: Any { snapshot["revision"] ?? 0 }
    func items(_ kind: String) -> [String: Any] { config[kind] as? [String: Any] ?? [:] }
    func close() { session.close() }
    func call(_ command: [String: Any]) async throws -> [String: Any] { try await session.call(command) }
    func run(mutation: Bool = false, operation: @escaping () async throws -> [String: Any], done: @escaping ([String: Any]) -> Void = { _ in }) { session.run(mutation: mutation, operation: operation, done: done) }
    func load() { session.load() }
    func save(_ candidate: [String: Any], secrets: [String: String] = [:], done: @escaping () -> Void) { session.save(candidate, secrets: secrets, done: done) }
}

struct SettingsRow: View {
    let title: String
    var detail = ""
    var symbol = "chevron.right"
    var action: (() -> Void)?
    var body: some View {
        Button { action?() } label: {
            HStack(spacing: 12) {
                Image(systemName: symbol).foregroundColor(WorkspaceStyle.accent).frame(width: 24)
                VStack(alignment: .leading, spacing: 6) {
                    Text(title).foregroundColor(WorkspaceStyle.foreground)
                    if !detail.isEmpty { Text(detail).font(.caption).foregroundColor(WorkspaceStyle.muted).fixedSize(horizontal: false, vertical: true) }
                }.frame(maxWidth: .infinity, alignment: .leading)
                if action != nil { Image(systemName: "chevron.right").font(.caption).foregroundColor(WorkspaceStyle.muted) }
            }.padding(14).frame(minHeight: 64).background(WorkspaceStyle.surface)
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(WorkspaceStyle.line)).cornerRadius(8)
        }.buttonStyle(.plain).disabled(action == nil)
    }
}

private enum SettingsPage: Equatable {
    case root, provider(String?), model(String?), catalog, bindings, detail(String), mcp(String?), skill, upload, skillEdit(String)
}

struct AgentSettingsView: View {
    @StateObject private var store: SettingsStore
    @Environment(\.dismiss) private var dismiss
    private let section: String
    @State private var pages: [SettingsPage] = [.root]
    @State private var provider = ProviderDraft()
    @State private var modelDraft = ModelDraft()
    @State private var bindings: [String: String] = [:]
    @State private var head = "10"
    @State private var tail = "20"
    @State private var text = ""
    @State private var skillID = ""
    @State private var path = ""
    @State private var importing = false
    @State private var folder: URL?
    @State private var query = ""
    @State private var catalog: [[String: Any]] = []
    @State private var cursor: String?
    @State private var catalogQuery = ""
    @State private var detailTargetEnabled = false
    @State private var deleteID: String?
    init(model: AssistantModel, section: String = "llm") { _store = StateObject(wrappedValue: SettingsStore(model)); self.section = section }
    private var page: SettingsPage { pages.last ?? .root }
    private var title: String {
        switch page {
        case .root: return ["llm": "LLM 大模型", "reading": "终端读取", "mcp": "MCP", "skills": "Skills"][section] ?? "设置"
        case .provider: return "编辑供应商"
        case .model: return "编辑模型"
        case .catalog: return "模型目录"
        case .bindings: return "作用域绑定"
        case .detail: return section == "mcp" ? "MCP 详情" : "Skill 详情"
        case .mcp(let id): return id == nil ? "导入 MCP JSON" : "编辑 MCP"
        case .skill: return "安装 Codex Skill"
        case .upload: return "导入 Skill 文件夹"
        case .skillEdit: return "编辑用户 SKILL.md"
        }
    }
    private var editing: Bool {
        switch page { case .root: return section == "reading"; case .catalog, .detail: return false; default: return true }
    }
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                ToolButton(symbol: "arrow.left", label: pages.count == 1 ? "返回设置" : "返回") { back() }.disabled(store.busy && page != .catalog && store.loaded).accessibilityIdentifier("settings.back")
                Text(title).font(.title3.weight(.semibold)).frame(maxWidth: .infinity, alignment: .leading)
                if store.busy { ProgressView().padding(.trailing, 16).accessibilityIdentifier("settings.busy") }
                else if page == .root && !editing { ToolButton(symbol: "arrow.clockwise", label: "刷新设置") { store.load() } }
            }.padding(.vertical, 8).background(WorkspaceStyle.surface)
            Divider().overlay(WorkspaceStyle.line)
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(store.model.destinationLabel).font(.caption).foregroundColor(WorkspaceStyle.muted)
                    if store.loaded { content }
                    else if store.busy { ProgressView("正在读取 Desktop 设置…") }
                    else { Button("重新读取设置") { store.load() } }
                }.padding(16).disabled(store.busy)
            }
            if !store.error.isEmpty { Text(store.error).font(.subheadline).foregroundColor(WorkspaceStyle.danger).padding(12).frame(maxWidth: .infinity, alignment: .leading).accessibilityIdentifier("settings.error") }
            if editing && store.loaded {
                HStack(spacing: 12) {
                    Button("取消") { back() }.accessibilityIdentifier("settings.cancel").frame(maxWidth: .infinity, minHeight: 52)
                    PrimaryButton(title: page == .skill ? "安装" : page == .upload ? "上传" : "保存", symbol: "checkmark", busy: store.busy) { save() }.accessibilityIdentifier("settings.save")
                }.padding(16).background(WorkspaceStyle.surface).disabled(store.busy)
            } else { Text("修改在下次任务生效").font(.caption).foregroundColor(WorkspaceStyle.muted).padding(12) }
        }.background(WorkspaceStyle.background).foregroundColor(WorkspaceStyle.foreground).tint(WorkspaceStyle.accent)
            .textInputAutocapitalization(.never).autocorrectionDisabled().interactiveDismissDisabled(store.busy)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("settings.page").accessibilityValue(title)
            .onAppear { store.load() }.onDisappear { provider.key = ""; store.close() }
            .onChange(of: store.loaded) { loaded in if loaded { readDefaults() } }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.folder]) { result in
                switch result { case .success(let url): folder = url; skillID = url.lastPathComponent; push(.upload)
                case .failure(let error): store.error = error.localizedDescription }
            }
            .alert("删除此扩展？", isPresented: Binding(get: { deleteID != nil }, set: { if !$0 { deleteID = nil } })) {
                Button("删除", role: .destructive) { if let id = deleteID { mutateExtension(id, enabled: nil) }; deleteID = nil }.accessibilityIdentifier("extension.delete.confirm")
                Button("取消", role: .cancel) { deleteID = nil }
            } message: { Text("从 Desktop 配置中移除，下次任务将不再使用。") }
    }
    private func push(_ next: SettingsPage) { store.error = ""; pages.append(next) }
    private func back() {
        guard !store.busy || page == .catalog || !store.loaded else { return }
        if store.busy { store.session.cancelRead() }
        provider.key = ""; store.error = ""
        if pages.count == 1 { dismiss() } else { pages.removeLast() }
    }
    private func finish() { provider.key = ""; pages = [.root] }
    private func finishDetail(_ id: String) {
        detailTargetEnabled = !((store.items(section)[id] as? [String: Any])?["enabled"] as? Bool ?? true)
        pages = [.root, .detail(id)]
    }
    private func readDefaults() {
        let value = store.config["terminal_reading"] as? [String: Any] ?? [:]
        head = String(value["head_lines"] as? Int ?? 10); tail = String(value["tail_lines"] as? Int ?? 20)
    }
    @ViewBuilder private var content: some View {
        switch page {
        case .root: root
        case .provider(let id): providerForm(editing: id != nil)
        case .model(let id): modelForm(editing: id != nil)
        case .catalog: catalogForm
        case .bindings: bindingForm
        case .detail(let id): extensionDetail(id)
        case .mcp: codeEditor("MCP JSON")
        case .skill:
            field("Skill ID *", $skillID)
            field("Desktop 上的绝对目录 *", $path)
            hint("安装完整文件夹，包括 SKILL.md 和所有引用的资源。")
        case .upload:
            field("Skill ID *", $skillID)
            hint(folder?.lastPathComponent ?? "未选择文件夹")
            hint("完整上传文件夹：最多 256 个文件、8 层目录、单文件 2 MiB、总计 8 MiB。")
        case .skillEdit: codeEditor("SKILL.md")
        }
    }
    @ViewBuilder private var root: some View {
        if section == "reading" {
            field("首部保留行数（1–100）", $head, number: true)
            field("尾部保留行数（1–100）", $tail, number: true)
            hint("搜索排除已识别的动态 TUI 行，原文仍完整保留。")
        } else if section == "llm" {
            SettingsRow(title: effectiveModel, detail: "当前有效模型", symbol: "cpu")
            heading("供应商")
            ForEach(store.items("providers").keys.sorted(), id: \.self) { id in
                let item = store.items("providers")[id] as? [String: Any] ?? [:]
                SettingsRow(title: item["name"] as? String ?? id, detail: ((item["connection"] as? [String: Any])?["protocol"] as? String ?? "") + (item["enabled"] as? Bool == false ? " · 已停用" : ""), symbol: "network") { provider = ProviderDraft(id: id, item: item); push(.provider(id)) }.accessibilityIdentifier("provider.select." + id)
            }
            Button("添加供应商") { provider = ProviderDraft(); push(.provider(nil)) }.accessibilityIdentifier("provider.add")
            heading("模型")
            ForEach(store.items("models").keys.sorted(), id: \.self) { id in
                let item = store.items("models")[id] as? [String: Any] ?? [:]
                SettingsRow(title: item["model"] as? String ?? id, detail: id + " · " + (item["provider_id"] as? String ?? ""), symbol: "cpu") { modelDraft = ModelDraft(id: id, item: item); push(.model(id)) }.accessibilityIdentifier("model.select." + id)
            }
            Button("添加模型") { modelDraft = ModelDraft(); modelDraft.provider = store.items("providers").keys.sorted().first ?? ""; push(.model(nil)) }.accessibilityIdentifier("model.add").disabled(store.items("providers").isEmpty)
            heading("默认模型")
            SettingsRow(title: "作用域绑定", detail: "Global、Session 默认、当前终端覆盖", symbol: "slider.horizontal.3") {
                bindings = store.items("bindings").mapValues { ($0 as? [String: Any])?["model_id"] as? String ?? "" }; push(.bindings)
            }.accessibilityIdentifier("bindings.open")
        } else {
            heading("内置 · 只读")
            SettingsRow(title: section == "mcp" ? "Terminal" : "终端与 Agent 能力", detail: section == "mcp" ? "builtin/terminal · 内置终端工具" : "截图、会话管理、Agent 管理、等待、历史定位", symbol: "lock")
            heading("已安装")
            if store.items(section).isEmpty { hint("暂无已安装的扩展，添加后可在下次任务使用。") }
            ForEach(store.items(section).keys.sorted(), id: \.self) { id in
                let item = store.items(section)[id] as? [String: Any] ?? [:]
                SettingsRow(title: item["name"] as? String ?? id, detail: id + " · " + (item["enabled"] as? Bool == false ? "已停用" : "已启用"), symbol: section == "mcp" ? "puzzlepiece.extension" : "book") { detailTargetEnabled = !(item["enabled"] as? Bool ?? true); push(.detail(id)) }.accessibilityIdentifier((section == "mcp" ? "mcp.select." : "skill.select.") + id)
            }
            if section == "mcp" { PrimaryButton(title: "导入 MCP JSON", symbol: "plus") { text = "{\"mcpServers\": {}}"; push(.mcp(nil)) }.accessibilityIdentifier("mcp.import") }
            else {
                PrimaryButton(title: "安装 Desktop 上的 Skill", symbol: "plus") { skillID = ""; path = ""; push(.skill) }.accessibilityIdentifier("skill.install")
                Button("从手机文件夹导入 Skill") { importing = true }.accessibilityIdentifier("skill.folder").frame(minHeight: 44)
            }
        }
    }
    private var effectiveModel: String {
        let session = store.destination?.session ?? ""
        let bindings = store.items("bindings")
        let binding = (session.isEmpty || session.hasPrefix("global:") ? bindings["global"] : bindings["session/" + session] ?? bindings["session-default"]) as? [String: Any]
        let id = binding?["model_id"] as? String ?? ""
        return (store.items("models")[id] as? [String: Any])?["model"] as? String ?? "尚未绑定模型"
    }
    private func heading(_ value: String) -> some View { Text(value).font(.subheadline.weight(.semibold)).foregroundColor(WorkspaceStyle.muted).padding(.top, 8) }
    private func hint(_ value: String) -> some View { Text(value).font(.caption).foregroundColor(WorkspaceStyle.muted).fixedSize(horizontal: false, vertical: true) }
    private func field(_ title: String, _ value: Binding<String>, number: Bool = false) -> some View {
        FieldShell(title: title, symbol: number ? "number" : "pencil") { TextField(title, text: value).keyboardType(number ? .numbersAndPunctuation : .default).accessibilityLabel(title).accessibilityIdentifier(fieldID(title)) }
    }
    private func fieldID(_ title: String) -> String {
        ["供应商 ID *": "provider.id", "API 地址 *": "provider.endpoint", "Azure API version *": "provider.apiVersion", "配置 ID *": "model.id", "模型 ID / Azure deployment *": "model.name", "上下文窗口": "model.context", "最大输出 tokens": "model.maxTokens", "温度（可留空）": "model.temperature", "Top P（可留空）": "model.topP", "思考等级（逗号分隔）": "model.levels", "预算下限": "model.budgetMin", "预算上限": "model.budgetMax", "思考等级": "model.reasoningValue", "思考 tokens": "model.reasoningValue", "搜索模型": "catalog.search", "首部保留行数（1–100）": "reading.head", "尾部保留行数（1–100）": "reading.tail", "Skill ID *": "skill.id", "Desktop 上的绝对目录 *": "skill.path"][title] ?? title
    }
    private func codeEditor(_ title: String) -> some View {
        VStack(alignment: .leading) { heading(title); TextEditor(text: $text).font(.system(.body, design: .monospaced)).frame(minHeight: 300).accessibilityLabel(title).accessibilityIdentifier(title == "MCP JSON" ? "mcp.json" : "skill.body") }
    }
    private func providerForm(editing: Bool) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            field("供应商 ID *", $provider.id).disabled(editing)
            Picker("连接协议", selection: $provider.protocolName) { ForEach(SettingsValidation.protocols, id: \.self) { Text($0).tag($0) } }.accessibilityIdentifier("provider.protocol")
            field("API 地址 *", $provider.endpoint)
            if provider.protocolName == "azure_openai" { field("Azure API version *", $provider.apiVersion) }
            FieldShell(title: "API 密钥", symbol: "key") { SecureField("留空保留原凭据", text: $provider.key).accessibilityIdentifier("provider.key") }
            hint("密钥仅写入 Desktop，不会回显。留空保留已有凭据。")
            Toggle("启用供应商", isOn: $provider.enabled)
        }
    }
    private func modelForm(editing: Bool) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            field("配置 ID *", $modelDraft.id).disabled(editing)
            Picker("供应商", selection: Binding(get: { modelDraft.provider }, set: { modelDraft.provider = $0; modelDraft.resetCapabilities() })) { ForEach(store.items("providers").keys.sorted(), id: \.self) { Text($0).tag($0) } }.accessibilityIdentifier("model.provider")
            field("模型 ID / Azure deployment *", Binding(get: { modelDraft.model }, set: { modelDraft.model = $0; modelDraft.resetCapabilities() }))
            SettingsRow(title: "浏览模型目录", detail: "搜索供应商模型与能力声明", symbol: "magnifyingglass") { query = ""; catalog = []; cursor = nil; push(.catalog); discover(nil) }.accessibilityIdentifier("catalog.open")
            DisclosureGroup("模型参数与能力", isExpanded: $modelDraft.advanced) {
                VStack(alignment: .leading, spacing: 16) {
                    field("上下文窗口", $modelDraft.context, number: true); field("最大输出 tokens", $modelDraft.output, number: true)
                    field("温度（可留空）", $modelDraft.temperature, number: true); field("Top P（可留空）", $modelDraft.topP, number: true)
                    Toggle("支持工具", isOn: $modelDraft.tools).accessibilityIdentifier("model.tools"); Toggle("支持图片", isOn: $modelDraft.vision).accessibilityIdentifier("model.vision")
                    field("思考等级（逗号分隔）", $modelDraft.levels)
                    field("预算下限", $modelDraft.budgetMin, number: true); field("预算上限", $modelDraft.budgetMax, number: true)
                    Toggle("支持 adaptive", isOn: $modelDraft.adaptive).accessibilityIdentifier("model.adaptive"); Toggle("支持关闭思考", isOn: $modelDraft.disabled).accessibilityIdentifier("model.disabled")
                }.padding(.top, 12)
            }.accessibilityIdentifier("model.advanced")
            Picker("思考模式", selection: $modelDraft.mode) {
                Text("供应商默认").tag("provider_default"); Text("等级").tag("level"); Text("预算").tag("budget"); Text("自适应").tag("adaptive"); Text("关闭").tag("disabled")
            }.accessibilityIdentifier("model.reasoningMode")
            if modelDraft.mode == "level" || modelDraft.mode == "budget" { field(modelDraft.mode == "level" ? "思考等级" : "思考 tokens", $modelDraft.strength) }
            Picker("默认绑定", selection: $modelDraft.binding) {
                Text("不改默认").tag(""); Text("Global 默认").tag("global"); Text("Session 默认").tag("session-default")
                if let session = currentSession { Text("当前终端覆盖").tag("session/" + session) }
            }
        }
    }
    private var currentSession: String? { let session = store.destination?.session ?? ""; return session.isEmpty || session.hasPrefix("global:") ? nil : session }
    private var scopes: [(String, String)] { [("global", "Global 默认"), ("session-default", "Session 默认")] + (currentSession.map { [("session/" + $0, "当前终端覆盖")] } ?? []) }
    private var bindingForm: some View {
        ForEach(scopes, id: \.0) { scope, title in
            Picker(title, selection: Binding(get: { bindings[scope] ?? "" }, set: { bindings[scope] = $0 })) {
                Text(scope.hasPrefix("session/") ? "继承 Session 默认" : "未绑定").tag("")
                ForEach(store.items("models").keys.sorted(), id: \.self) { Text($0).tag($0) }
            }.padding(12).background(WorkspaceStyle.surface).cornerRadius(8).accessibilityIdentifier(scope == "global" ? "bindings.global" : scope == "session-default" ? "bindings.sessionDefault" : "bindings.current")
        }
    }
    private var catalogForm: some View {
        VStack(alignment: .leading, spacing: 16) {
            field("搜索模型", Binding(get: { query }, set: { query = $0; cursor = nil }))
            Button("搜索") { discover(nil) }.accessibilityIdentifier("catalog.submit")
            if catalog.isEmpty { hint("暂无模型结果") }
            ForEach(catalog.indices, id: \.self) { index in
                let row = catalog[index]
                SettingsRow(title: row["id"] as? String ?? "", detail: "上下文：\((row["context_window"] as? NSNumber)?.stringValue ?? "未知")", symbol: "cpu") { modelDraft.select(row); pages.removeLast() }.accessibilityIdentifier("catalog.select." + (row["id"] as? String ?? ""))
            }
            if let cursor { Button("下一页") { discover(cursor) }.accessibilityIdentifier("catalog.next") }
        }
    }
    private func discover(_ cursor: String?) {
        let provider = modelDraft.provider; let search = query
        var command: [String: Any] = ["action": "discover", "provider": provider, "search": search, "refresh": cursor == nil]
        if let cursor { command["cursor"] = cursor }
        store.run(operation: { try await store.call(command) }) { result in
            guard page == .catalog, modelDraft.provider == provider, query == search else { return }
            catalog = result["models"] as? [[String: Any]] ?? []; self.cursor = result["cursor"] as? String; catalogQuery = search
        }
    }
    @ViewBuilder private func extensionDetail(_ id: String) -> some View {
        if let item = store.items(section)[id] as? [String: Any] {
            SettingsRow(title: item["name"] as? String ?? id, detail: id, symbol: section == "mcp" ? "puzzlepiece.extension" : "book")
            hint(item["description"] as? String ?? item["command"] as? String ?? item["url"] as? String ?? "")
            Button("查看 / 编辑") {
                if section == "mcp" { text = SettingsValidation.encode(item); push(.mcp(id)) }
                else { store.run(operation: { try await store.call(["action": "skill_read", "id": id]) }) { text = $0["body"] as? String ?? ""; push(.skillEdit(id)) } }
            }.frame(minHeight: 44).accessibilityIdentifier("extension.edit")
            Button(detailTargetEnabled ? "启用" : "停用") { mutateExtension(id, enabled: detailTargetEnabled) }.frame(minHeight: 44).accessibilityIdentifier("extension.toggle")
            Button("删除", role: .destructive) { deleteID = id }.frame(minHeight: 44).accessibilityIdentifier("extension.delete")
        } else { hint("此扩展已删除，请返回列表重新检查。") }
    }
    private func mutateExtension(_ id: String, enabled: Bool?) {
        guard var item = store.items(section)[id] as? [String: Any] else { store.error = "此扩展已删除"; return }
        var items = store.items(section)
        if let enabled { item["enabled"] = enabled; items[id] = item } else { items.removeValue(forKey: id) }
        var candidate = store.config; candidate[section] = items; store.save(candidate) { if enabled != nil { finishDetail(id) } else { finish() } }
    }
    private func save() {
        do {
            var candidate = store.config
            switch page {
            case .provider(let id):
                try SettingsValidation.id(provider.id, kind: "providers", editing: id != nil, items: store.items("providers"))
                let value = try provider.value(previous: store.items("providers")[provider.id] as? [String: Any])
                var all = store.items("providers"); all[provider.id] = value; candidate["providers"] = all
                let secrets = provider.secrets(); provider.key = ""
                store.save(candidate, secrets: secrets) { finish() }; return
            case .model(let id): candidate = try modelDraft.candidate(store.config, editing: id != nil)
            case .bindings:
                var all = store.items("bindings")
                for (scope, _) in scopes {
                    let id = bindings[scope] ?? ""
                    if id.isEmpty { all.removeValue(forKey: scope) }
                    else {
                        guard store.items("models")[id] != nil else { throw ChatFailure.message("所选模型已删除") }
                        if (all[scope] as? [String: Any])?["model_id"] as? String != id { var value = all[scope] as? [String: Any] ?? [:]; value["model_id"] = id; value.removeValue(forKey: "reasoning"); all[scope] = value }
                    }
                }; candidate["bindings"] = all
            case .root:
                guard let h = Int(head), let t = Int(tail), (1...100).contains(h), (1...100).contains(t) else { throw ChatFailure.message("首尾行数须在 1–100 之间") }
                var reading = candidate["terminal_reading"] as? [String: Any] ?? [:]; reading["head_lines"] = h; reading["tail_lines"] = t; candidate["terminal_reading"] = reading
            case .mcp(let id):
                let object = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] ?? [:]
                var all = store.items("mcp")
                let servers = id.map { [$0: object as Any] } ?? object["mcpServers"] as? [String: Any] ?? [:]
                guard !servers.isEmpty else { throw ChatFailure.message("需要非空 mcpServers 对象") }
                if let id, all[id] == nil { throw ChatFailure.message("此 MCP 已删除") }
                for (id, value) in servers { guard let value = value as? [String: Any] else { throw ChatFailure.message("MCP 配置须为对象") }; try SettingsValidation.mcp(id, value); all[id] = value }
                candidate["mcp"] = all
            case .skill:
                try SettingsValidation.id(skillID, kind: "skills", editing: false, items: store.items("skills"))
                guard SettingsValidation.absolutePath(path) else { throw ChatFailure.message("请输入 Desktop 上的绝对路径") }
                mutation(["action": "skill_install", "id": skillID, "path": path]); return
            case .skillEdit(let id):
                guard store.items("skills")[id] != nil, text.utf8.count <= 512 * 1024 else { throw ChatFailure.message("Skill 已删除或 SKILL.md 超过 512 KiB") }
                mutation(["action": "skill_edit", "id": id, "path": "SKILL.md", "body": text]); return
            case .upload: try SettingsValidation.id(skillID, kind: "skills", editing: false, items: store.items("skills")); upload(); return
            default: return
            }
            store.save(candidate) {
                if section == "reading" { dismiss() }
                else if case .mcp(let id?) = page { finishDetail(id) }
                else { finish() }
            }
        } catch { store.error = terminalError(error); if case .model = page { modelDraft.advanced = true } }
    }
    private func mutation(_ command: [String: Any]) {
        var command = command; command["expected_revision"] = store.revision
        store.run(mutation: true, operation: { try await store.call(command) }) { _ in
            if command["action"] as? String == "skill_edit", let id = command["id"] as? String { finishDetail(id) } else { finish() }
        }
    }
    private func upload() {
        guard let folder else { return }; let id = skillID; let revision = store.revision
        store.run(mutation: true, operation: {
            let access = folder.startAccessingSecurityScopedResource(); defer { if access { folder.stopAccessingSecurityScopedResource() } }
            // Preflight the full package before creating a remote upload or reading large files.
            let files = try await Task.detached { try SettingsValidation.skillFiles(folder) }.value
            let begin = try await store.call(["action": "skill_upload_begin", "id": id, "expected_revision": revision])
            guard let token = begin["upload_id"] as? String else { throw ChatFailure.message("无效上传响应") }
            var total = 0
            for (relative, url) in files {
                let input = try FileHandle(forReadingFrom: url); defer { try? input.close() }
                var offset = 0
                while true {
                    let data = try input.read(upToCount: 49152) ?? Data()
                    if data.isEmpty && offset > 0 { break }
                    total += data.count
                    guard offset + data.count <= 2097152, total <= 8388608 else { throw SettingsFailure.message("Skill 文件大小超过限制") }
                    _ = try await store.call(["action": "skill_upload_chunk", "upload_id": token, "path": relative, "offset": offset, "data": data.base64EncodedString()])
                    offset += data.count
                    if data.isEmpty { break }
                }
            }
            return try await store.call(["action": "skill_upload_commit", "upload_id": token])
        }) { _ in self.folder = nil; finish() }
    }
}
