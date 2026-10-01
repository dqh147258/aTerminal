import Foundation
import SQLite3

/// Read the existing bounded AgentCache pages, including pages whose first-page cursor was evicted.
/// No legacy table, new index, network request, or credential store is involved.
enum AgentCacheSearch {
    static func matches(path: String, scopes: Set<String>, query: String, cancelled: () -> Bool = { Task.isCancelled }) throws -> Set<String> {
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty, !scopes.isEmpty else { return [] }
        var database: OpaquePointer?
        guard sqlite3_open_v2(path, &database, SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX, nil) == SQLITE_OK else {
            if let database { sqlite3_close(database) }
            throw SearchFailure.unavailable
        }
        defer { sqlite3_close(database) }
        sqlite3_busy_timeout(database, 100)
        var statement: OpaquePointer?
        let sql = "SELECT p.body FROM pages p JOIN cache_generations g ON p.scope=g.scope AND p.generation=g.generation WHERE p.scope=?1 ORDER BY p.touched DESC LIMIT 3"
        guard sqlite3_prepare_v2(database, sql, -1, &statement, nil) == SQLITE_OK else { throw SearchFailure.unavailable }
        defer { sqlite3_finalize(statement) }
        let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
        var matches = Set<String>()
        for scope in scopes.sorted() {
            if cancelled() { throw CancellationError() }
            sqlite3_reset(statement); sqlite3_clear_bindings(statement)
            guard sqlite3_bind_text(statement, 1, scope, -1, transient) == SQLITE_OK else { throw SearchFailure.unavailable }
            while true {
                if cancelled() { throw CancellationError() }
                let status = sqlite3_step(statement)
                if status == SQLITE_DONE { break }
                guard status == SQLITE_ROW, let bytes = sqlite3_column_text(statement, 0) else { throw SearchFailure.unavailable }
                let count = Int(sqlite3_column_bytes(statement, 0))
                guard count <= 1024 * 1024 else { throw SearchFailure.unavailable }
                let data = Data(bytes: bytes, count: count)
                guard let page = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
                if (page["items"] as? [[String: Any]] ?? []).contains(where: { body($0).localizedCaseInsensitiveContains(query) }) {
                    matches.insert(scope); break
                }
            }
        }
        return matches
    }
    private static func body(_ item: [String: Any]) -> String {
        guard ["user", "assistant", "interaction"].contains(item["kind"] as? String ?? ""), let value = item["value"] as? [String: Any] else { return "" }
        // Match visible message text, not IDs, tool arguments, configuration or other JSON metadata.
        for key in ["message", "text", "summary"] { if let text = value[key] as? String, !text.isEmpty { return text } }
        return (value["updates"] as? [[String: Any]] ?? []).reversed().compactMap { $0["summary"] as? String }.first(where: { !$0.isEmpty }) ?? ""
    }
    enum SearchFailure: LocalizedError {
        case unavailable
        var errorDescription: String? { "正文缓存搜索暂不可用" }
    }
}

/// Production debounce and identity/query fence, independently testable with a controlled search closure.
@MainActor final class AgentSearchSession {
    struct Request: Equatable {
        let identity: String
        let query: String
        let scopes: Set<String>
    }
    typealias Search = (Set<String>, String) async throws -> Set<String>
    private(set) var request: Request?
    private(set) var matches = Set<String>()
    private(set) var busy = false
    private(set) var error = ""
    var changed: (() -> Void)?
    private var serial = 0
    private var task: Task<Void, Never>?
    private let delay: UInt64
    private let search: Search
    init(delayNanoseconds: UInt64 = 250_000_000, search: @escaping Search) { delay = delayNanoseconds; self.search = search }
    func update(identity: String, query: String, scopes: Set<String>, refresh: Bool = false) {
        let next = Request(identity: identity, query: query.trimmingCharacters(in: .whitespacesAndNewlines), scopes: scopes)
        guard refresh || request != next else { return }
        task?.cancel(); serial += 1; let ticket = serial
        request = next; matches = []; error = ""; busy = !next.query.isEmpty && !scopes.isEmpty; changed?()
        guard busy else { task = nil; return }
        task = Task {
            do {
                try await Task.sleep(nanoseconds: delay)
                try Task.checkCancellation()
                let result = try await search(scopes, next.query)
                guard !Task.isCancelled, serial == ticket, request == next else { return }
                matches = result.intersection(scopes); busy = false; changed?()
            } catch {
                guard !Task.isCancelled, serial == ticket, request == next else { return }
                matches = []; busy = false; self.error = "正文缓存搜索暂不可用"; changed?()
            }
        }
    }
    func contains(scope: String, identity: String, query: String) -> Bool {
        request?.identity == identity && request?.query == query.trimmingCharacters(in: .whitespacesAndNewlines) && matches.contains(scope)
    }
    func cancel() {
        task?.cancel(); task = nil; serial += 1; request = nil; matches = []; busy = false; error = ""; changed?()
    }
}
