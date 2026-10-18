import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    guard condition() else { fatalError(message) }
}

private enum FixtureFailure: LocalizedError {
    case temporary, authentication, validation, disk
    var errorDescription: String? {
        switch self {
        case .temporary: return "connection reset by peer"
        case .authentication: return "HTTP status client error (401 Unauthorized)"
        case .validation: return "invalid_permission_mode"
        case .disk: return "fixture disk unavailable"
        }
    }
}

private final class Writes {
    private let lock = NSLock()
    private var names: [String] = []
    private var mainWrites: [String] = []
    func save(_ data: Data, to url: URL) throws {
        try data.write(to: url, options: .atomic)
        lock.lock(); defer { lock.unlock() }
        names.append(url.lastPathComponent)
        if Thread.isMainThread { mainWrites.append(url.lastPathComponent) }
    }
    func count(_ name: String) -> Int {
        lock.lock(); defer { lock.unlock() }; return names.filter { $0 == name }.count
    }
    func ranOnMain(_ name: String) -> Bool { lock.lock(); defer { lock.unlock() }; return mainWrites.contains(name) }
}

@main private struct AgentChatDeliveryChecks {
    @MainActor static func main() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("agent-delivery-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let writes = Writes()
        let store = AgentComposerStore(root: root, debounce: 0.15, writer: writes.save)
        let attachment = AgentAttachment(mime: "image/png", data: Data(repeating: 42, count: 1024 * 1024))
        var first = AgentComposer(text: "原消息 A", images: [attachment], requestID: "request-a", allowInput: nil)
        let delivery = AgentSendSnapshot(scopeKey: "account-a-agent-a", composer: first, permissionMode: "ask")
        try store.flush(first, key: delivery.scopeKey)
        try store.saveDelivery(delivery)
        first.text = "输入中的草稿 B"; first.requestID = "request-b"; first.images = []
        check(delivery.composer.text == "原消息 A" && delivery.composer.images.count == 1, "Editing changed the immutable send snapshot")

        // A response can arrive before the newly edited draft reaches disk.
        store.schedule(first, key: delivery.scopeKey) { _ in fatalError("Unexpected persistence failure") }
        try store.confirm(delivery)
        check(store.draft(delivery.scopeKey).text == first.text, "An old ACK erased a queued newer draft")
        check(store.delivery(delivery.scopeKey) == nil, "Confirmed delivery was retained")
        try await Task.sleep(nanoseconds: 400_000_000)
        let reopened = AgentComposerStore(root: root)
        check(reopened.draft(delivery.scopeKey).requestID == "request-b", "Newer draft was lost after reopening")
        check(reopened.delivery(delivery.scopeKey) == nil, "A delayed save resurrected a confirmed send")

        // A matching queued draft must not reappear after its send is confirmed.
        let same = AgentComposer(text: "确认后清除", images: [], requestID: "same-id", allowInput: nil)
        let sameDelivery = AgentSendSnapshot(scopeKey: "account-a-agent-b", composer: same, permissionMode: "read_only")
        try store.saveDelivery(sameDelivery)
        store.schedule(same, key: sameDelivery.scopeKey) { _ in fatalError("Unexpected persistence failure") }
        try store.confirm(sameDelivery)
        try await Task.sleep(nanoseconds: 400_000_000)
        check(reopened.draft(sameDelivery.scopeKey).text.isEmpty, "Delayed same-ID draft was resurrected after acknowledgement")

        // Reopening and retrying keep exactly the original request, images and mode.
        try store.saveDelivery(delivery)
        let retried = reopened.delivery(delivery.scopeKey)!
        check(retried.requestID == "request-a" && retried.composer.text == "原消息 A" && retried.composer.images[0].data == attachment.data && retried.permissionMode == "ask", "Restart changed the retry payload/identity")
        check(reopened.delivery("account-b-agent-a") == nil && reopened.draft("account-b-agent-a").text.isEmpty, "Delivery or draft crossed account scope")
        let newerDelivery = AgentSendSnapshot(scopeKey: delivery.scopeKey, composer: first, permissionMode: "read_only")
        try store.saveDelivery(newerDelivery)
        try store.confirm(delivery)
        check(store.delivery(delivery.scopeKey)?.requestID == "request-b", "Old ACK removed a newer in-flight delivery")
        try store.discard(newerDelivery)
        check(store.draft(delivery.scopeKey).requestID == "request-b" && store.delivery(delivery.scopeKey) == nil, "Stopping a retry erased the current draft")

        // Large attachment data is encoded once after a typing burst, on the worker.
        for index in 0..<80 {
            let value = AgentComposer(text: "typing-\(index)", images: [attachment], requestID: "typing-\(index)", allowInput: nil)
            store.schedule(value, key: "typing") { _ in fatalError("Unexpected persistence failure") }
        }
        try await Task.sleep(nanoseconds: 500_000_000)
        check(writes.count("draft-typing.json") == 1, "Typing re-encoded the attachment for each keystroke")
        check(store.draft("typing").text == "typing-79" && !writes.ranOnMain("draft-typing.json"), "Typing encoded/wrote on the main thread or saved stale text")
        let background = AgentComposer(text: "进入后台前的草稿", images: [], requestID: "background", allowInput: nil)
        store.schedule(first, key: "background") { _ in fatalError("Unexpected persistence failure") }
        try store.flush(background, key: "background")
        check(reopened.draft("background").text == background.text, "Background/scope flush did not persist immediately")
        try await Task.sleep(nanoseconds: 400_000_000)
        check(reopened.draft("background").text == background.text, "Old scheduled write overwrote a lifecycle flush")
        let broken = AgentComposerStore(root: root.appendingPathComponent("broken"), writer: { _, _ in throw FixtureFailure.disk })
        do { try broken.flush(first, key: "draft"); fatalError("Disk failure reported success") } catch is FixtureFailure {}
        check(broken.draft("draft").requestID == first.requestID, "A disk failure discarded the latest in-memory draft")

        // The retry policy is called by production AssistantModel.request.
        var attempts = 0, pauses: [UInt64] = []
        let result: String = try await AgentReadRetry.run(action: "history", valid: { true }, message: { $0.localizedDescription }, pause: { pauses.append($0) }) {
            attempts += 1
            if attempts < 3 { throw FixtureFailure.temporary }
            return "history"
        }
        check(result == "history" && attempts == 3 && pauses.count == 2 && pauses[1] > pauses[0], "Read retry did not use bounded increasing backoff")
        check(pauses[0] >= 280_000_000 && pauses[0] <= 420_000_000 && pauses[1] >= 720_000_000 && pauses[1] <= 1_080_000_000, "Retry jitter escaped its bounds")
        for action in ["send", "cancel", "resolve", "revoke_rule", "set_permissions", "image_begin", "image_chunk", "image_release", "global_create"] {
            var count = 0
            do {
                let _: String = try await AgentReadRetry.run(action: action, valid: { true }, message: { $0.localizedDescription }, pause: { _ in fatalError("Mutation was retried") }) {
                    count += 1; throw FixtureFailure.temporary
                }
                fatalError("Failed mutation reported success")
            } catch is FixtureFailure {}
            check(count == 1, "Mutation was automatically replayed")
        }
        for failure in [FixtureFailure.authentication, .validation] {
            var count = 0
            do {
                let _: Int = try await AgentReadRetry.run(action: "state", valid: { true }, message: { $0.localizedDescription }, pause: { _ in fatalError("Permanent rejection was retried") }) {
                    count += 1; throw failure
                }
                fatalError("Rejected read reported success")
            } catch is FixtureFailure {}
            check(count == 1, "Authentication/business failure retried")
        }
        var exhausted = 0
        do {
            let _: Int = try await AgentReadRetry.run(action: "permissions", valid: { true }, message: { $0.localizedDescription }, pause: { _ in }) {
                exhausted += 1; throw FixtureFailure.temporary
            }
            fatalError("Exhausted read reported success")
        } catch is FixtureFailure {}
        check(exhausted == 3, "Read retry has no fixed attempt limit")
        var valid = true, staleAttempts = 0
        do {
            let _: Int = try await AgentReadRetry.run(action: "state", valid: { valid }, message: { $0.localizedDescription }, pause: { _ in valid = false }) {
                staleAttempts += 1; throw FixtureFailure.temporary
            }
            fatalError("Connection change reported success")
        } catch is CancellationError {}
        check(staleAttempts == 1, "A queued retry crossed the connection epoch")
        var waiting: CheckedContinuation<Void, Never>?, cancelledAttempts = 0
        let cancelled = Task {
            try await AgentReadRetry.run(action: "state", valid: { true }, message: { $0.localizedDescription }, pause: { _ in
                await withCheckedContinuation { waiting = $0 }
            }) { () -> Int in cancelledAttempts += 1; throw FixtureFailure.temporary }
        }
        while waiting == nil { await Task.yield() }
        cancelled.cancel(); waiting?.resume()
        do { _ = try await cancelled.value; fatalError("Cancelled retry reported success") } catch is CancellationError {}
        check(cancelledAttempts == 1, "Cancelled task dispatched another request")

        let interrupted: [String: Any] = ["source": "model_interrupted", "interrupted": true, "state": "interrupted"]
        check(AgentMessagePresentation.interrupted(kind: "interaction", value: interrupted), "Runtime interrupted event was presented as a tool action")
        check(!AgentMessagePresentation.interrupted(kind: "assistant", value: ["partial": true]), "Pagination partial marker became an interrupted reply")
        check(!AgentMessagePresentation.interrupted(kind: "interaction", value: ["source": "tool", "interrupted": true, "state": "interrupted"]), "Tool event was mislabeled as a model interruption")
        check(AgentMessagePresentation.modelRetryStatus(["attempt": 2, "max_attempts": 3, "delay_ms": 10000, "reason": "temporary backend failure"]) == "AI 正在重试（2/3）", "Runtime retry was not exposed in the existing status text")
        check(AgentMessagePresentation.modelRetryStatus(nil) == nil && AgentMessagePresentation.modelRetryStatus(NSNull()) == nil, "Absent retry left a stale retry indicator")
        check(AgentMessagePresentation.modelRetryStatus(["attempt": 4, "max_attempts": 3]) == nil && AgentMessagePresentation.modelRetryStatus(["attempt": 1, "max_attempts": 3]) == nil, "Invalid retry metadata was shown as an active retry")

        var live = AgentLiveReplyState()
        live.receive(["run_id": "run-a", "root_user_message_id": "root-a", "live_text": "visible reply"])
        live.receive(["state": "completed", "last_run": ["run_id": "run-a", "root_user_message_id": "root-a"]])
        check(live.text == "visible reply" && live.awaitingHistory, "Completion/desktop restart removed the live reply before history arrived")
        live.reconcile(history: [])
        check(live.text == "visible reply", "Empty/delayed history erased the last visible reply")
        live.reconcile(history: [["kind": "assistant", "root_user_message_id": "root-a", "value": ["run_id": "run-b", "text": "visible reply from another run"]]])
        live.reconcile(history: [["kind": "assistant", "root_user_message_id": "root-b", "value": ["run_id": "run-a", "text": "visible reply from another root"]]])
        live.reconcile(history: [["kind": "assistant", "root_user_message_id": "root-a", "value": ["run_id": "run-a", "text": "visible reply preview", "partial": true]]])
        live.reconcile(history: [["kind": "interaction", "root_user_message_id": "root-a", "value": ["text": "other tool narration"]]])
        check(live.text == "visible reply", "Unrelated, partial or earlier reply cleared the live handoff")
        live.reconcile(history: [["kind": "assistant", "root_user_message_id": "root-a", "value": ["run_id": "run-a", "text": "visible reply and the remaining durable text"]]])
        check(live.text.isEmpty && !live.awaitingHistory, "Complete matching durable reply did not settle the handoff")
        live.receive(["run_id": "run-a", "root_user_message_id": "root-a", "live_text": "interrupted reply"])
        live.receive(["run_id": "run-a", "root_user_message_id": "root-a", "live_text": ""])
        live.reconcile(history: [["kind": "interaction", "root_user_message_id": "root-a", "value": ["run_id": "run-a", "source": "model_interrupted", "interrupted": true, "state": "interrupted", "text": "interrupted reply"]]])
        check(live.text.isEmpty, "Interrupted durable reply could not replace its live text")
        live.receive(["run_id": "run-a", "root_user_message_id": "root-a", "live_text": "old longer reply"])
        live.receive(["run_id": "run-b", "root_user_message_id": "root-b", "live_text": "new"])
        check(live.text == "new" && !live.awaitingHistory, "New run inherited the old longer reply")
        live.receive(["run_id": "run-c", "root_user_message_id": "root-c", "live_text": ""])
        check(live.text.isEmpty, "New run's empty state retained a previous run's live text")
        live.receive(["run_id": "run-c", "root_user_message_id": "root-c", "live_text": "scope-local reply"])
        live = AgentLiveReplyState()
        check(live.text.isEmpty && !live.awaitingHistory, "Scope/generation reset retained old live text")
        for (prefix, complete) in [("e", "e\u{301} reply"), ("👩", "👩‍🔬 reply")] {
            live.receive(["run_id": "unicode", "root_user_message_id": "unicode-root", "live_text": prefix])
            live.receive(["run_id": "unicode", "root_user_message_id": "unicode-root", "live_text": ""])
            live.reconcile(history: [["kind": "assistant", "root_user_message_id": "unicode-root", "value": ["run_id": "unicode", "text": complete]]])
            check(live.text.isEmpty, "UTF-8 stream prefix ending inside a grapheme did not hand off to its durable reply")
        }
        var historyGeneration = AgentHistoryGenerationFence()
        historyGeneration.bind("scope-a")
        var oldHistory: CheckedContinuation<Int64, Never>?
        let delayedHistory = Task { () -> Bool in
            let oldGeneration = await withCheckedContinuation { oldHistory = $0 }
            return historyGeneration.accept(oldGeneration, scopeKey: "scope-a")
        }
        while oldHistory == nil { await Task.yield() }
        check(historyGeneration.accept(2, scopeKey: "scope-a"), "Initial state did not establish the known generation")
        oldHistory?.resume(returning: 1)
        let acceptedStale = await delayedHistory.value
        check(!acceptedStale && historyGeneration.minimum == 2, "Late history from before deletion rolled back the known generation")
        historyGeneration.bind("scope-a")
        check(!historyGeneration.accept(1, scopeKey: "scope-a"), "Same-scope view reset or offline fallback accepted older history")
        check(historyGeneration.accept(2, scopeKey: "scope-a") && historyGeneration.accept(3, scopeKey: "scope-a"), "Current/new generation was rejected")
        check(!historyGeneration.accept(2, scopeKey: "scope-a"), "A stale state response rolled generation backwards")
        historyGeneration.bind("scope-b")
        check(historyGeneration.accept(0, scopeKey: "scope-b") && !historyGeneration.accept(99, scopeKey: "scope-a"), "History generation crossed Agent scope")
        print("PASS: immutable persisted retries, scoped/queued draft ACK isolation, background flush, debounce worker I/O, bounded read retries with cancellation and epoch/auth fences, interrupted event/retry status presentation, run/root-isolated live/history handoff and generation fencing")
    }
}
