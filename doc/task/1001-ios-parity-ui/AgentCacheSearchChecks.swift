// Host-only checks; links the actual AgentCacheSearch.swift production implementation.
import Foundation
import SQLite3

@main struct AgentCacheSearchChecks {
    static func require(_ condition: Bool, _ message: String) throws {
        if !condition { throw NSError(domain: "SearchChecks", code: 1, userInfo: [NSLocalizedDescriptionKey: message]) }
    }
    static func database() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let path = root.appendingPathComponent("cache.sqlite").path
        var db: OpaquePointer?; try require(sqlite3_open(path, &db) == SQLITE_OK, "open")
        defer { sqlite3_close(db) }
        try require(sqlite3_exec(db, "CREATE TABLE pages(scope TEXT,cursor TEXT,generation INTEGER,body TEXT,touched INTEGER); CREATE TABLE cache_generations(scope TEXT,generation INTEGER); CREATE TABLE legacy(scope TEXT,body TEXT); INSERT INTO cache_generations VALUES('alice-closed',2),('bob',1); INSERT INTO legacy VALUES('alice-closed','legacy-only-token');", nil, nil, nil) == SQLITE_OK, "schema")
        func insert(_ scope: String, _ cursor: String, _ generation: Int32, _ item: [String: Any]) throws {
            var statement: OpaquePointer?
            try require(sqlite3_prepare_v2(db, "INSERT INTO pages VALUES(?1,?2,?3,?4,1)", -1, &statement, nil) == SQLITE_OK, "prepare")
            defer { sqlite3_finalize(statement) }
            let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
            sqlite3_bind_text(statement, 1, scope, -1, transient); sqlite3_bind_text(statement, 2, cursor, -1, transient); sqlite3_bind_int(statement, 3, generation)
            let body = String(decoding: try JSONSerialization.data(withJSONObject: ["generation": generation, "items": [item]]), as: UTF8.self)
            sqlite3_bind_text(statement, 4, body, -1, transient)
            try require(sqlite3_step(statement) == SQLITE_DONE, "insert")
        }
        try insert("alice-closed", "older-page-without-first", 2, ["id": "metadata-only-token", "kind": "assistant", "value": ["text": "星河缓存检索 Cedar-Body-Only-731"]])
        try insert("alice-closed", "another-page", 2, ["kind": "interaction", "value": ["text": "", "updates": [["summary": "Visible tool summary"]]]])
        try insert("alice-closed", "stale", 1, ["kind": "user", "value": ["message": "stale-only-token"]])
        try insert("bob", "", 1, ["kind": "assistant", "value": ["text": "foreign-only-token"]])
        let allowed: Set<String> = ["alice-closed"]
        try require(AgentCacheSearch.matches(path: path, scopes: allowed, query: "CEDAR-body") == allowed, "non-first cached page/case folding")
        try require(AgentCacheSearch.matches(path: path, scopes: allowed, query: "星河缓存检索") == allowed, "Unicode body")
        try require(AgentCacheSearch.matches(path: path, scopes: allowed, query: "tool summary") == allowed, "visible interaction summary")
        for query in ["metadata-only-token", "stale-only-token", "foreign-only-token", "legacy-only-token"] {
            try require(AgentCacheSearch.matches(path: path, scopes: allowed, query: query).isEmpty, "scope/generation/body-only/legacy fence")
        }
        do { _ = try AgentCacheSearch.matches(path: path, scopes: allowed, query: "cedar", cancelled: { true }); throw NSError(domain: "not-cancelled", code: 1) }
        catch is CancellationError {}
        let corrupt = root.appendingPathComponent("corrupt.sqlite")
        try Data("invalid database".utf8).write(to: corrupt)
        do { _ = try AgentCacheSearch.matches(path: corrupt.path, scopes: allowed, query: "cedar"); throw NSError(domain: "corrupt-accepted", code: 1) }
        catch AgentCacheSearch.SearchFailure.unavailable {}
        print("PASS: current scope, generation, arbitrary cached cursor, Unicode/case folding, visible body only, no legacy, cancellation, corrupt DB rejection")
    }
    @MainActor static func asynchronous() async throws {
        var calls: [(Set<String>, String)] = []
        var replies: [CheckedContinuation<Set<String>, Error>] = []
        let search = AgentSearchSession(delayNanoseconds: 20_000_000) { scopes, query in
            calls.append((scopes, query))
            return try await withCheckedThrowingContinuation { replies.append($0) }
        }
        func next(_ count: Int) async throws {
            for _ in 0..<1000 { if calls.count >= count { return }; try await Task.sleep(nanoseconds: 1_000_000) }
            throw NSError(domain: "search-timeout", code: 1)
        }
        search.update(identity: "alice", query: "c", scopes: ["a"])
        search.update(identity: "alice", query: "cedar", scopes: ["a"])
        try await next(1); try require(calls.count == 1 && calls[0].1 == "cedar", "debounce")
        search.update(identity: "alice", query: "new", scopes: ["a"])
        try await next(2); replies[0].resume(returning: ["a"])
        await Task.yield(); try require(search.matches.isEmpty && search.busy, "late query")
        search.update(identity: "bob", query: "new", scopes: ["b"])
        try await next(3); replies[1].resume(throwing: NSError(domain: "old-error", code: 1))
        await Task.yield(); try require(search.error.isEmpty && search.matches.isEmpty, "late identity failure")
        replies[2].resume(returning: ["b", "not-allowed"])
        for _ in 0..<100 where search.busy { try await Task.sleep(nanoseconds: 1_000_000) }
        try require(search.contains(scope: "b", identity: "bob", query: "new") && !search.contains(scope: "b", identity: "alice", query: "new") && search.matches == ["b"], "identity/query/scopes fence")
        search.update(identity: "bob", query: "new", scopes: ["b"], refresh: true)
        try await next(4); search.cancel(); replies[3].resume(returning: ["b"])
        await Task.yield(); try require(search.matches.isEmpty && !search.busy, "closed search")
        search.update(identity: "bob", query: "broken-cache", scopes: ["b"])
        try await next(5); replies[4].resume(throwing: AgentCacheSearch.SearchFailure.unavailable)
        for _ in 0..<100 where search.busy { try await Task.sleep(nanoseconds: 1_000_000) }
        try require(!search.busy && search.matches.isEmpty && search.error == "正文缓存搜索暂不可用", "active failure releases UI state")
        print("PASS: debounce, query/identity late success+failure fences, scope intersection, refresh, cancellation, active cache failure")
    }
    static func main() async throws { try database(); try await asynchronous() }
}
