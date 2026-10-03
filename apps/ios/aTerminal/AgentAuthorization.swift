import Foundation

struct AgentPermissions {
    let mode: String
    let full: Bool
    let revision: Int64
    let canMutate: Bool
    init(_ value: [String: Any]) throws {
        guard let mode = value["permission_mode"] as? String, ["ask", "read_only"].contains(mode),
              let full = value["full_authorization"] as? Bool,
              let revision = value["revision"] as? NSNumber else {
            throw AuthorizationFailure.message("Desktop 缺少新版授权功能，请升级 Desktop")
        }
        self.mode = mode; self.full = mode == "ask" && full; self.revision = revision.int64Value; canMutate = value["can_mutate"] as? Bool ?? false
    }
}

struct AgentPending: Identifiable {
    let id: String
    let kind: String
    let state: String
    let title: String
    let reason: String
    let session: String
    let tool: String
    let arguments: String
    let cwd: String
    let rule: String
    let fingerprint: String
    let requiresDetails: Bool
    let canAlways: Bool
    let alwaysUnavailableReason: String
    let question: String
    let options: [String]
    var actionable: Bool { ["pending", "waiting", "waiting_for_user"].contains(state) }
    init?(_ value: [String: Any]) {
        guard let id = value["id"] as? String, let kind = value["kind"] as? String,
              ["approval", "question"].contains(kind) else { return nil }
        self.id = id; self.kind = kind; state = value["state"] as? String ?? "unknown"
        title = value["title"] as? String ?? (kind == "approval" ? "操作授权" : "需要你的答复")
        reason = value["reason"] as? String ?? ""
        session = value["session_id"] as? String ?? "全局对话"
        tool = value["tool"] as? String ?? ""
        arguments = authorizationPreview(value["arguments_preview"])
        cwd = value["cwd"] as? String ?? "未知目录"
        rule = authorizationPreview(value["rule_preview"])
        fingerprint = value["fingerprint"] as? String ?? ""
        requiresDetails = value["requires_details"] as? Bool == true || value["arguments_truncated"] as? Bool == true
        canAlways = value["can_always"] as? Bool ?? false
        alwaysUnavailableReason = value["always_unavailable_reason"] as? String ?? "此操作无法形成稳定的精确规则，仅可授权一次。"
        question = value["question"] as? String ?? title
        options = value["options"] as? [String] ?? []
    }
}
struct AgentRule: Identifiable {
    let id: String
    let preview: String
    init?(_ value: [String: Any]) {
        guard let id = value["id"] as? String else { return nil }
        self.id = id; preview = authorizationPreview(value["rule_preview"] ?? value["preview"] ?? value)
    }
}
private func authorizationPreview(_ value: Any?) -> String {
    guard let value, !(value is NSNull) else { return "" }
    if let text = value as? String { return text }
    guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys, .fragmentsAllowed]) else { return "" }
    return String(decoding: data, as: UTF8.self)
}
enum AuthorizationFailure: LocalizedError {
    case message(String)
    var errorDescription: String? { if case let .message(text) = self { return text }; return nil }
}

/// Foundation-only state machine; every continuation belongs to a single account/scope/connection.
@MainActor final class AgentAuthorization {
    typealias Object = [String: Any]
    typealias Transport = (Object) async throws -> Object
    private(set) var permissions: AgentPermissions?
    private(set) var pending: [AgentPending] = []
    private(set) var rules: [AgentRule] = []
    private(set) var detailText: [String: String] = [:]
    private(set) var detailErrors: [String: String] = [:]
    private var detailFingerprints: [String: String] = [:]
    private(set) var error = ""
    private(set) var busy = false
    private(set) var loading = false
    private(set) var writable = false
    var changed: (() -> Void)?
    private var generation = 0
    private var context = ""
    private var transport: Transport?
    // Keep the idempotency key for a retry after an uncertain network response.
    private var requestIDs: [String: String] = [:]
    var canAct: Bool { writable && permissions?.canMutate == true && !busy }
    var mode: String { permissions?.mode ?? "ask" }
    var full: Bool { permissions?.full ?? false }
    func bind(context: String, writable: Bool, transport: Transport?) {
        if self.context == context { self.writable = writable; self.transport = transport; return }
        generation += 1; self.context = context; self.writable = writable; self.transport = transport
        permissions = nil; pending = []; rules = []; detailText = [:]; detailErrors = [:]; detailFingerprints = [:]; error = ""; busy = false; loading = false; requestIDs = [:]; changed?()
    }
    private func call(_ command: Object, using transport: Transport) async throws -> Object {
        let result = try await transport(command)
        if let error = result["error"], !(error is NSNull) { throw AuthorizationFailure.message(authorizationPreview(error)) }
        return result
    }
    func refresh(includeRules: Bool = false) async {
        guard !loading, !busy, let transport else { return }
        let token = generation; loading = true; changed?()
        defer { if token == generation { loading = false; changed?() } }
        do {
            let value = try await call(["action": "permissions"], using: transport)
            guard token == generation else { return }
            permissions = try AgentPermissions(value)
            let rows = try await list("pending", key: "items", transport: transport, token: token)
            guard token == generation else { return }
            pending = rows.compactMap(AgentPending.init)
            changed?()
            var retained = Set<String>()
            for item in pending { if detailFingerprints[item.id] == item.fingerprint { retained.insert(item.id) } }
            for id in Array(detailText.keys) where !retained.contains(id) { detailText[id] = nil; detailFingerprints[id] = nil }
            for item in pending where item.kind == "approval" && item.actionable && item.requiresDetails && !canApprove(item) {
                await fetchDetails(item, transport: transport, token: token)
                guard token == generation else { return }
            }
            if includeRules {
                let values = try await list("rules", key: "items", transport: transport, token: token)
                guard token == generation else { return }; rules = values.compactMap(AgentRule.init)
                var activeRules = Set<String>()
                for rule in rules { activeRules.insert(rule.id) }
                // A refreshed absence confirms revocation even if its mutation reply was lost.
                for key in Array(requestIDs.keys) where key.hasPrefix("revoke:") {
                    if !activeRules.contains(String(key.dropFirst(7))) { requestIDs[key] = nil }
                }
            }
            error = ""
        } catch {
            guard token == generation else { return }
            // Never offer stale authorization controls after a failed refresh.
            permissions = nil
            let message = error.localizedDescription
            self.error = message.lowercased().contains("unsupported") ? "Desktop 缺少新版授权功能，请升级 Desktop：" + message : "授权状态不可用：" + message
        }
    }
    private func list(_ action: String, key: String, transport: Transport, token: Int) async throws -> [Object] {
        var rows: [Object] = []; var cursor: Any?; var seen = Set<String>()
        repeat {
            var command: Object = ["action": action]; command["cursor"] = cursor
            let result = try await call(command, using: transport)
            guard token == generation else { throw CancellationError() }
            guard let page = result[key] as? [Object] else { throw AuthorizationFailure.message("Desktop 缺少新版\(action)功能，请升级 Desktop") }
            rows += page
            cursor = result["cursor"]; if cursor is NSNull { cursor = nil }
            guard let more = result["has_more"] as? Bool, more == (cursor != nil), cursor == nil || cursor is String else {
                throw AuthorizationFailure.message("Desktop 分页状态不完整")
            }
            if let cursor, !seen.insert(authorizationPreview(cursor)).inserted { throw AuthorizationFailure.message("Desktop 返回重复分页游标") }
            if rows.count > 1000 { throw AuthorizationFailure.message("待办或规则过多，请使用 Desktop 管理") }
        } while cursor != nil
        return rows
    }
    func canApprove(_ item: AgentPending) -> Bool {
        !item.requiresDetails || (!item.fingerprint.isEmpty && detailFingerprints[item.id] == item.fingerprint && detailText[item.id] != nil)
    }
    private func fetchDetails(_ item: AgentPending, transport: Transport, token: Int) async {
        do {
            guard !item.fingerprint.isEmpty else { throw AuthorizationFailure.message("详情缺少动作指纹") }
            var text = ""; var cursor: String?; var seen = Set<String>()
            repeat {
                var command: Object = ["action": "approval_details", "pending_id": item.id]
                command["cursor"] = cursor
                let value = try await call(command, using: transport)
                guard token == generation else { return }
                guard value["pending_id"] as? String == item.id, value["fingerprint"] as? String == item.fingerprint,
                      let fragment = value["text"] as? String, let more = value["has_more"] as? Bool,
                      value["truncated"] as? Bool == false else { throw AuthorizationFailure.message("详情不完整或动作已变化") }
                text += fragment
                guard text.utf8.count <= 4 * 1024 * 1024 else { throw AuthorizationFailure.message("详情超过手机 4 MiB 读取限制，请通过 Desktop 审阅") }
                cursor = value["cursor"] as? String
                guard more == (cursor != nil) else { throw AuthorizationFailure.message("详情分页状态不一致") }
                if let cursor, !seen.insert(cursor).inserted { throw AuthorizationFailure.message("详情返回重复游标") }
            } while cursor != nil
            guard token == generation else { return }
            detailText[item.id] = text; detailFingerprints[item.id] = item.fingerprint; detailErrors[item.id] = nil
            changed?()
        } catch {
            guard token == generation else { return }
            detailErrors[item.id] = "无法取齐详情：" + error.localizedDescription
            changed?()
        }
    }
    func setPermissions(mode: String? = nil, full: Bool? = nil) async {
        guard canAct, let permissions else { return }
        var command: Object = ["action": "set_permissions", "expected_revision": permissions.revision]
        command["permission_mode"] = mode; command["full_authorization"] = full
        await mutate(command)
    }
    func resolve(_ item: AgentPending, decision: String? = nil, answer: String? = nil) async {
        guard canAct, let current = pending.first(where: { $0.id == item.id && $0.actionable }), item.actionable,
              current.fingerprint == item.fingerprint else { return }
        if item.kind == "approval" {
            guard let decision, ["once", "always", "deny"].contains(decision), decision != "always" || current.canAlways, decision == "deny" || canApprove(current) else { return }
        } else { guard let answer, !answer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return } }
        let key = "resolve:\(item.id):\(decision ?? answer ?? "")"
        var command: Object = ["action": "resolve", "pending_id": item.id, "request_id": requestID(key)]
        command["decision"] = decision; command["answer"] = answer
        if item.kind == "approval", decision != "deny", current.requiresDetails {
            command["details_ack"] = true; command["fingerprint"] = current.fingerprint
        }
        await mutate(command)
    }
    func revoke(_ rule: AgentRule) async {
        guard canAct, rules.contains(where: { $0.id == rule.id }) else { return }
        let key = "revoke:" + rule.id
        await mutate(["action": "revoke_rule", "rule_id": rule.id, "request_id": requestID(key)], includeRules: true, idempotencyKey: key)
    }
    private func requestID(_ key: String) -> String {
        if let id = requestIDs[key] { return id }
        let id = UUID().uuidString; requestIDs[key] = id; return id
    }
    private func mutate(_ command: Object, includeRules: Bool = false, idempotencyKey: String? = nil) async {
        guard let transport else { return }
        generation += 1 // Fence any polling response that began before this user action.
        let token = generation; loading = false; busy = true; changed?()
        var failure: String?
        do {
            _ = try await call(command, using: transport)
            if token == generation, let idempotencyKey { requestIDs[idempotencyKey] = nil }
        }
        catch { failure = error.localizedDescription }
        guard token == generation else { return }
        busy = false
        // Re-read on success, conflict and uncertain delivery; never optimistic or automatic retry.
        await refresh(includeRules: includeRules)
        guard token == generation else { return }
        if let failure { error = "操作未确认，已重新读取 Desktop 状态：" + failure }
        changed?()
    }
}

/// Owned by AssistantModel, independently of LazyVStack row and sheet lifetimes.
struct AgentInteractionPresentation: Identifiable {
    enum Kind: String { case question, details }
    let kind: Kind
    let item: AgentPending
    let scopeKey: String
    let context: String
    var id: String { scopeKey + ":" + item.id + ":" + kind.rawValue }
}

@MainActor final class AgentInteractionState {
    private struct DraftKey: Hashable { let scope: String; let pending: String }
    private(set) var presentation: AgentInteractionPresentation?
    private var drafts: [DraftKey: String] = [:]
    private var consumed = Set<DraftKey>()
    private var scopeKey: String?
    private var context = ""
    var changed: (() -> Void)?
    func bind(scopeKey: String?, context: String) {
        guard self.scopeKey != scopeKey || self.context != context else { return }
        self.scopeKey = scopeKey; self.context = context
        presentation = nil; changed?()
    }
    func open(_ item: AgentPending, kind: AgentInteractionPresentation.Kind, scopeKey: String) {
        guard self.scopeKey == scopeKey,
              (kind == .question && item.kind == "question" && item.actionable) || (kind == .details && item.kind == "approval") else { return }
        let key = DraftKey(scope: scopeKey, pending: item.id)
        guard !consumed.contains(key) else { return }
        presentation = AgentInteractionPresentation(kind: kind, item: item, scopeKey: scopeKey, context: context)
        changed?()
    }
    func dismiss() { presentation = nil; changed?() }
    private func matches(_ value: AgentInteractionPresentation) -> Bool { scopeKey == value.scopeKey && context == value.context }
    func isCurrent(_ value: AgentInteractionPresentation) -> Bool { matches(value) && presentation?.id == value.id }
    func draft(_ value: AgentInteractionPresentation) -> String {
        guard matches(value) else { return "" }
        return drafts[DraftKey(scope: value.scopeKey, pending: value.item.id)] ?? ""
    }
    func setDraft(_ text: String, for value: AgentInteractionPresentation) {
        let key = DraftKey(scope: value.scopeKey, pending: value.item.id)
        guard value.kind == .question, matches(value), !consumed.contains(key),
              presentation == nil || presentation?.id == value.id else { return }
        // Dismissal keeps the same pending draft; a late edit cannot cross scope/epoch.
        drafts[key] = text; changed?()
    }
    /// Call only for a fully fetched, successful pending snapshot in this context.
    func reconcile(scopeKey: String, pending: [AgentPending]) {
        guard self.scopeKey == scopeKey else { return }
        var current: [String: AgentPending] = [:]
        for item in pending { current[item.id] = item }
        for key in Array(drafts.keys) where key.scope == scopeKey {
            // The server omits consumed requests; cancelled/expired requests remain
            // visible and must retain their draft rather than imply consumption.
            if current[key.pending] == nil || current[key.pending]?.state == "consumed" {
                drafts[key] = nil; consumed.insert(key)
            }
        }
        if let value = presentation, value.scopeKey == scopeKey {
            if let item = current[value.item.id] {
                if item.kind != value.item.kind || item.fingerprint != value.item.fingerprint || !item.actionable && value.kind == .question { presentation = nil }
            } else { presentation = nil }
        }
        changed?()
    }
}

#if DEBUG
/// In-memory UI fixture, enabled only alongside the existing isolated settings fixture.
@MainActor final class AuthorizationFixture {
    static let shared = AuthorizationFixture()
    static var enabled: Bool { ProcessInfo.processInfo.arguments.contains("--authorization-fixture") }
    private var permissions: [String: [String: Any]] = [:]
    private var resolved = Set<String>()
    private var savedRules: [[String: Any]] = []
    private var result = ""
    func request(_ command: [String: Any], scope: String) -> [String: Any]? {
        let mode = ProcessInfo.processInfo.arguments.first { $0.hasPrefix("--authorization-scenario=") }?.split(separator: "=").last.map(String.init) ?? ""
        let current = permissions[scope] ?? ["permission_mode": "ask", "full_authorization": false, "revision": 1, "can_mutate": mode != "readonly"]
        var approval: [String: Any] = ["id": "fixture-approval", "kind": "approval", "state": "pending", "title": "确认终端操作", "session_id": "fixture-delegated-session", "tool": "run_program", "arguments_preview": ["program": "/usr/bin/tee", "args": ["-a", "example.txt"], "stdin": "example\n"], "cwd": "/fixture/project", "reason": "此命令会创建文件", "can_always": mode != "nonpermanent", "rule_preview": "run_program /usr/bin/tee -a example.txt · stdin=example\n · /fixture/project · v3"]
        if mode == "nonpermanent" {
            approval["tool"] = "run_command"; approval["arguments_preview"] = ["command": "touch example.txt"]
            approval["always_unavailable_reason"] = "交互终端输入无法固定实际 Shell 程序身份，仅支持授权一次。"
        }
        if mode.hasPrefix("long") {
            approval["requires_details"] = true; approval["arguments_truncated"] = true; approval["fingerprint"] = "fixture-long-fingerprint"
            approval["arguments_preview"] = "TRUNCATED preview"
        }
        let question: [String: Any] = ["id": "fixture-question", "kind": "question", "state": "pending", "title": "需要你的答复", "question": "选择输出格式，也可以输入其他要求", "session_id": "fixture-delegated-session", "options": ["Markdown", "纯文本"]]
        let rows = [approval, question].filter { !resolved.contains(scope + ($0["id"] as! String)) }
        switch command["action"] as? String {
        case "permissions": return mode == "unsupported" ? ["error": "unsupported action permissions"] : current
        case "set_permissions":
            if mode == "readonly" { return ["error": "device_read_only"] }
            var next = current; next["revision"] = (current["revision"] as? Int ?? 0) + 1
            if let value = command["permission_mode"] { next["permission_mode"] = value }
            if let value = command["full_authorization"] { next["full_authorization"] = value }
            if next["permission_mode"] as? String == "read_only" { next["full_authorization"] = false }
            permissions[scope] = next; return next
        case "pending": return ["items": rows, "cursor": NSNull(), "has_more": false]
        case "approval_details":
            if mode == "long-failure" { return ["error": "fixture detail delivery failed"] }
            let second = command["cursor"] != nil
            return ["pending_id": "fixture-approval", "fingerprint": "fixture-long-fingerprint",
                    "text": second ? "literal detail end" : "Complete command: /usr/bin/printf ",
                    "cursor": second ? NSNull() : "detail-next", "has_more": !second, "truncated": false]
        case "rules": return ["items": savedRules, "cursor": NSNull(), "has_more": false]
        case "resolve":
            if mode == "readonly" { return ["error": "device_read_only"] }
            let id = command["pending_id"] as? String ?? ""
            if mode.hasPrefix("long"), id == "fixture-approval", command["decision"] as? String != "deny" {
                guard command["details_ack"] as? Bool == true, command["fingerprint"] as? String == "fixture-long-fingerprint" else { return ["error": "missing exact details acknowledgement"] }
            }
            resolved.insert(scope + id)
            if command["decision"] as? String == "always" { savedRules = [["id": "fixture-rule", "rule_preview": "run_program /usr/bin/tee -a example.txt · stdin=example\n · /fixture/project · v3"]] }
            result = command["answer"] as? String ?? command["decision"] as? String ?? ""
            return ["pending": ["id": id, "state": "resolved"], "duplicate": false]
        case "revoke_rule": savedRules = []; return ["revoked": true, "rule_id": "fixture-rule"]
        case "state": return ["available": true, "state": rows.isEmpty ? "idle" : "waiting_for_user", "history_generation": 1]
        case "cancel": resolved.insert(scope + "fixture-approval"); resolved.insert(scope + "fixture-question"); result = "cancelled"; return [:]
        case "history": return ["generation": 1, "has_more": false, "items": [["id": "fixture-result", "sequence": 1, "kind": "assistant", "value": ["text": "Fixture result: " + result]]]]
        default: return nil
        }
    }
}
#endif
