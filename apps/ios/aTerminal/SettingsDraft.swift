import Foundation

struct ProviderDraft {
    var id = "", protocolName = "openai_responses", endpoint = "https://api.openai.com/v1", apiVersion = "", key = ""
    var enabled = true
    init(id: String = "", item: [String: Any] = [:]) {
        self.id = id
        let connection = item["connection"] as? [String: Any] ?? [:]
        protocolName = connection["protocol"] as? String ?? protocolName; endpoint = connection["endpoint"] as? String ?? endpoint
        apiVersion = connection["api_version"] as? String ?? ""; enabled = item["enabled"] as? Bool ?? true
    }
    func secrets() -> [String: String] {
        key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? [:] : [id: key]
    }
    func value(previous: [String: Any]?) throws -> [String: Any] {
        guard SettingsValidation.endpoint(endpoint) else { throw SettingsFailure.message("API 地址须为无用户凭据的 HTTP(S) URL") }
        guard protocolName != "azure_openai" || !apiVersion.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw SettingsFailure.message("Azure 需要 API version") }
        var value = previous ?? ["id": id, "name": id, "credential_revision": 0]
        var connection = value["connection"] as? [String: Any] ?? [:]
        connection["protocol"] = protocolName; connection["endpoint"] = endpoint
        connection["api_version"] = protocolName == "azure_openai" ? apiVersion : nil
        value["connection"] = connection; value["enabled"] = enabled; return value
    }
}

struct ModelDraft {
    var id = "", provider = "", model = "", context = "128000", output = "4096", temperature = "", topP = ""
    var tools = false, vision = false, adaptive = false, disabled = false, advanced = false
    var levels = "", budgetMin = "", budgetMax = "", mode = "provider_default", strength = "", binding = ""
    var caps: [String: Any] = [:]
    var capabilitiesFromSnapshot = true
    init(id: String = "", item: [String: Any] = [:]) {
        self.id = id; provider = item["provider_id"] as? String ?? ""; model = item["model"] as? String ?? ""
        context = (item["context_window"] as? NSNumber)?.stringValue ?? context; output = (item["max_tokens"] as? NSNumber)?.stringValue ?? output
        temperature = (item["temperature"] as? NSNumber)?.stringValue ?? ""; topP = (item["top_p"] as? NSNumber)?.stringValue ?? ""
        applyCapabilities(item["capabilities"] as? [String: Any] ?? [:])
        let reasoning = item["reasoning"] as? [String: Any] ?? [:]
        mode = reasoning["mode"] as? String ?? mode; strength = reasoning["level"] as? String ?? (reasoning["tokens"] as? NSNumber)?.stringValue ?? ""
    }
    mutating func applyCapabilities(_ value: [String: Any]) {
        caps = value; tools = value["tools"] as? Bool ?? false; vision = value["vision"] as? Bool ?? false
        levels = (value["reasoning_levels"] as? [String] ?? []).joined(separator: ",")
        let budget = value["reasoning_budget"] as? [Int] ?? []
        budgetMin = budget.first.map(String.init) ?? ""; budgetMax = budget.last.map(String.init) ?? ""
        adaptive = value["reasoning_adaptive"] as? Bool ?? false; disabled = value["reasoning_disabled"] as? Bool ?? false
    }
    mutating func resetCapabilities() { applyCapabilities([:]); capabilitiesFromSnapshot = false; mode = "provider_default"; strength = ""; temperature = ""; topP = "" }
    mutating func select(_ row: [String: Any]) {
        model = row["id"] as? String ?? ""; resetCapabilities()
        applyCapabilities(row["capabilities"] as? [String: Any] ?? [:])
        if let value = row["context_window"] as? NSNumber { context = value.stringValue }
        if let value = row["max_output_tokens"] as? NSNumber { output = value.stringValue }
    }
    func candidate(_ config: [String: Any], editing: Bool) throws -> [String: Any] {
        var candidate = config; var models = config["models"] as? [String: Any] ?? [:]
        try SettingsValidation.id(id, kind: "models", editing: editing, items: models)
        guard let providers = config["providers"] as? [String: Any], let p = providers[provider] as? [String: Any] else { throw SettingsFailure.message("请选择有效供应商") }
        guard !model.isEmpty, model.utf8.count <= 256 else { throw SettingsFailure.message("模型 ID 须为 1–256 字节") }
        guard let context = Int(context), (4096...4000000).contains(context), let output = Int(output), output > 0, output < context - 2048 else { throw SettingsFailure.message("上下文须为 4096–4000000；最大输出大于 0 且小于上下文减 2048") }
        var item = models[id] as? [String: Any] ?? ["id": id, "name": id]
        var caps = capabilitiesFromSnapshot ? item["capabilities"] as? [String: Any] ?? self.caps : self.caps
        for (field, value, range) in [("temperature", temperature, 0.0...2.0), ("top_p", topP, 0.0...1.0)] {
            if value.isEmpty { item.removeValue(forKey: field) }
            else {
                guard let number = Double(value), number.isFinite, range.contains(number), field != "top_p" || number > 0, caps[field] as? Bool != false else { throw SettingsFailure.message("温度须在 0–2，Top P 须大于 0 且不超过 1，并受模型支持") }
                item[field] = number
            }
        }
        caps["tools"] = tools; caps["vision"] = vision; caps["source"] = "user_declared"
        caps["reasoning_levels"] = levels.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        caps["reasoning_adaptive"] = adaptive; caps["reasoning_disabled"] = disabled
        if !budgetMin.isEmpty || !budgetMax.isEmpty {
            guard let low = Int(budgetMin), let high = Int(budgetMax), low >= 0, high >= low else { throw SettingsFailure.message("预算上下限须为非负整数，下限不超过上限") }
            caps["reasoning_budget"] = [low, high]
        } else { caps.removeValue(forKey: "reasoning_budget") }
        var reasoning: [String: Any] = ["mode": mode]
        if mode == "level" { reasoning["level"] = strength }
        if mode == "budget" { guard let number = Int(strength), number >= 0 else { throw SettingsFailure.message("请输入有效思考 token 预算") }; reasoning["tokens"] = number }
        let protocolName = (p["connection"] as? [String: Any])?["protocol"] as? String ?? ""
        let sampling = !temperature.isEmpty || !topP.isEmpty
        try SettingsValidation.reasoning(protocolName, reasoning, caps, output, sampling)
        item.merge(["provider_id": provider, "model": model, "context_window": context, "max_tokens": output, "capabilities": caps, "reasoning": reasoning, "read_only": !tools || (item["read_only"] as? Bool ?? false)]) { _, new in new }
        models[id] = item; candidate["models"] = models
        var bindings = config["bindings"] as? [String: Any] ?? [:]
        // A same-ID catalog selection or an advanced edit can invalidate scope overrides too.
        for key in Array(bindings.keys) {
            guard var value = bindings[key] as? [String: Any], value["model_id"] as? String == id,
                  let override = value["reasoning"] as? [String: Any] else { continue }
            do { try SettingsValidation.reasoning(protocolName, override, caps, output, sampling) }
            catch { value.removeValue(forKey: "reasoning"); bindings[key] = value }
        }
        if !binding.isEmpty { var value = bindings[binding] as? [String: Any] ?? [:]; value["model_id"] = id; value["reasoning"] = reasoning; bindings[binding] = value }
        candidate["bindings"] = bindings; return candidate
    }
}

enum SettingsValidation {
    static let protocols = ["openai_responses", "openai_chat", "anthropic", "gemini", "azure_openai", "ollama"]
    static func encode(_ value: Any) -> String { (try? JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])).map { String(decoding: $0, as: UTF8.self) } ?? "" }
    static func endpoint(_ value: String) -> Bool {
        guard let url = URLComponents(string: value) else { return false }
        return ["http", "https"].contains(url.scheme ?? "") && !(url.host ?? "").isEmpty && url.user == nil && url.password == nil
    }
    static func absolutePath(_ value: String) -> Bool {
        guard !value.contains("\0") else { return false }
        if value.hasPrefix("/") { return true }
        if value.range(of: "^[A-Za-z]:[/\\\\]", options: .regularExpression) != nil { return true }
        let parts = value.dropFirst(2).split(whereSeparator: { $0 == "\\" || $0 == "/" })
        return value.hasPrefix("\\\\") && parts.count >= 2
    }
    static func id(_ id: String, kind: String, editing: Bool, items: [String: Any]) throws {
        let extensionID = kind == "skills" || kind == "mcp"
        let name = extensionID && id.hasPrefix("user/") ? String(id.dropFirst(5)) : id
        guard name.range(of: "^[A-Za-z0-9_-]{1,128}$", options: .regularExpression) != nil, name != "builtin", !extensionID || !id.hasPrefix("builtin") else { throw SettingsFailure.message("ID 须为 1–128 位字母、数字、- 或 _，不能使用内置 ID") }
        if editing && items[id] == nil { throw SettingsFailure.message("此配置已删除，请返回列表重新检查") }
        if !editing && (items[id] != nil || (kind == "skills" && (items[name] != nil || items["user/" + name] != nil))) { throw SettingsFailure.message("此 ID 已存在") }
    }
    static func reasoning(_ protocolName: String, _ value: [String: Any], _ caps: [String: Any], _ max: Int, _ sampling: Bool) throws {
        var valid = true
        switch value["mode"] as? String ?? "provider_default" {
        case "level": valid = ["openai_responses", "openai_chat", "azure_openai", "gemini"].contains(protocolName) && (caps["reasoning_levels"] as? [String] ?? []).contains(value["level"] as? String ?? "")
        case "budget":
            let range = caps["reasoning_budget"] as? [Int] ?? []; let tokens = value["tokens"] as? Int ?? -1
            valid = ["anthropic", "gemini"].contains(protocolName) && range.count == 2 && tokens >= range[0] && tokens <= range[1] && tokens < max && !(protocolName == "anthropic" && sampling)
        case "adaptive": valid = protocolName == "anthropic" && caps["reasoning_adaptive"] as? Bool == true && !sampling
        case "disabled": valid = ["anthropic", "gemini", "ollama"].contains(protocolName) && caps["reasoning_disabled"] as? Bool == true
        case "provider_default": break
        default: valid = false
        }
        if !valid { throw SettingsFailure.message("思考模式/等级/预算须受协议和模型能力支持；预算小于输出，Anthropic 思考不能同时设置采样") }
    }
    static func mcp(_ id: String, _ value: [String: Any]) throws {
        try self.id(id, kind: "mcp", editing: false, items: [:])
        let command = value["command"] as? String, url = value["url"] as? String
        let transport = value["transport"] as? String ?? (command != nil && url == nil ? "stdio" : url != nil && command == nil ? "streamable_http" : "")
        guard transport == "stdio" ? !(command ?? "").isEmpty && url == nil : transport == "streamable_http" && command == nil && endpoint(url ?? "") else { throw SettingsFailure.message("检查 transport、command 和 url") }
        let args = value["args"] as? [String] ?? []
        let env = value["env"] as? [String: Any] ?? [:], refs = value["envSecretRefs"] as? [String: Any] ?? [:]
        let headers = value["headers"] as? [String: Any] ?? [:], headerRefs = value["headerSecretRefs"] as? [String: Any] ?? [:]
        guard args.count <= 128, args.reduce(0, { $0 + $1.utf8.count }) <= 16000, env.count + refs.count <= 128, headers.count + headerRefs.count <= 32,
              (100...60000).contains(value["startup_timeout_ms"] as? Int ?? 10000), (100...300000).contains(value["call_timeout_ms"] as? Int ?? 30000),
              (Array(env.keys) + Array(refs.keys)).allSatisfy({ !$0.isEmpty && !$0.contains("=") && !$0.contains("\0") }) else { throw SettingsFailure.message("MCP 参数、环境变量或超时超出限制") }
    }
    static func skillFiles(_ root: URL) throws -> [(String, URL)] {
        var result: [(String, URL)] = []; var total = 0
        func walk(_ folder: URL, _ prefix: String, _ depth: Int) throws {
            guard depth <= 8 else { throw SettingsFailure.message("文件夹层级超过 8 层") }
            for url in try FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey, .fileSizeKey, .isRegularFileKey]) {
                let name = url.lastPathComponent; let path = prefix + name
                let values = try url.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey, .fileSizeKey, .isRegularFileKey])
                guard values.isSymbolicLink != true, name != ".", name != "..", !name.contains("/") else { throw SettingsFailure.message("Skill 不支持符号链接或无效文件名") }
                if values.isDirectory == true { try walk(url, path + "/", depth + 1) }
                else {
                    let size = values.fileSize ?? 0; total += size
                    guard values.isRegularFile == true, result.count < 256, size <= 2097152, total <= 8388608 else { throw SettingsFailure.message("Skill 文件数量或大小超限") }
                    result.append((path, url))
                }
            }
        }
        try walk(root, "", 0)
        guard result.contains(where: { $0.0 == "SKILL.md" }) else { throw SettingsFailure.message("文件夹根目录需要 SKILL.md") }
        return result
    }
}

enum SettingsFailure: LocalizedError {
    case message(String)
    var errorDescription: String? { if case .message(let text) = self { return text }; return nil }
}

/// Foundation-only production request coordinator. A transport is bound to one identity/connection epoch.
@MainActor final class ConfigurationSession {
    typealias Object = [String: Any]
    typealias Transport = (Object) async throws -> Object
    private(set) var snapshot: Object = [:]
    private(set) var busy = false
    private(set) var loaded = false
    var error = "" { didSet { changed?() } }
    var changed: (() -> Void)?
    private let transport: Transport
    private let identity: () -> String
    private let connectionEpoch: () -> Int
    private var operationConnection: Int?
    private let origin: String
    private var active = true
    private var serial = 0
    init(identity: @escaping () -> String, connectionEpoch: @escaping () -> Int = { 0 }, transport: @escaping Transport) {
        self.identity = identity; origin = identity(); self.connectionEpoch = connectionEpoch; self.transport = transport
    }
    var valid: Bool { active && identity() == origin }
    func cancelRead() { serial += 1; busy = false; changed?() }
    func close() { active = false; serial += 1; busy = false; changed?() }
    func call(_ command: Object) async throws -> Object {
        guard valid else { throw SettingsFailure.message("账号或 Desktop 连接已变化，请重新打开设置") }
        let connection = connectionEpoch()
        guard !busy || operationConnection == nil || operationConnection == connection else { throw SettingsFailure.message("连接已变化，草稿已保留，请确认状态后重试") }
        let result = try await transport(command)
        guard connectionEpoch() == connection else { throw SettingsFailure.message("连接已变化，操作未确认；草稿已保留") }
        guard valid else { throw SettingsFailure.message("账号或 Desktop 连接已变化，请重新打开设置") }
        return result
    }
    @discardableResult func run(mutation: Bool = false, operation: @escaping () async throws -> Object, done: @escaping (Object) -> Void = { _ in }) -> Task<Void, Never>? {
        guard !busy, valid else { if !valid { error = "连接已变化，请重新打开设置" }; return nil }
        busy = true; error = ""; serial += 1; let ticket = serial; let connection = connectionEpoch()
        operationConnection = connection; changed?()
        return Task {
            do {
                let result = try await operation()
                guard valid, serial == ticket, connectionEpoch() == connection else { if active && serial == ticket { busy = false; self.error = "连接已变化，操作未确认；草稿已保留，请确认状态后重试"; changed?() }; return }
                if mutation { snapshot = result; loaded = true }
                busy = false; changed?(); done(result)
            } catch {
                guard valid, serial == ticket, connectionEpoch() == connection else { if active && serial == ticket { busy = false; self.error = "连接已变化，操作未确认；草稿已保留，请确认状态后重试"; changed?() }; return }
                let message = error.localizedDescription
                let conflict = message.localizedCaseInsensitiveContains("revision")
                let fresh = conflict ? try? await call(["action": "show"]) : nil
                guard valid, serial == ticket, connectionEpoch() == connection else { if active && serial == ticket { busy = false; self.error = "连接已变化，操作未确认；草稿已保留，请确认状态后重试"; changed?() }; return }
                if let fresh { snapshot = fresh }
                busy = false
                self.error = conflict ? "配置已变化，\(fresh == nil ? "刷新失败" : "已刷新")。草稿已保留，请检查后再次保存。" : "未完成：\(message)"
                changed?()
            }
        }
    }
    @discardableResult func load() -> Task<Void, Never>? {
        run(operation: { try await self.call(["action": "show"]) }) { self.snapshot = $0; self.loaded = true; self.changed?() }
    }
    @discardableResult func save(_ config: Object, secrets: [String: String] = [:], done: @escaping () -> Void = {}) -> Task<Void, Never>? {
        let command: Object = ["action": "replace", "expected_revision": snapshot["revision"] ?? 0, "config": config, "secrets": secrets]
        return run(mutation: true, operation: {
            do { return try await self.call(command) }
            catch {
                var message = error.localizedDescription
                for secret in secrets.values where !secret.isEmpty { message = message.replacingOccurrences(of: secret, with: "[密钥]") }
                throw SettingsFailure.message(message)
            }
        }) { _ in done() }
    }
}

#if DEBUG
@MainActor final class SettingsFixture {
    static let shared = SettingsFixture()
    static var enabled: Bool { ProcessInfo.processInfo.arguments.contains("--settings-fixture") }
    private var revision = 1
    private var failed = false
    private var config: [String: Any] = SettingsFixture.configuration
    private var skillBody = "# Fixture Skill\n\nA fixture with complete resources."
    static var configuration: [String: Any] {
        let caps: [String: Any] = ["tools": true, "vision": true, "streaming": true, "temperature": true, "top_p": true, "reasoning_levels": ["low", "high"], "sentinel": "caps"]
        let model: [String: Any] = ["id": "fixture-model", "name": "Fixture model", "model": "fixture-one", "provider_id": "openai", "context_window": 128000, "max_tokens": 4096, "capabilities": caps, "reasoning": ["mode": "provider_default"], "sentinel": "model"]
        return ["schema_version": 1, "sentinel": "root",
                "providers": ["openai": ["id": "openai", "name": "OpenAI", "enabled": true, "credential_revision": 1, "connection": ["protocol": "openai_responses", "endpoint": "https://fixture.invalid/v1", "sentinel": "connection"], "sentinel": "provider"], "azure": ["id": "azure", "name": "Azure", "enabled": true, "credential_revision": 1, "connection": ["protocol": "azure_openai", "endpoint": "https://azure.fixture.invalid", "api_version": "2025-01-01"]]],
                "models": ["fixture-model": model, "fixture-two": ["id": "fixture-two", "name": "Second model", "model": "fixture-two", "provider_id": "azure", "context_window": 128000, "max_tokens": 4096, "capabilities": caps]],
                "bindings": ["global": ["model_id": "fixture-model", "sentinel": "binding"], "session-default": ["model_id": "fixture-model"]],
                "terminal_reading": ["head_lines": 10, "tail_lines": 20, "sentinel": "reading"],
                "mcp": ["user/fixture": ["command": "fixture-command", "args": [], "enabled": true, "sentinel": "mcp"]],
                "skills": ["user/fixture": ["name": "Fixture Skill", "enabled": true, "sentinel": "skill", "description": "Includes a binary resource"]]]
    }
    func request(_ command: [String: Any]) async throws -> [String: Any] {
        let action = command["action"] as? String ?? ""
        let scenario = ProcessInfo.processInfo.arguments.first { $0.hasPrefix("--settings-fixture-scenario=") }?.split(separator: "=").last.map(String.init) ?? ""
        if action == "replace" || action.hasPrefix("skill_") && action != "skill_read" {
            if scenario == "save-delayed" { try await Task.sleep(nanoseconds: 3_500_000_000) }
            if !failed && ["save-failure", "conflict"].contains(scenario) {
                failed = true
                if scenario == "conflict" { revision += 1; throw SettingsFailure.message("revision conflict") }
                throw SettingsFailure.message("fixture save failure")
            }
        }
        switch action {
        case "show": break
        case "replace": config = command["config"] as? [String: Any] ?? config; revision += 1
        case "discover":
            try await Task.sleep(nanoseconds: 150_000_000)
            let second = command["cursor"] != nil
            let rows: [[String: Any]] = [["id": second ? "fixture-page-two" : "fixture-one", "context_window": 256000, "max_output_tokens": 8192, "capabilities": ["tools": true, "vision": false, "streaming": false, "temperature": false, "top_p": false, "reasoning_levels": ["low", "high"]]]]
            return ["models": rows, "cursor": second ? NSNull() : "page-two"]
        case "skill_read": return ["body": skillBody]
        case "skill_edit": skillBody = command["body"] as? String ?? skillBody; revision += 1
        case "skill_install", "skill_upload_commit":
            var items = config["skills"] as? [String: Any] ?? [:]; items[command["id"] as? String ?? "fixture-upload"] = ["name": "Installed fixture", "enabled": true]; config["skills"] = items; revision += 1
        case "skill_upload_begin": return ["upload_id": "fixture-upload"]
        case "skill_upload_chunk": return [:]
        default: throw SettingsFailure.message("Unsupported fixture action")
        }
        return ["revision": revision, "config": config]
    }
    static func agent(_ command: [String: Any], session: String?) -> [String: Any] {
        let row: [String: Any] = ["scope": ["agent": "fixture-global"], "title": "Fixture 全局会话", "preview": "跨终端工作", "state": "idle", "last_reply_sequence": 1]
        switch command["action"] as? String {
        case "global_list": return ["conversations": [row], "cursor": NSNull()]
        case "global_create": return ["scope": ["agent": UUID().uuidString]]
        case "state": return ["available": true, "state": "idle", "history_generation": 1]
        case "history": return ["generation": 1, "has_more": false, "items": [["id": "fixture-message", "sequence": 1, "kind": "assistant", "value": ["text": session == "fixture-closed" ? "正文专有词 星河缓存检索 cedar-body-only-731" : session?.hasPrefix("global:") == true ? "Global fixture 消息" : "Session fixture 消息"]]]]
        case "list": return ["agents": [["scope": ["session": "fixture-session"]], ["scope": ["session": "fixture-closed"]]]]
        default: return [:]
        }
    }
}
#endif
