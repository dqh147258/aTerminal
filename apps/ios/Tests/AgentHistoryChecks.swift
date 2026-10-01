import Foundation
import SQLite3

@main struct AgentHistoryChecks {
    static func require(_ value: Bool, _ message: String) throws {
        if !value { throw NSError(domain: "HistoryChecks", code: 1, userInfo: [NSLocalizedDescriptionKey: message]) }
    }
    static func bridge(_ arguments: [String]) throws -> String {
        let process = Process(); process.executableURL = URL(fileURLWithPath: CommandLine.arguments[1]); process.arguments = arguments
        let output = Pipe(); let errors = Pipe(); process.standardOutput = output; process.standardError = errors
        try process.run()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        let error = errors.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { throw NSError(domain: String(decoding: error, as: UTF8.self), code: Int(process.terminationStatus)) }
        return String(decoding: data, as: UTF8.self)
    }
    static func encode(_ value: [String: Any]) throws -> String { String(decoding: try JSONSerialization.data(withJSONObject: value), as: UTF8.self) }
    static func writer(_ path: String, root: URL) -> AgentHistoryCache.PageWriter {
        { scope, cursor, page in
            let input = root.appendingPathComponent(UUID().uuidString + ".json")
            try Data(page.utf8).write(to: input); defer { try? FileManager.default.removeItem(at: input) }
            _ = try bridge([path, "put", scope, cursor ?? "", input.path])
        }
    }
    static func reader(_ path: String) -> AgentHistoryCache.PageReader {
        { scope, cursor in let value = try bridge([path, "get", scope, cursor ?? ""]); return value.isEmpty ? nil : value }
    }
    @MainActor static func history(_ root: URL) async throws {
        let path = root.appendingPathComponent("history.sqlite").path; _ = try bridge([path, "open"])
        let messages = try AgentMessageCache(path: path)
        let write = writer(path, root: root); let read = reader(path)
        let cache = AgentHistoryCache(write: write, read: read, messages: messages)
        let longText = String(repeating: "x", count: 22000) + "末尾正文检索-token"
        let original: [String: Any] = ["text": longText]
        let body = Data(try encode(original).utf8)
        let items: [[String: Any]] = (0..<50).map { ["id": "message-\($0)", "kind": "assistant", "value": ["text": String(longText.prefix(8000)), "partial": true, "record_id": "record-\($0)"]] }
        let page: [String: Any] = ["generation": 1, "items": items, "has_more": false]
        var reads = 0
        let online = try await cache.load(scope: "alice-desktop-session", cursor: nil, fetch: { page }, record: { _, cursor in
            reads += 1
            let start = Int(cursor ?? "0")!
            var end = min(start + 12288, body.count)
            while end < body.count && body[end] & 0xC0 == 0x80 { end -= 1 }
            return ["kind": "history_event", "body": String(decoding: body[start..<end], as: UTF8.self), "cursor": end < body.count ? String(end) as Any : NSNull()]
        }, valid: { true })
        let raw = try read("alice-desktop-session", nil)!
        try require(raw.utf8.count < 1024 * 1024 && online.warning.isEmpty && !online.offline, "raw page must satisfy the actual Rust bound")
        try require(try encode(online.value).utf8.count > 1024 * 1024 && reads == 100, "50 valid long records must be expanded over the old failing limit")
        let directValue = try messages.value(scope: "alice-desktop-session", generation: 1, record: "record-49")
        try require(directValue?["text"] as? String == longText, "full record must be readable directly after online load")
        let rawItems = (try JSONSerialization.jsonObject(with: Data(raw.utf8)) as! [String: Any])["items"] as! [[String: Any]]
        try require((rawItems[0]["value"] as! [String: Any])["partial"] as? Bool == true, "expanded values must never replace wire pages")
        // New instances model process restart, so an in-memory hit cannot hide a disk failure.
        let restarted = AgentHistoryCache(write: write, read: read, messages: try AgentMessageCache(path: path))
        let offline = try await restarted.load(scope: "alice-desktop-session", cursor: nil, fetch: { throw URLError(.notConnectedToInternet) }, record: { _, _ in fatalError("offline history attempted an RPC") }, valid: { true })
        let restored = offline.value["items"] as! [[String: Any]]
        try require(offline.offline && offline.warning.isEmpty && restored.count == 50 && (restored[49]["value"] as! [String: Any])["text"] as? String == longText, "restart must retain complete messages offline: offline=\(offline.offline), warning=\(offline.warning), count=\(restored.count), lastBytes=\(((restored.last?["value"] as? [String: Any])?["text"] as? String)?.utf8.count ?? 0)")
        try require(AgentCacheSearch.matches(path: path, scopes: ["alice-desktop-session"], query: "末尾正文检索-token") == ["alice-desktop-session"], "search must include text beyond the wire excerpt")
        try require(AgentCacheSearch.matches(path: path, scopes: ["other-account"], query: "末尾正文检索-token").isEmpty, "full records must respect account/session scope")
        _ = try bridge([path, "reconcile", "alice-desktop-session", "2"])
        try require(messages.value(scope: "alice-desktop-session", generation: 1, record: "record-0") == nil, "deleted generation must not expose old full records")
        try require(AgentCacheSearch.matches(path: path, scopes: ["alice-desktop-session"], query: "末尾正文检索-token").isEmpty, "deleted generations must leave search")
        print("PASS: actual Rust page bound, full records after restart, offline no-RPC, tail search, scope and generation isolation")
    }
    @MainActor static func failures(_ root: URL) async throws {
        let path = root.appendingPathComponent("failures.sqlite").path; _ = try bridge([path, "open"])
        let page: [String: Any] = ["generation": 1, "items": [["id": "a", "kind": "assistant", "value": ["text": "online response"]]], "has_more": false]
        let unavailable = AgentHistoryCache(write: { _, _, _ in throw CocoaError(.fileWriteNoPermission) }, read: { _, _ in nil }, messages: nil)
        let result = try await unavailable.load(scope: "a", cursor: nil, fetch: { page }, record: { _, _ in [:] }, valid: { true })
        try require(!result.warning.isEmpty && !result.offline && (result.value["items"] as? [[String: Any]])?.count == 1, "cache failure must be visible without discarding online content")
        var active = true; var writes = 0
        let cancelled = AgentHistoryCache(write: { _, _, _ in writes += 1 }, read: { _, _ in nil }, messages: nil)
        do {
            _ = try await cancelled.load(scope: "a", cursor: nil, fetch: { active = false; return page }, record: { _, _ in [:] }, valid: { active })
            throw NSError(domain: "late response accepted", code: 1)
        } catch is CancellationError {}
        try require(writes == 0, "late page must not write to a changed identity")
        let write = writer(path, root: root); try write("a", nil, encode(page))
        let messages = try AgentMessageCache(path: path, maximumBytes: 1024, maximumCount: 2)
        for id in ["one", "two", "three"] { try messages.store(["text": String(repeating: id, count: 100)], scope: "a", generation: 1, record: id) }
        try require(messages.value(scope: "a", generation: 1, record: "one") == nil && messages.value(scope: "a", generation: 1, record: "three") != nil, "full records must have bounded LRU retention")
        print("PASS: cache-write feedback, late-response write barrier and full-record retention budget")
    }
    static func directories(_ root: URL) throws {
        let identity = ChatIdentity(server: "https://one.invalid", account: "alice")
        let other = ChatIdentity(server: "https://one.invalid", account: "bob")
        let scope = ChatScope(identity: identity, device: "desktop", session: "terminal")
        var archive = ChatArchive(scope: scope, title: "alpha", deviceName: "Desktop", cwd: "/review/projects/alpha")
        archive.messages = [ChatMessage(role: "user", content: "retained content")]
        archive.pendingID = "request"
        let store = ChatStore(root: root.appendingPathComponent("directories"))
        try store.save(archive)
        let restored = try store.load(identity)[0]
        try require(restored.workingDirectory == "/review/projects/alpha" && restored.messages == archive.messages && restored.pendingID == "request", "closed/restarted history must retain full cwd and existing content")
        try require(try store.load(other).isEmpty, "cwd metadata must not cross accounts")
        var legacy = try JSONSerialization.jsonObject(with: JSONEncoder().encode(archive)) as! [String: Any]; legacy.removeValue(forKey: "cwd")
        var upgraded = try JSONDecoder().decode(ChatArchive.self, from: JSONSerialization.data(withJSONObject: legacy))
        try require(upgraded.cwd == nil && upgraded.workingDirectory == "alpha", "old histories without cwd must still decode")
        try require(upgraded.updateDirectory("/review/new/alpha", title: "alpha", deviceName: "Desktop") && upgraded.messages == archive.messages && upgraded.pendingID == "request", "live metadata backfill must preserve history and drafts")
        try require(!upgraded.updateDirectory("", title: "unknown", deviceName: "Desktop") && upgraded.workingDirectory == "/review/new/alpha", "missing metadata must not erase a known directory")
        print("PASS: full cwd persistence, old JSON compatibility, scoped metadata backfill and existing history preservation")
    }
    static func main() async throws {
        let root = URL(fileURLWithPath: CommandLine.arguments[2])
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try await history(root); try await failures(root); try directories(root)
    }
}
