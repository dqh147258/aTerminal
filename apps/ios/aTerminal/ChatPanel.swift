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
    @State private var historyBottomVisible = false
    var body: some View {
        VStack(spacing: compact ? 4 : 8) {
            if !compact {
            Picker("Agent 范围", selection: $model.global) { Text("当前终端").tag(false); Text("全局").tag(true) }.pickerStyle(.segmented).onChange(of: model.global) { _ in model.switchScope() }
            HStack {
                Text(model.status).font(.caption).foregroundColor(WorkspaceStyle.muted)
                Spacer()
                Button("旧归档") { model.legacy() }
                Button("设置") { model.settingsVisible = true }
                Button("停止") { model.cancel(core) }.disabled(!model.canCancel)
            }
            Picker("查看", selection: $model.browsing) { Text("对话").tag(false); Text("历史").tag(true) }.pickerStyle(.segmented).onChange(of: model.browsing) { _ in model.reset() }
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(model.items) { item in
                        VStack(alignment: .leading, spacing: 6) {
                            Text(item.kind).font(.caption).foregroundColor(WorkspaceStyle.accent)
                            Text(item.text).textSelection(.enabled)
                            ForEach(item.records, id: \.self) { id in Button("查看证据") { model.record(id) } }
                        }.padding(10).frame(maxWidth: .infinity, alignment: .leading).background(WorkspaceStyle.surface).cornerRadius(8)
                    }
                    if model.browsing && model.hasMore {
                        Button(model.loading ? "加载中…" : "加载更早的 50 条") { model.load() }.disabled(model.loading)
                            .onAppear { historyBottomVisible = true }.onDisappear { historyBottomVisible = false }
                    }
                    if model.browsing { Button("返回最新历史") { model.reset() } }
                }
            }
            .simultaneousGesture(DragGesture().onEnded { gesture in if model.browsing && historyBottomVisible && gesture.translation.height < 0 { model.load() } })
            Toggle("允许操作终端与扩展", isOn: $model.allowInput).font(.caption)
            HStack {
                if compact {
                    Menu {
                        Button("当前终端") { if model.global { model.global = false; model.switchScope() } }
                        Button("全局 Agent") { if !model.global { model.global = true; model.switchScope() } }
                        Button("对话") { model.browsing = false; model.reset() }
                        Button("历史") { model.browsing = true; model.reset() }
                        Button("旧归档") { model.legacy() }
                        Button("设置") { model.settingsVisible = true }
                        Button("停止") { model.cancel(core) }.disabled(!model.canCancel)
                    } label: { Image(systemName: "ellipsis").font(.system(size: 16)).frame(width: 32, height: 32) }
                    .accessibilityLabel("Agent 操作")
                }
                TextField("发送任务或追加消息", text: $model.draft)
                Button("发送") { model.send(core) }.disabled(!model.canSend).accessibilityIdentifier("chat.send")
            }.padding(compact ? 6 : 10).background(WorkspaceStyle.control).cornerRadius(8)
        }.padding(compact ? 6 : 12)
        .sheet(isPresented: $model.evidenceVisible) {
            NavigationView { ScrollView { if let image = model.image { Image(uiImage: image).resizable().scaledToFit() } else { Text(model.evidence).textSelection(.enabled).padding() } }.navigationTitle("证据原文").toolbar { Button("关闭") { model.evidenceVisible = false } } }
        }
        .sheet(isPresented: $model.legacyVisible) {
            NavigationView {
                ScrollView { VStack(alignment:.leading) {
                    if model.legacyText.isEmpty { ForEach(model.legacyRows.indices,id:\.self) { index in Button(model.legacyRows[index]["title"] ?? "旧对话") { model.legacyPage(model.legacyRows[index]["scope"]) } } }
                    if model.legacyText.isEmpty && model.legacyListCursor != nil {Button("下一页旧会话"){model.legacyMore()}}
                    if !model.legacyText.isEmpty { Text(model.legacyText).textSelection(.enabled); if model.legacyBefore != nil { Button("更早的 50 条") {model.legacyPage()} } }
                }.padding() }.navigationTitle("旧手机归档 · 只读").toolbar { Button("关闭") {model.legacyVisible=false} }
            }
        }
        .sheet(isPresented: $model.settingsVisible) { AgentSettingsView(model: model) }
    }
}

struct AgentSettingsView: View {
    @ObservedObject var model: AssistantModel
    @Environment(\.dismiss) private var dismiss
    @State private var snapshot: [String: Any] = [:]
    @State private var destination:ChatScope?
    @State private var error = ""
    @State private var edit: String?
    @State private var providerID = ""
    @State private var protocolName = "openai_responses"
    @State private var endpoint = "https://api.openai.com/v1"
    @State private var apiVersion = ""
    @State private var key = ""
    @State private var headLines = 10
    @State private var tailLines = 20
    @State private var profileID = ""
    @State private var selectedProvider = ""
    @State private var modelID = ""
    @State private var contextWindow = "128000"
    @State private var maxTokens = "4096"
    @State private var temperature = ""
    @State private var topP = ""
    @State private var catalogMetadata:[String:[String:Any]] = [:]
    @State private var tools = false
    @State private var vision = false
    @State private var levels = ""
    @State private var reasoningMode = "provider_default"
    @State private var strength = ""
    @State private var budgetMin = ""
    @State private var budgetMax = ""
    @State private var adaptive = false
    @State private var disabled = false
    @State private var binding = ""
    @State private var catalog: [String] = []
    @State private var catalogCursor: String?
    @State private var search = ""
    @State private var mcpJSON = "{\"mcpServers\":{}}"
    @State private var skillID = ""
    @State private var skillPath = ""
    @State private var skillBody = ""
    @State private var busy = false
    @State private var importing = false
    private var config: [String: Any] { snapshot["config"] as? [String: Any] ?? [:] }
    private func collection(_ kind: String) -> [String: Any] { config[kind] as? [String: Any] ?? [:] }
    var body: some View {
        NavigationView {
            Form {
                Text(model.destinationLabel).font(.caption).foregroundColor(.secondary)
                if !error.isEmpty { Text(error).foregroundColor(.orange) }
                if edit == "provider" { providerForm }
                else if edit == "model" { modelForm }
                else if edit == "skill_edit" {
                    Section("用户 SKILL.md") {TextEditor(text:$skillBody).frame(minHeight:300);Button("保存"){perform(["action":"skill_edit","id":skillID,"path":"SKILL.md","body":skillBody,"expected_revision":snapshot["revision"] ?? 0]){_ in edit=nil;load()}}}
                }
                else if edit == "mcp" {
                    Section("MCP JSON") { TextEditor(text: $mcpJSON).frame(minHeight: 220); Button("保存") { saveMcp() } }
                } else if edit == "skill" {
                    Section("安装 Codex Skill") {
                        TextField("Skill ID", text: $skillID)
                        TextField("Desktop 上的绝对目录", text: $skillPath)
                        Button("安装") { perform(["action": "skill_install", "id": skillID, "path": skillPath, "expected_revision": snapshot["revision"] ?? 0]) { _ in edit = nil; load() } }
                    }
                } else {
                    Section("终端读取锚点") {
                        Stepper("首部保留 \(headLines) 行",value:$headLines,in:1...100)
                        Stepper("尾部保留 \(tailLines) 行",value:$tailLines,in:1...100)
                        Text("搜索排除已识别的动态 TUI 行，原文仍完整保留。").font(.caption).foregroundColor(.secondary)
                        Button("保存读取设置"){var candidate=config;candidate["terminal_reading"]=["head_lines":headLines,"tail_lines":tailLines];save(candidate)}
                    }
                    Section("供应商") {
                        ForEach(collection("providers").keys.sorted(), id: \.self) { id in Button(id) { openProvider(id) } }
                        Button("添加供应商") { openProvider(nil) }
                    }
                    Section("模型与思考强度") {
                        ForEach(collection("models").keys.sorted(), id: \.self) { id in Button(id) { openModel(id) } }
                        Button("添加模型") { openModel(nil) }.disabled(collection("providers").isEmpty)
                    }
                    Section("MCP") {
                        Text("builtin/terminal · 内置，只读").foregroundColor(.secondary)
                        ForEach(collection("mcp").keys.sorted(), id: \.self) { id in extensionRow("mcp", id) }
                        Button("导入 MCP JSON") { edit = "mcp" }
                    }
                    Section("Skills") {
                        Text("终端截图、会话管理、Agent 管理、等待、历史定位 · 内置，只读").foregroundColor(.secondary)
                        ForEach(collection("skills").keys.sorted(), id: \.self) { id in extensionRow("skills", id) }
                        Button("安装 Desktop 上的 Skill") { edit = "skill" }
                        Button("从手机文件夹导入 Skill") { importing = true }
                    }
                    Text("修改在下次任务生效。密钥仅写入 Desktop，不会回显。").font(.caption)
                }
            }.disabled(busy).textInputAutocapitalization(.never).autocorrectionDisabled()
                .navigationTitle("Agent 设置").toolbar {
                    ToolbarItem(placement: .cancellationAction) { Button(edit == nil ? "关闭" : "返回") { if edit == nil { dismiss() } else { edit = nil; key = "" } } }
                    ToolbarItem(placement: .primaryAction) { Button("刷新") { load() } }
                }.task { load() }
                .fileImporter(isPresented:$importing,allowedContentTypes:[.folder],allowsMultipleSelection:false) { result in
                    switch result {case .success(let urls):if let url=urls.first{upload(url)};case .failure(let failure):error=failure.localizedDescription}
                }
        }
    }
    private var providerForm: some View {
        Section("供应商连接") {
            TextField("供应商 ID", text: $providerID)
            Picker("协议", selection: $protocolName) { ForEach(["openai_responses", "openai_chat", "anthropic", "gemini", "azure_openai", "ollama"], id: \.self) { Text($0).tag($0) } }
            TextField("API 地址", text: $endpoint).keyboardType(.URL)
            if protocolName == "azure_openai" { TextField("API version", text: $apiVersion) }
            SecureField("API 密钥（留空保留）", text: $key)
            Button("保存") {
                var providers = collection("providers"); var provider = providers[providerID] as? [String: Any] ?? ["id": providerID, "name": providerID, "credential_revision": 0, "enabled": true]
                var connection: [String: Any] = ["protocol": protocolName, "endpoint": endpoint]
                if !apiVersion.isEmpty { connection["api_version"] = apiVersion }
                provider["connection"] = connection; providers[providerID] = provider
                var candidate = config; candidate["providers"] = providers
                let secrets = key.isEmpty ? [:] : [providerID: key]; key = ""; save(candidate, secrets: secrets)
            }
        }
    }
    private var modelForm: some View {
        Group {
            Section("模型") {
                TextField("配置 ID", text: $profileID)
                Picker("供应商", selection: $selectedProvider) { ForEach(collection("providers").keys.sorted(), id: \.self) { Text($0).tag($0) } }
                    .onChange(of: selectedProvider) { _ in catalog = []; catalogMetadata = [:]; catalogCursor = nil; resetCapabilities() }
                TextField("模型 ID / Azure deployment", text: $modelID).onChange(of: modelID) { _ in if catalogMetadata[modelID]==nil {resetCapabilities()} else {reasoningMode="provider_default";strength=""} }
                TextField("搜索模型目录", text: $search)
                Button("搜索供应商目录") { discover(nil) }
                ForEach(catalog, id: \.self) { id in Button(id) { selectCatalog(id); catalog = [] } }
                if let catalogCursor { Button("下一页模型") { discover(catalogCursor) } }
                TextField("上下文窗口", text: $contextWindow).keyboardType(.numberPad)
                TextField("最大输出 tokens", text: $maxTokens).keyboardType(.numberPad)
                TextField("温度（可留空）",text:$temperature).keyboardType(.decimalPad)
                TextField("Top P（可留空）",text:$topP).keyboardType(.decimalPad)
                Toggle("已确认支持工具", isOn: $tools); Toggle("已确认支持图片", isOn: $vision)
            }
            Section("供应商声明的思考能力") {
                TextField("支持的等级（逗号分隔）", text: $levels)
                TextField("预算下限", text: $budgetMin).keyboardType(.numberPad)
                TextField("预算上限", text: $budgetMax).keyboardType(.numberPad)
                Toggle("支持 adaptive", isOn: $adaptive); Toggle("支持关闭思考", isOn: $disabled)
                Picker("思考模式", selection: $reasoningMode) {
                    Text("供应商默认").tag("provider_default")
                    if !levels.isEmpty { Text("等级").tag("level") }
                    if !budgetMin.isEmpty && !budgetMax.isEmpty { Text("预算").tag("budget") }
                    if adaptive { Text("自适应").tag("adaptive") }; if disabled { Text("关闭").tag("disabled") }
                }
                if reasoningMode == "level" { Picker("思考强度", selection: $strength) { ForEach(levels.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }, id: \.self) { Text($0).tag($0) } } }
                if reasoningMode == "budget" { TextField("思考 tokens", text: $strength).keyboardType(.numberPad) }
            }
            Section("默认绑定") {
                Picker("应用范围", selection: $binding) {
                    Text("不改默认").tag(""); Text("全局默认").tag("global"); Text("终端默认").tag("session-default")
                    if let session = model.target?.session, !session.isEmpty { Text("当前终端").tag("session/" + session) }
                }
                Button("保存模型与强度") { saveModel() }
            }
        }
    }
    private func extensionRow(_ kind: String, _ id: String) -> some View {
        Menu(id) {
            Button("启用") { modify(kind, id, enabled: true) }; Button("停用") { modify(kind, id, enabled: false) }
            Button("删除", role: .destructive) { modify(kind, id, enabled: nil) }
            if kind == "mcp" { Button("编辑 JSON") { mcpJSON = encode(["mcpServers": [id: collection(kind)[id]!]]); edit = "mcp" } }
            else { Button("查看 / 编辑 SKILL.md") { perform(["action": "skill_read", "id": id]) { value in skillID=id;skillBody=value["body"] as? String ?? "";edit="skill_edit" } } }
        }
    }
    private func encode(_ value: Any) -> String { (try? JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted])).map { String(decoding: $0, as: UTF8.self) } ?? "" }
    private func perform(_ command: [String: Any], done: @escaping ([String: Any]) -> Void) {
        let expected=model.target
        guard destination==expected else{error="账号或 Desktop 已变化，请刷新设置";return}
        busy = true; Task {
            guard expected==model.target else{busy=false;return}
            do { let result = try await model.request(command, configuration: true);busy=false;guard expected==model.target else{return};error="";done(result) }
            catch {if expected==model.target{self.error="未保存："+terminalError(error)};busy=false}
        }
    }
    private func load() { destination=model.target;perform(["action": "show"]) { value in snapshot=value;let reading=(value["config"] as? [String:Any])?["terminal_reading"] as? [String:Any] ?? [:];headLines=reading["head_lines"] as? Int ?? 10;tailLines=reading["tail_lines"] as? Int ?? 20 } }
    private func save(_ candidate: [String: Any], secrets: [String: String] = [:]) {
        perform(["action": "replace", "expected_revision": snapshot["revision"] ?? 0, "config": candidate, "secrets": secrets]) { _ in edit = nil; load() }
    }
    private func openProvider(_ id: String?) {
        let item = id.flatMap { collection("providers")[$0] as? [String: Any] } ?? [:]; let connection = item["connection"] as? [String: Any] ?? [:]
        providerID = id ?? ""; protocolName = connection["protocol"] as? String ?? "openai_responses"; endpoint = connection["endpoint"] as? String ?? "https://api.openai.com/v1"; apiVersion = connection["api_version"] as? String ?? ""; key = ""; edit = "provider"
    }
    private func openModel(_ id: String?) {
        let item = id.flatMap { collection("models")[$0] as? [String: Any] } ?? [:]; let caps = item["capabilities"] as? [String: Any] ?? [:]; let reasoning = item["reasoning"] as? [String: Any] ?? [:]
        profileID = id ?? ""; selectedProvider = item["provider_id"] as? String ?? collection("providers").keys.sorted().first ?? ""; modelID = item["model"] as? String ?? ""
        temperature=(item["temperature"] as? NSNumber)?.stringValue ?? "";topP=(item["top_p"] as? NSNumber)?.stringValue ?? ""
        contextWindow = String((item["context_window"] as? Int) ?? 128000); maxTokens = String((item["max_tokens"] as? Int) ?? 4096)
        tools = caps["tools"] as? Bool ?? false; vision = caps["vision"] as? Bool ?? false; levels = (caps["reasoning_levels"] as? [String] ?? []).joined(separator: ",")
        let budget = caps["reasoning_budget"] as? [Int] ?? []; budgetMin = budget.first.map(String.init) ?? ""; budgetMax = budget.last.map(String.init) ?? ""
        adaptive = caps["reasoning_adaptive"] as? Bool ?? false; disabled = caps["reasoning_disabled"] as? Bool ?? false
        reasoningMode = reasoning["mode"] as? String ?? "provider_default"; strength = reasoning["level"] as? String ?? (reasoning["tokens"] as? Int).map(String.init) ?? ""; binding = ""; edit = "model"
    }
    private func discover(_ cursor: String?) {
        var command: [String: Any] = ["action": "discover", "provider": selectedProvider, "search": search, "refresh": cursor == nil]; if let cursor { command["cursor"] = cursor }
        perform(command) { page in let rows=page["models"] as? [[String:Any]] ?? [];catalog=rows.compactMap{$0["id"] as? String};catalogMetadata=Dictionary(uniqueKeysWithValues:rows.compactMap { row in (row["id"] as? String).map{($0,row)} });catalogCursor=page["cursor"] as? String }
    }
    private func resetCapabilities(){reasoningMode="provider_default";strength="";levels="";budgetMin="";budgetMax="";tools=false;vision=false;adaptive=false;disabled=false}
    private func selectCatalog(_ id:String) {
        modelID=id;reasoningMode="provider_default";strength=""
        let row=catalogMetadata[id] ?? [:];let caps=row["capabilities"] as? [String:Any] ?? [:]
        if let window=row["context_window"] as? Int{contextWindow=String(window)}
        if let output=row["max_output_tokens"] as? Int{maxTokens=String(output)}
        tools=caps["tools"] as? Bool ?? false;vision=caps["vision"] as? Bool ?? false
        levels=(caps["reasoning_levels"] as? [String] ?? []).joined(separator:",")
        adaptive=caps["reasoning_adaptive"] as? Bool ?? false;disabled=caps["reasoning_disabled"] as? Bool ?? false
        let budget=caps["reasoning_budget"] as? [Int] ?? [];budgetMin=budget.first.map(String.init) ?? "";budgetMax=budget.last.map(String.init) ?? ""
    }
    private func saveModel() {
        guard let context = Int(contextWindow), let output = Int(maxTokens) else { error = "请输入有效 token 数量"; return }
        var models = collection("models"); var item = models[profileID] as? [String: Any] ?? ["id": profileID, "name": profileID]
        var caps: [String: Any] = ["tools": tools, "vision": vision, "source": "user_declared", "reasoning_levels": levels.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }, "reasoning_adaptive": adaptive, "reasoning_disabled": disabled]
        if let low = Int(budgetMin), let high = Int(budgetMax) { caps["reasoning_budget"] = [low, high] }
        var reasoning: [String: Any] = ["mode": reasoningMode]
        if reasoningMode == "level" { reasoning["level"] = strength }
        if reasoningMode == "budget" { guard let tokens = Int(strength) else { error = "请输入思考预算"; return }; reasoning["tokens"] = tokens }
        item.merge(["provider_id": selectedProvider, "model": modelID, "context_window": context, "max_tokens": output, "read_only": !tools, "capabilities": caps, "reasoning": reasoning]) { _, new in new }
        item["temperature"]=Double(temperature);item["top_p"]=Double(topP)
        let previous=collection("models")[profileID] as? [String:Any]
        models[profileID] = item; var candidate = config; candidate["models"] = models
        var bindings=collection("bindings")
        if let previous,(previous["model"] as? String != modelID || previous["provider_id"] as? String != selectedProvider) {
            for scope in Array(bindings.keys) {if var value=bindings[scope] as? [String:Any],value["model_id"] as? String==profileID {value.removeValue(forKey:"reasoning");bindings[scope]=value}}
        }
        candidate["bindings"]=bindings
        if !binding.isEmpty { bindings[binding] = ["model_id": profileID, "reasoning": reasoning]; candidate["bindings"] = bindings }
        save(candidate)
    }
    private func saveMcp() {
        do {
            guard let object = try JSONSerialization.jsonObject(with: Data(mcpJSON.utf8)) as? [String: Any], let servers = object["mcpServers"] as? [String: Any] else { throw ChatFailure.message("需要 mcpServers 对象") }
            var all = collection("mcp"); all.merge(servers) { _, new in new }; var candidate = config; candidate["mcp"] = all; save(candidate)
        } catch { self.error = terminalError(error) }
    }
    private func uploadRequest(_ command:[String:Any],configuration:Bool) async throws -> [String:Any] {
        guard model.target==destination else{throw ChatFailure.message("账号或 Desktop 已变化，请刷新设置")}
        let result=try await model.request(command,configuration:configuration)
        guard model.target==destination else{throw ChatFailure.message("账号或 Desktop 已变化")};return result
    }
    private func upload(_ url:URL) {
        busy=true
        Task {
            let access=url.startAccessingSecurityScopedResource();defer{if access{url.stopAccessingSecurityScopedResource()};busy=false}
            do {
                let id=url.lastPathComponent
                let begin=try await uploadRequest(["action":"skill_upload_begin","id":id,"expected_revision":snapshot["revision"] ?? 0],configuration:true)
                guard let token=begin["upload_id"] as? String else {throw ChatFailure.message("上传未建立")}
                let base=url.standardizedFileURL.resolvingSymlinksInPath()
                guard let enumerator=FileManager.default.enumerator(at:base,includingPropertiesForKeys:[.isRegularFileKey,.isSymbolicLinkKey]) else {throw ChatFailure.message("无法读取文件夹")}
                var total=0;var count=0
                for case let file as URL in enumerator {
                    let properties=try file.resourceValues(forKeys:[.isRegularFileKey,.isSymbolicLinkKey])
                    if properties.isSymbolicLink==true {throw ChatFailure.message("导入不支持软链接")}
                    guard properties.isRegularFile==true else {continue}
                    count+=1;guard count<=256 else {throw ChatFailure.message("文件数量超过限制")}
                    let path=String(file.path.dropFirst(base.path.count+1));guard path.split(separator:"/").count<=9 else {throw ChatFailure.message("目录层级超过限制")}
                    let handle=try FileHandle(forReadingFrom:file);defer{try? handle.close()}
                    var offset=0
                    repeat {
                        let bytes=try handle.read(upToCount:49152) ?? Data()
                        if bytes.isEmpty && offset>0 {break}
                        total+=bytes.count;guard total<=8388608 && offset+bytes.count<=2097152 else {throw ChatFailure.message("Skill 文件过大")}
                        _ = try await uploadRequest(["action":"skill_upload_chunk","upload_id":token,"path":path,"offset":offset,"data":bytes.base64EncodedString()],configuration:true)
                        offset+=bytes.count;if bytes.isEmpty{break}
                    } while true
                }
                _ = try await uploadRequest(["action":"skill_upload_commit","upload_id":token],configuration:true)
                load()
            } catch {self.error="导入失败："+terminalError(error)}
        }
    }
    private func modify(_ kind: String, _ id: String, enabled: Bool?) {
        var all = collection(kind)
        if let enabled { var item = all[id] as? [String: Any] ?? [:]; item["enabled"] = enabled; all[id] = item } else { all.removeValue(forKey: id) }
        var candidate = config; candidate[kind] = all; save(candidate)
    }
}
