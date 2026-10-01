import Foundation
import SQLite3

/// Full records are separate from the Rust cache's bounded wire pages.
final class AgentMessageCache {
    static let maximumMessageBytes = 4 * 1024 * 1024
    private var database: OpaquePointer?
    private let lock = NSLock()
    private let maximumBytes: Int
    private let maximumCount: Int
    private static let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

    init(path: String, maximumBytes: Int = 64 * 1024 * 1024, maximumCount: Int = 256) throws {
        guard maximumBytes > 0, maximumCount > 0 else { throw Failure.unavailable }
        self.maximumBytes = maximumBytes; self.maximumCount = maximumCount
        var opened: OpaquePointer?
        guard sqlite3_open_v2(path, &opened, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_NOMUTEX, nil) == SQLITE_OK else {
            if let opened { sqlite3_close(opened) }; throw Failure.unavailable
        }
        database = opened
        sqlite3_busy_timeout(database, 1000)
        try connection { db in
            try Self.execute(db, "CREATE TABLE IF NOT EXISTS message_values(scope TEXT NOT NULL,generation INTEGER NOT NULL,record TEXT NOT NULL,body TEXT NOT NULL,bytes INTEGER NOT NULL,touched INTEGER NOT NULL,PRIMARY KEY(scope,generation,record))")
        }
    }
    deinit { sqlite3_close(database) }

    func value(scope: String, generation: Int64, record: String) throws -> [String: Any]? {
        try connection { try Self.value(in: $0, scope: scope, generation: generation, record: record) }
    }

    static func value(in db: OpaquePointer?, scope: String, generation: Int64, record: String) throws -> [String: Any]? {
        var table: OpaquePointer?
        guard sqlite3_prepare_v2(db, "SELECT 1 FROM sqlite_master WHERE type='table' AND name='message_values'", -1, &table, nil) == SQLITE_OK else { throw Failure.unavailable }
        let tableStatus = sqlite3_step(table)
        sqlite3_finalize(table)
        guard tableStatus == SQLITE_ROW || tableStatus == SQLITE_DONE else { throw Failure.unavailable }
        let exists = tableStatus == SQLITE_ROW
        guard exists else { return nil } // Older app caches have only raw pages.
        var statement: OpaquePointer?
        let sql = "SELECT m.body FROM message_values m JOIN cache_generations g ON m.scope=g.scope AND m.generation=g.generation WHERE m.scope=?1 AND m.generation=?2 AND m.record=?3"
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else { throw Failure.unavailable }
        defer { sqlite3_finalize(statement) }
        sqlite3_bind_text(statement, 1, scope, -1, transient)
        sqlite3_bind_int64(statement, 2, generation)
        sqlite3_bind_text(statement, 3, record, -1, transient)
        let status = sqlite3_step(statement)
        if status == SQLITE_DONE { return nil }
        guard status == SQLITE_ROW, let bytes = sqlite3_column_text(statement, 0) else { throw Failure.unavailable }
        let count = Int(sqlite3_column_bytes(statement, 0))
        guard count <= maximumMessageBytes else { throw Failure.tooLarge }
        guard let value = try JSONSerialization.jsonObject(with: Data(bytes: bytes, count: count)) as? [String: Any] else { throw Failure.unavailable }
        return value
    }

    func store(_ value: [String: Any], scope: String, generation: Int64, record: String) throws {
        let data = try JSONSerialization.data(withJSONObject: value)
        guard data.count <= min(Self.maximumMessageBytes, maximumBytes) else { throw Failure.tooLarge }
        try connection { db in
            try Self.execute(db, "BEGIN IMMEDIATE")
            do {
                // Deleted generations and evicted scopes must not keep full records alive.
                try Self.execute(db, "DELETE FROM message_values WHERE NOT EXISTS(SELECT 1 FROM pages p JOIN cache_generations g ON p.scope=g.scope AND p.generation=g.generation WHERE p.scope=message_values.scope AND p.generation=message_values.generation)")
                var statement: OpaquePointer?
                let sql = "INSERT OR REPLACE INTO message_values SELECT ?1,?2,?3,?4,?5,(SELECT COALESCE(MAX(touched),0)+1 FROM message_values) WHERE EXISTS(SELECT 1 FROM pages p JOIN cache_generations g ON p.scope=g.scope AND p.generation=g.generation WHERE p.scope=?1 AND p.generation=?2)"
                guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else { throw Failure.unavailable }
                defer { sqlite3_finalize(statement) }
                sqlite3_bind_text(statement, 1, scope, -1, Self.transient)
                sqlite3_bind_int64(statement, 2, generation)
                sqlite3_bind_text(statement, 3, record, -1, Self.transient)
                sqlite3_bind_text(statement, 4, String(decoding: data, as: UTF8.self), -1, Self.transient)
                sqlite3_bind_int64(statement, 5, Int64(data.count))
                guard sqlite3_step(statement) == SQLITE_DONE, sqlite3_changes(db) == 1 else { throw Failure.unavailable }
                try Self.execute(db, "DELETE FROM message_values WHERE rowid NOT IN(SELECT rowid FROM message_values ORDER BY touched DESC LIMIT \(maximumCount))")
                while try Self.totalBytes(db) > maximumBytes {
                    try Self.execute(db, "DELETE FROM message_values WHERE rowid=(SELECT rowid FROM message_values ORDER BY touched LIMIT 1)")
                }
                try Self.execute(db, "COMMIT")
            } catch { try? Self.execute(db, "ROLLBACK"); throw error }
        }
    }

    private func connection<T>(_ operation: (OpaquePointer?) throws -> T) throws -> T {
        // Keep a WAL-capable connection alive while the Rust page cache shares this file.
        // Each operation is serialized; background searches use a separate read-only handle.
        lock.lock(); defer { lock.unlock() }
        return try operation(database)
    }
    private static func execute(_ db: OpaquePointer?, _ sql: String) throws {
        guard sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK else { throw Failure.unavailable }
    }
    private static func totalBytes(_ db: OpaquePointer?) throws -> Int {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, "SELECT COALESCE(SUM(bytes),0) FROM message_values", -1, &statement, nil) == SQLITE_OK else { throw Failure.unavailable }
        defer { sqlite3_finalize(statement) }
        guard sqlite3_step(statement) == SQLITE_ROW else { throw Failure.unavailable }
        return Int(sqlite3_column_int64(statement, 0))
    }
    enum Failure: Error { case unavailable, tooLarge }
}

/// The actual online/offline history pipeline, without UI or transport dependencies.
@MainActor final class AgentHistoryCache {
    typealias Object = [String: Any]
    typealias PageWriter = (String, String?, String) throws -> Void
    typealias PageReader = (String, String?) throws -> String?
    typealias RecordReader = (String, String?) async throws -> Object
    struct Page { let value: Object; let offline: Bool; let warning: String }
    private let write: PageWriter
    private let read: PageReader
    private let messages: AgentMessageCache?
    private let memory = NSCache<NSString, NSDictionary>()

    init(write: @escaping PageWriter, read: @escaping PageReader, messages: AgentMessageCache?) {
        self.write = write; self.read = read; self.messages = messages
        memory.totalCostLimit = 8 * 1024 * 1024; memory.countLimit = 100
    }

    func load(scope: String, cursor: String?, fetch: () async throws -> Object, record: RecordReader,
              valid: () -> Bool) async throws -> Page {
        var page: Object
        var offline = false
        var warning = ""
        do {
            page = try await fetch()
            guard valid() else { throw CancellationError() }
            // Cache the unexpanded wire response before fetching any full message.
            do { try write(scope, cursor, String(decoding: JSONSerialization.data(withJSONObject: page), as: UTF8.self)) }
            catch { warning = "离线历史保存失败，当前内容仍可在线查看" }
        } catch {
            guard valid() else { throw CancellationError() }
            guard let cached = try read(scope, cursor), let object = try JSONSerialization.jsonObject(with: Data(cached.utf8)) as? Object else { throw error }
            page = object; offline = true
        }
        guard let generation = (page["generation"] as? NSNumber)?.int64Value else { throw AgentMessageCache.Failure.unavailable }
        var expanded: [Object] = []
        for item in page["items"] as? [Object] ?? [] {
            guard valid() else { throw CancellationError() }
            guard let value = item["value"] as? Object, value["partial"] as? Bool == true,
                  let id = value["record_id"] as? String else { expanded.append(item); continue }
            let key = "\(scope):\(generation):\(id)" as NSString
            var full = memory.object(forKey: key) as? Object
            if full == nil { full = try? messages?.value(scope: scope, generation: generation, record: id) }
            if full == nil && !offline {
                do {
                    full = try await original(id, record: record, valid: valid)
                    if let full {
                        do {
                            guard let messages else { throw AgentMessageCache.Failure.unavailable }
                            try messages.store(full, scope: scope, generation: generation, record: id)
                        } catch { warning = "部分消息原文未能保存到离线历史" }
                    }
                } catch {
                    guard valid() else { throw CancellationError() }
                    warning = "部分消息原文暂不可用，恢复连接后可重试"
                }
            }
            if let full {
                if let data = try? JSONSerialization.data(withJSONObject: full) { memory.setObject(full as NSDictionary, forKey: key, cost: data.count) }
                var complete = item; complete["value"] = full; expanded.append(complete)
            } else {
                expanded.append(item)
                if warning.isEmpty { warning = "部分消息原文未缓存，连接 Desktop 后可读取完整内容" }
            }
        }
        guard valid() else { throw CancellationError() }
        page["items"] = expanded
        return Page(value: page, offline: offline, warning: warning)
    }

    private func original(_ id: String, record: RecordReader, valid: () -> Bool) async throws -> Object {
        var text = ""; var cursor: String?
        repeat {
            guard valid() else { throw CancellationError() }
            let part = try await record(id, cursor)
            guard valid() else { throw CancellationError() }
            guard part["kind"] as? String == "history_event" else { throw AgentMessageCache.Failure.unavailable }
            text += part["body"] as? String ?? ""
            guard text.utf8.count <= AgentMessageCache.maximumMessageBytes else { throw AgentMessageCache.Failure.tooLarge }
            cursor = part["cursor"] as? String
        } while cursor != nil
        guard let value = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? Object else { throw AgentMessageCache.Failure.unavailable }
        return value
    }
}
