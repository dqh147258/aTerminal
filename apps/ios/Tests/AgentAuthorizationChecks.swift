import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    guard condition() else { fatalError(message) }
}
@MainActor private final class Server {
    var canMutate = true
    var detailPages: [[String: Any]] = []
    var revision = 1
    var full = false
    var mode = "ask"
    var commands: [[String: Any]] = []
    var rows: [[String: Any]] = [
        ["id": "approval", "kind": "approval", "state": "pending", "can_always": true, "session_id": "delegated-session", "arguments_preview": ["command": "rm example"]],
        ["id": "question", "kind": "question", "state": "pending", "question": "Which?", "options": ["A", "B"]]]
    var rules: [[String: Any]] = [["id": "rule", "rule_preview": ["command": "exact", "cwd": "/tmp"]]]
    var conflict = false
    var failedResolve = false
    var lostRevokeReply = false
    func request(_ command: [String: Any]) async throws -> [String: Any] {
        commands.append(command)
        switch command["action"] as? String {
        case "permissions": return ["revision": revision, "permission_mode": mode, "full_authorization": full, "can_mutate": canMutate]
        case "pending": return ["items": rows, "cursor": NSNull(), "has_more": false]
        case "rules": return ["items": rules, "cursor": NSNull(), "has_more": false]
        case "approval_details": return detailPages.removeFirst()
        case "set_permissions":
            if conflict { conflict = false; revision += 1; full = false; throw AuthorizationFailure.message("permission_revision_conflict") }
            check((command["expected_revision"] as? NSNumber)?.intValue == revision, "Missing CAS revision")
            mode = command["permission_mode"] as? String ?? mode
            full = command["full_authorization"] as? Bool ?? full
            revision += 1; return ["revision": revision, "permission_mode": mode, "full_authorization": full, "can_mutate": canMutate]
        case "resolve":
            if failedResolve { failedResolve = false; throw AuthorizationFailure.message("connection lost") }
            let pendingID = command["pending_id"] as? String
            var remaining: [[String: Any]] = []
            for row in rows { if row["id"] as? String != pendingID { remaining.append(row) } }
            rows = remaining
            return ["duplicate": false]
        case "revoke_rule":
            rules = []
            if lostRevokeReply { lostRevokeReply = false; throw AuthorizationFailure.message("lost revoke response") }
            return ["revoked": true]
        default: fatalError("Unexpected command")
        }
    }
}
@main private struct AgentAuthorizationChecks {
    @MainActor static func main() async {
        let editing = AgentInteractionState()
        let question = AgentPending(["id": "same-pending", "kind": "question", "state": "pending", "question": "Choose", "options": ["A", "B"]])!
        editing.bind(scopeKey: "accountA/desktop/session", context: "connection1")
        editing.open(question, kind: .question, scopeKey: "accountA/desktop/session")
        let original = editing.presentation!
        editing.setDraft("typed answer", for: original)
        // A successful pending poll re-creates row values without owning the editor.
        editing.reconcile(scopeKey: original.scopeKey, pending: [question])
        check(editing.isCurrent(original) && editing.draft(original) == "typed answer", "Row refresh destroyed the presentation/draft")
        editing.dismiss()
        editing.open(question, kind: .question, scopeKey: original.scopeKey)
        check(editing.draft(editing.presentation!) == "typed answer", "Dismiss/reopen lost draft")
        // Failed network delivery does not reconcile a successful empty list.
        check(editing.isCurrent(editing.presentation!) && editing.draft(editing.presentation!) == "typed answer", "Unconfirmed answer lost draft")
        editing.bind(scopeKey: "accountB/desktop/session", context: "connection2")
        check(editing.presentation == nil, "Account switch did not dismiss")
        editing.setDraft("late stale edit", for: original)
        editing.open(question, kind: .question, scopeKey: "accountB/desktop/session")
        check(editing.draft(editing.presentation!).isEmpty, "Old account draft leaked")
        editing.setDraft("other account answer", for: editing.presentation!)
        editing.bind(scopeKey: original.scopeKey, context: "connection3")
        editing.open(question, kind: .question, scopeKey: original.scopeKey)
        let recovered = editing.presentation!
        check(editing.draft(recovered) == "typed answer" && !editing.isCurrent(original), "Connection epoch did not isolate old editor")
        editing.setDraft("late old epoch", for: original)
        check(editing.draft(recovered) == "typed answer", "Late edit polluted reconnected editor")
        let cancelled = AgentPending(["id": question.id, "kind": "question", "state": "cancelled", "question": "Choose"])!
        editing.reconcile(scopeKey: original.scopeKey, pending: [cancelled])
        check(editing.presentation == nil && editing.draft(recovered) == "typed answer", "Cancellation cleared an unconsumed draft")
        editing.open(question, kind: .question, scopeKey: original.scopeKey)
        editing.reconcile(scopeKey: original.scopeKey, pending: [])
        check(editing.presentation == nil && editing.draft(recovered).isEmpty, "Consumed question retained draft/presentation")
        editing.setDraft("late after consume", for: recovered)
        check(editing.draft(recovered).isEmpty, "Late input resurrected consumed draft")
        editing.bind(scopeKey: nil, context: "disconnected")
        check(editing.presentation == nil, "Disconnect retained presentation")
        let server = Server(), model = AgentAuthorization()
        model.bind(context: "account1/desktop/session:1", writable: true, transport: server.request)
        check(model.mode == "ask" && !model.full && !model.canAct, "Unsafe default")
        await model.refresh(includeRules: true)
        check(model.canAct && model.pending.count == 2 && model.rules.count == 1, "Recovery failed")
        check(model.pending[0].session == "delegated-session", "Global pending target lost")
        await model.setPermissions(full: true)
        check(model.full, "Server full state not shown")
        await model.refresh()
        check(model.full, "Full did not persist across refresh/run")
        server.conflict = true
        await model.setPermissions(full: true)
        check(!model.full && model.permissions?.revision == 3 && model.error.contains("conflict"), "Conflict overwrote server")
        check(server.commands.filter { $0["action"] as? String == "set_permissions" }.count == 2, "Conflict retried mutation")
        await model.setPermissions(mode: "read_only")
        check(!model.full && model.mode == "read_only", "Read-only became full")
        await model.setPermissions(mode: "ask")
        server.failedResolve = true
        let approval = model.pending[0]
        await model.resolve(approval, decision: "once")
        await model.resolve(approval, decision: "once")
        let resolutions = server.commands.filter { $0["action"] as? String == "resolve" }
        check(resolutions.count == 2 && resolutions[0]["request_id"] as? String == resolutions[1]["request_id"] as? String, "Retry lost idempotency key")
        check(model.pending.count == 1, "Resolved card not refreshed")
        await model.resolve(approval, decision: "always")
        check(server.commands.filter { $0["action"] as? String == "resolve" }.count == 2, "Stale card resolved")
        await model.resolve(model.pending[0], answer: "custom free text")
        check(model.pending.isEmpty && server.commands.contains { $0["answer"] as? String == "custom free text" }, "Free text not delivered")
        await model.revoke(model.rules[0])
        check(model.rules.isEmpty, "Revocation not refreshed")
        server.rules = [["id": "rule", "rule_preview": "same rule regranted"]]
        await model.refresh(includeRules: true); await model.revoke(model.rules[0])
        var revokeIDs: [String] = []
        for command in server.commands where command["action"] as? String == "revoke_rule" { revokeIDs.append(command["request_id"] as! String) }
        check(revokeIDs.count == 2 && revokeIDs[0] != revokeIDs[1] && model.rules.isEmpty, "Regranted rule reused an old revocation ID")
        server.rules = [["id": "rule", "rule_preview": "uncertain regrant"]]
        await model.refresh(includeRules: true); server.lostRevokeReply = true; await model.revoke(model.rules[0])
        check(model.rules.isEmpty, "Applied revoke not recovered after lost response")
        server.rules = [["id": "rule", "rule_preview": "regrant after uncertain revoke"]]
        await model.refresh(includeRules: true); await model.revoke(model.rules[0])
        revokeIDs = []
        for command in server.commands where command["action"] as? String == "revoke_rule" { revokeIDs.append(command["request_id"] as! String) }
        check(revokeIDs.count == 4 && revokeIDs[2] != revokeIDs[3] && model.rules.isEmpty, "Lost revoke reply poisoned a regranted rule")
        model.bind(context: "readonly", writable: false, transport: server.request)
        await model.refresh(); let before = server.commands.count
        await model.setPermissions(full: true)
        check(server.commands.count == before, "Read-only client mutated permissions")
        // The old account returns after a complete new-account read. It must not publish.
        var continuation: CheckedContinuation<[String: Any], Error>?
        model.bind(context: "old", writable: true, transport: { _ in try await withCheckedThrowingContinuation { continuation = $0 } })
        let old = Task { await model.refresh() }
        while continuation == nil { await Task.yield() }
        model.bind(context: "new", writable: true, transport: server.request)
        await model.refresh()
        continuation?.resume(returning: ["revision": 999, "permission_mode": "ask", "full_authorization": true])
        await old.value
        check(!model.full && model.permissions?.revision != 999, "Old account response leaked")
        // A mutation response after a connection epoch change cannot refresh the new context.
        var mutation: CheckedContinuation<[String: Any], Error>?
        model.bind(context: "epoch1", writable: true, transport: { command in
            if command["action"] as? String == "set_permissions" { return try await withCheckedThrowingContinuation { mutation = $0 } }
            return try await server.request(command)
        })
        await model.refresh()
        let stale = Task { await model.setPermissions(full: true) }
        while mutation == nil { await Task.yield() }
        model.bind(context: "epoch2", writable: true, transport: server.request)
        await model.refresh(); let count = server.commands.count
        mutation?.resume(returning: [:]); await stale.value
        check(server.commands.count == count && !model.full, "Old connection mutation published")
        // Unsupported desktops are visible and fail closed, never legacy allow_input fallback.
        model.bind(context: "legacy", writable: true, transport: { _ in [:] })
        await model.refresh()
        check(!model.canAct && model.error.contains("升级 Desktop"), "Legacy Desktop silently accepted")
        let nonPermanent = AgentPending(["id": "raw", "kind": "approval", "state": "pending", "can_always": false])!
        server.rows = [["id": "raw", "kind": "approval", "state": "pending", "can_always": false]]
        model.bind(context: "safe", writable: true, transport: server.request); await model.refresh()
        let start = server.commands.count; await model.resolve(nonPermanent, decision: "always")
        check(server.commands.count == start, "Unstable action granted permanent rule")
        await model.resolve(nonPermanent, decision: "deny")
        check(model.pending.isEmpty, "Deny did not consume pending")
        server.canMutate = false
        model.bind(context: "device-readonly", writable: true, transport: server.request)
        await model.refresh(); let readCount = server.commands.count
        await model.setPermissions(full: true)
        check(!model.canAct && server.commands.count == readCount, "Server read-only grant ignored")
        model.bind(context: "missing-mutate", writable: true, transport: { command in
            var value = try await server.request(command); value.removeValue(forKey: "can_mutate"); return value
        })
        await model.refresh(); check(!model.canAct, "Missing grant became writable")
        server.canMutate = true
        let longRow: [String: Any] = ["id": "long", "kind": "approval", "state": "pending", "can_always": true,
                                    "requires_details": true, "fingerprint": "fp-long", "arguments_preview": "truncated"]
        server.rows = [longRow]
        server.detailPages = [["pending_id": "long", "fingerprint": "fp-changed", "text": "wrong", "cursor": NSNull(), "has_more": false, "truncated": false]]
        model.bind(context: "long-error", writable: true, transport: server.request); await model.refresh()
        let longItem = model.pending[0], noDetailCount = server.commands.count
        check(model.canAct && !model.canApprove(longItem), "Incomplete details enabled approval or disabled deny")
        await model.resolve(longItem, decision: "once"); await model.resolve(longItem, decision: "always")
        check(server.commands.count == noDetailCount, "Wrong details acknowledged")
        await model.resolve(longItem, decision: "deny")
        check(model.pending.isEmpty, "Deny incorrectly required details")
        server.rows = [longRow]
        server.detailPages = [["pending_id": "long", "fingerprint": "fp-long", "text": "first ", "cursor": "page2", "has_more": true, "truncated": false],
                             ["pending_id": "long", "fingerprint": "fp-long", "text": "last", "cursor": NSNull(), "has_more": false, "truncated": false]]
        model.bind(context: "long-complete", writable: true, transport: server.request); await model.refresh()
        check(model.canApprove(model.pending[0]) && model.detailText["long"] == "first last", "Details pages lost")
        await model.resolve(model.pending[0], decision: "always")
        var acknowledged: [String: Any] = [:]
        for command in server.commands { if command["action"] as? String == "resolve" { acknowledged = command } }
        check(acknowledged["details_ack"] as? Bool == true && acknowledged["fingerprint"] as? String == "fp-long", "Resolve did not acknowledge exact details")
        // Offline reading keeps the full detail body, but verification belongs
        // to the connection and must be renewed before approving that body.
        let detailsModel = AgentAuthorization(), detailsServer = Server()
        let completeDetail: [String: Any] = ["pending_id": "long", "fingerprint": "fp-long", "text": "complete cached detail", "cursor": NSNull(), "has_more": false, "truncated": false]
        detailsServer.rows = [longRow]; detailsServer.detailPages = [completeDetail]
        detailsModel.bind(context: "details-1", scopeKey: "details-scope-a", writable: true, transport: detailsServer.request)
        await detailsModel.refresh()
        let detailItem = detailsModel.pending[0]
        check(detailsModel.canApprove(detailItem), "Initial complete details were not verified")
        detailsModel.bind(context: "details-offline", scopeKey: "details-scope-a", writable: false, transport: nil)
        check(detailsModel.detailText["long"] == "complete cached detail" && detailsModel.canReadDetails(detailItem), "Reconnect replaced the full detail body with its preview")
        check(!detailsModel.canAct && !detailsModel.canApprove(detailItem), "Cached details retained connection authority")
        let detailCommands = detailsServer.commands.count
        await detailsModel.resolve(detailItem, decision: "once")
        check(detailsServer.commands.count == detailCommands, "Offline cached detail authorized an action")
        var detailReply: CheckedContinuation<[String: Any], Error>?
        detailsModel.bind(context: "details-2", scopeKey: "details-scope-a", writable: true, transport: { command in
            if command["action"] as? String == "approval_details" { return try await withCheckedThrowingContinuation { detailReply = $0 } }
            return try await detailsServer.request(command)
        })
        let detailsRefresh = Task { await detailsModel.refresh() }
        while detailReply == nil { await Task.yield() }
        check(detailsModel.detailText["long"] == "complete cached detail" && !detailsModel.canApprove(detailItem), "Detail refetch removed readable content or accepted stale verification")
        detailReply?.resume(returning: completeDetail); await detailsRefresh.value
        check(detailsModel.canAct && detailsModel.canApprove(detailItem), "Reconnect did not verify exact complete detail again")
        detailsServer.detailPages = [["pending_id": "long", "fingerprint": "different", "text": "wrong", "cursor": NSNull(), "has_more": false, "truncated": false]]
        detailsModel.bind(context: "details-3", scopeKey: "details-scope-a", writable: true, transport: detailsServer.request)
        await detailsModel.refresh()
        check(detailsModel.detailText["long"] == "complete cached detail" && detailsModel.canReadDetails(detailItem) && !detailsModel.canApprove(detailItem), "Failed detail validation destroyed the readable cache or allowed approval")
        check(detailsModel.detailErrors["long"] != nil, "A real detail validation error was hidden")
        detailsModel.bind(context: "details-other-account", scopeKey: "details-scope-b", writable: false, transport: nil)
        check(detailsModel.detailText.isEmpty && !detailsModel.canReadDetails(detailItem), "Full detail text crossed account scope")
        // The transport may retire before SwiftUI rebinds the authorization
        // generation. Cancellation must not insert a generic error row or leave
        // that still-current generation able to submit live operations.
        let cancelling = AgentAuthorization(), cancellationServer = Server()
        cancellationServer.rows = [longRow]; cancellationServer.detailPages = [completeDetail]
        var cancelAction = ""
        let cancellingTransport: AgentAuthorization.Transport = { command in
            if command["action"] as? String == cancelAction { throw CancellationError() }
            return try await cancellationServer.request(command)
        }
        cancelling.bind(context: "cancellation-1", scopeKey: "cancellation-scope", writable: true, transport: cancellingTransport)
        await cancelling.refresh(includeRules: true)
        let cachedApproval = cancelling.pending[0]
        check(cancelling.canAct && cancelling.canApprove(cachedApproval), "Cancellation fixture did not establish its cached authority")
        var publishedErrors: [String] = []
        cancelling.changed = { publishedErrors.append(cancelling.error) }
        cancelAction = "permissions"
        await cancelling.refresh()
        check(cancelling.permissions == nil && !cancelling.canAct && !cancelling.loading, "Same-generation cancellation retained live authorization")
        check(cancelling.error.isEmpty && publishedErrors.allSatisfy { $0.isEmpty }, "Cancellation inserted a second connection error row")
        check(cancelling.pending.count == 1 && cancelling.rules.count == 1 && cancelling.detailText["long"] == "complete cached detail", "Cancellation discarded visible pending, rules or full details")
        var cancellationCommands = cancellationServer.commands.count
        await cancelling.setPermissions(full: true); await cancelling.resolve(cachedApproval, decision: "once")
        check(cancellationServer.commands.count == cancellationCommands, "Cancelled refresh transmitted a live mutation")
        // On a replacement connection, cached text stays readable while its
        // verification is renewed. Cancelling that detail read must also unwind
        // the permission snapshot fetched earlier in the same refresh.
        cancelAction = "approval_details"
        cancelling.bind(context: "cancellation-2", scopeKey: "cancellation-scope", writable: true, transport: cancellingTransport)
        await cancelling.refresh()
        check(cancelling.permissions == nil && !cancelling.canAct && !cancelling.canApprove(cachedApproval), "Cancelled details finished refresh with old approval authority")
        check(cancelling.error.isEmpty && cancelling.detailErrors.isEmpty && publishedErrors.allSatisfy { $0.isEmpty }, "Cancelled details published a generic or per-card error")
        check(cancelling.detailText["long"] == "complete cached detail" && cancelling.canReadDetails(cachedApproval), "Cancelled details replaced the readable cache")
        cancellationCommands = cancellationServer.commands.count
        await cancelling.setPermissions(full: true); await cancelling.resolve(cachedApproval, decision: "once")
        check(cancellationServer.commands.count == cancellationCommands, "Cancelled detail refresh transmitted a live mutation")
        // Lost response, transport replacement, and explicit retry keep the same
        // account/device/agent idempotency key. Recovery itself sends only reads.
        let resumed = AgentAuthorization(), resumedServer = Server()
        let stableScope = "account-a/desktop-a/session-a"
        resumed.bind(context: "transport-1", scopeKey: stableScope, writable: true, transport: resumedServer.request)
        await resumed.refresh()
        let retryItem = resumed.pending[0]
        resumedServer.failedResolve = true
        await resumed.resolve(retryItem, decision: "once")
        let uncertain = resumedServer.commands.last { $0["action"] as? String == "resolve" }!
        let beforeDisconnect = resumedServer.commands.count
        resumed.bind(context: "offline", scopeKey: stableScope, writable: false, transport: nil)
        check(!resumed.canAct && resumed.permissions == nil && resumed.pending.count == 2, "Reconnect discarded pending UI or retained stale authority")
        await resumed.resolve(retryItem, decision: "once")
        check(resumedServer.commands.count == beforeDisconnect, "Disconnected approval transmitted")
        resumed.bind(context: "transport-2", scopeKey: stableScope, writable: true, transport: resumedServer.request)
        check(!resumed.canAct, "Reconnect enabled action before fresh permission snapshot")
        await resumed.refresh()
        check(resumedServer.commands.filter { $0["action"] as? String == "resolve" }.count == 1, "Recovery automatically replayed approval")
        await resumed.resolve(resumed.pending[0], decision: "once")
        let retried = resumedServer.commands.last { $0["action"] as? String == "resolve" }!
        check(uncertain["request_id"] as? String == retried["request_id"] as? String, "Transport replacement lost the uncertain request ID")
        // Same pending IDs in another account must never reuse an approval token.
        resumedServer.rows = [["id": retryItem.id, "kind": "approval", "state": "pending"]]
        resumed.bind(context: "transport-3", scopeKey: "account-b/desktop-a/session-a", writable: true, transport: resumedServer.request)
        await resumed.refresh(); await resumed.resolve(resumed.pending[0], decision: "once")
        let other = resumedServer.commands.last { $0["action"] as? String == "resolve" }!
        check(uncertain["request_id"] as? String != other["request_id"] as? String, "Approval token crossed account scope")
        // The presentation context is stable across transport changes, preserving
        // the open question, while authorization uses a separate connection epoch.
        let retained = AgentInteractionState()
        retained.bind(scopeKey: stableScope, context: stableScope)
        retained.open(question, kind: .question, scopeKey: stableScope)
        let retainedQuestion = retained.presentation!
        retained.setDraft("answer through offline period", for: retainedQuestion)
        retained.bind(scopeKey: stableScope, context: stableScope)
        check(retained.isCurrent(retainedQuestion) && retained.draft(retainedQuestion) == "answer through offline period", "Transport retry closed the open question")
        let beforeDelayedTap = resumedServer.commands.count
        let delayedTap = resumed.perform { await $0.setPermissions(full: true) }
        resumed.bind(context: "next-transport", scopeKey: "account-b/desktop-a/session-a", writable: true, transport: resumedServer.request)
        await delayedTap.value
        check(resumedServer.commands.count == beforeDelayedTap, "Queued authorization tap crossed the connection epoch")
        // Permission polling and offline recovery must not disable local edits.
        let answerModel = AgentAuthorization(), answerServer = Server()
        var pausePermissions = false
        var permissionReply: CheckedContinuation<[String: Any], Error>?
        answerModel.bind(context: "editable-1", scopeKey: "editable-scope", writable: true, transport: { command in
            if pausePermissions && command["action"] as? String == "permissions" {
                return try await withCheckedThrowingContinuation { permissionReply = $0 }
            }
            return try await answerServer.request(command)
        })
        await answerModel.refresh()
        let editableQuestion = answerModel.pending.first { $0.kind == "question" }!
        check(answerModel.canEditAnswer(editableQuestion), "Confirmed writable question cannot be edited")
        pausePermissions = true
        let permissionPoll = Task { await answerModel.refresh() }
        while permissionReply == nil { await Task.yield() }
        check(answerModel.loading && !answerModel.canAct && answerModel.canEditAnswer(editableQuestion), "Permission poll blocked local answer input")
        permissionReply?.resume(returning: ["revision": 1, "permission_mode": "ask", "full_authorization": false, "can_mutate": true])
        await permissionPoll.value
        let beforeOffline = answerServer.commands.count
        answerModel.bind(context: "editable-offline", scopeKey: "editable-scope", writable: false, transport: nil)
        check(answerModel.canEditAnswer(editableQuestion) && !answerModel.canAct, "Reconnect blocked editing or granted submission")
        await answerModel.resolve(editableQuestion, answer: "Offline draft")
        check(answerServer.commands.count == beforeOffline, "Local offline answer was sent")
        answerModel.bind(context: "other-account", scopeKey: "other-account-scope", writable: true, transport: answerServer.request)
        check(!answerModel.canEditAnswer(editableQuestion), "Question editor crossed account scope")
        answerServer.canMutate = false
        await answerModel.refresh()
        check(!answerModel.canEditAnswer(answerModel.pending.first { $0.kind == "question" }!), "Read-only device enabled answer editing")
        answerModel.bind(context: "readonly-offline", scopeKey: "other-account-scope", writable: false, transport: nil)
        check(!answerModel.canEditAnswer(editableQuestion), "Reconnect changed a known read-only grant")
        print("PASS: scoped presentation/draft lifetime, consumption and stale-editor fences; authorization recovery, CAS, idempotency, grants, detail ACK and legacy fail-closed")
    }
}
