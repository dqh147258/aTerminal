import Foundation

struct AgentAttachment: Codable, Identifiable {
    var id = UUID().uuidString
    let mime: String
    let data: Data
}

/// Keep the existing draft format readable by older installations.
struct AgentComposer: Codable {
    var text = ""
    var images: [AgentAttachment] = []
    var requestID = UUID().uuidString
    var allowInput: Bool?
}

/// Retries use the original message and permission mode, independently of edits
/// to the next draft. The scope includes the account, server, device and Agent.
struct AgentSendSnapshot: Codable {
    let scopeKey: String
    let composer: AgentComposer
    let permissionMode: String
    var requestID: String { composer.requestID }
}

/// The worker owns both pending writes and on-disk state. Confirming a send must
/// inspect queued drafts as well as the file, so a delayed save cannot resurrect
/// a confirmed message or erase text entered while its response was in flight.
final class AgentComposerStore {
    typealias Writer = (Data, URL) throws -> Void
    private struct Pending { let token: UUID; let value: AgentComposer }
    private let root: URL
    private let delay: TimeInterval
    private let writer: Writer
    private let worker = DispatchQueue(label: "dev.aiterminal.agent-drafts", qos: .utility)
    private var pending: [String: Pending] = [:]

    init(root: URL, debounce: TimeInterval = 0.35,
         writer: @escaping Writer = { try $0.write(to: $1, options: .atomic) }) {
        self.root = root; delay = debounce; self.writer = writer
    }

    func draft(_ key: String) -> AgentComposer {
        worker.sync { currentDraft(key) }
    }

    func schedule(_ value: AgentComposer, key: String, failed: @escaping (String) -> Void) {
        worker.async { [self] in
            let token = UUID()
            pending[key] = Pending(token: token, value: value)
            worker.asyncAfter(deadline: .now() + delay) { [self] in
                guard let current = pending[key], current.token == token else { return }
                do {
                    try write(current.value, prefix: "draft-", key: key)
                    pending[key] = nil
                } catch {
                    // Retain the latest in-memory value for a later flush/retry.
                    let message = error.localizedDescription
                    DispatchQueue.main.async { failed(message) }
                }
            }
        }
    }

    /// Scope changes, sends and backgrounding finish their current draft before
    /// returning. Normal typing performs no encoding or file I/O on the main actor.
    func flush(_ value: AgentComposer, key: String) throws {
        try worker.sync {
            pending[key] = Pending(token: UUID(), value: value)
            try write(value, prefix: "draft-", key: key)
            pending[key] = nil
        }
    }

    func delivery(_ key: String) -> AgentSendSnapshot? {
        worker.sync { read(AgentSendSnapshot.self, prefix: "pending-send-", key: key) }
    }

    func saveDelivery(_ value: AgentSendSnapshot) throws {
        try worker.sync { try write(value, prefix: "pending-send-", key: value.scopeKey) }
    }

    func confirm(_ delivery: AgentSendSnapshot) throws {
        try worker.sync {
            if currentDraft(delivery.scopeKey).requestID == delivery.requestID {
                try write(AgentComposer(), prefix: "draft-", key: delivery.scopeKey)
                pending[delivery.scopeKey] = nil
            }
            try removeDelivery(delivery)
        }
    }

    func discard(_ delivery: AgentSendSnapshot) throws {
        try worker.sync { try removeDelivery(delivery) }
    }

    private func currentDraft(_ key: String) -> AgentComposer {
        pending[key]?.value ?? read(AgentComposer.self, prefix: "draft-", key: key) ?? AgentComposer()
    }

    private func removeDelivery(_ delivery: AgentSendSnapshot) throws {
        guard let stored: AgentSendSnapshot = read(AgentSendSnapshot.self, prefix: "pending-send-", key: delivery.scopeKey),
              stored.requestID == delivery.requestID else { return }
        try FileManager.default.removeItem(at: url("pending-send-", delivery.scopeKey))
    }

    private func read<T: Decodable>(_ type: T.Type, prefix: String, key: String) -> T? {
        (try? Data(contentsOf: url(prefix, key))).flatMap { try? JSONDecoder().decode(type, from: $0) }
    }

    private func write<T: Encodable>(_ value: T, prefix: String, key: String) throws {
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try writer(JSONEncoder().encode(value), url(prefix, key))
    }

    private func url(_ prefix: String, _ key: String) -> URL {
        root.appendingPathComponent(prefix + key + ".json")
    }
}

/// Only repeat reads. A send, upload, approval, cancel or configuration write
/// must keep its own explicit delivery/acknowledgement contract.
enum AgentReadRetry {
    static let actions: Set<String> = ["state", "history", "record", "list", "global_list", "permissions", "pending", "rules", "approval_details"]

    static func transient(_ message: String) -> Bool {
        let value = message.lowercased()
        guard !WorkspaceRecovery.requiresAuthentication(message) else { return false }
        return WorkspaceRecovery.isTransportFailure(message) ||
            ["timed out", "temporarily unavailable", "connection lost", "network connection was lost"].contains { value.contains($0) }
    }

    @MainActor static func run<T>(action: String, configuration: Bool = false,
                                  valid: () -> Bool,
                                  message: (Error) -> String,
                                  pause: (UInt64) async throws -> Void = { try await Task.sleep(nanoseconds: $0) },
                                  operation: () async throws -> T) async throws -> T {
        let maximumAttempts = !configuration && actions.contains(action) ? 3 : 1
        for attempt in 0..<maximumAttempts {
            try Task.checkCancellation()
            guard valid() else { throw CancellationError() }
            do {
                let value = try await operation()
                try Task.checkCancellation()
                guard valid() else { throw CancellationError() }
                return value
            } catch {
                try Task.checkCancellation()
                guard valid() else { throw CancellationError() }
                guard attempt + 1 < maximumAttempts, transient(message(error)), !(error is CancellationError) else { throw error }
                let base = attempt == 0 ? 350_000_000.0 : 900_000_000.0
                try await pause(UInt64(base * Double.random(in: 0.8...1.2)))
            }
        }
        throw CancellationError()
    }
}

enum AgentMessagePresentation {
    /// The runtime reports the next attempt while it waits. Its delay is the
    /// original backoff, not a countdown, so only show the stable attempt count.
    static func modelRetryStatus(_ value: Any?) -> String? {
        guard let retry = value as? [String: Any],
              let attempt = (retry["attempt"] as? NSNumber)?.intValue,
              let maximum = (retry["max_attempts"] as? NSNumber)?.intValue,
              attempt > 1, maximum >= attempt else { return nil }
        return "AI 正在重试（\(attempt)/\(maximum)）"
    }

    static func interrupted(kind: String, value: [String: Any]) -> Bool {
        kind == "interaction" && value["source"] as? String == "model_interrupted" &&
            value["interrupted"] as? Bool == true && value["state"] as? String == "interrupted"
    }
}

/// State and paginated history arrive independently. Keep the last streamed
/// reply readable until its complete durable row can replace it in one render.
struct AgentLiveReplyState {
    private(set) var text = ""
    private(set) var awaitingHistory = false
    private var runID: String?
    private var rootID: String?

    mutating func receive(_ state: [String: Any]) {
        let lastRun = state["last_run"] as? [String: Any]
        let nextRun = state["run_id"] as? String ?? lastRun?["run_id"] as? String
        let nextRoot = state["root_user_message_id"] as? String ?? lastRun?["root_user_message_id"] as? String
        if let runID, let nextRun, runID != nextRun { self = AgentLiveReplyState() }
        if let rootID, let nextRoot, rootID != nextRoot { self = AgentLiveReplyState() }
        if let nextRun { runID = nextRun }
        if let nextRoot { rootID = nextRoot }
        let incoming = state["live_text"] as? String ?? ""
        if !incoming.isEmpty { text = incoming; awaitingHistory = false }
        else if !text.isEmpty { awaitingHistory = true }
    }

    mutating func reconcile(history: [[String: Any]]) {
        guard awaitingHistory, !text.isEmpty else { return }
        let replaced = history.contains { item in
            guard let kind = item["kind"] as? String, ["assistant", "interaction"].contains(kind),
                  let value = item["value"] as? [String: Any], value["partial"] as? Bool != true,
                  let complete = value["text"] as? String, complete.utf8.starts(with: text.utf8) else { return false }
            let itemRun = value["run_id"] as? String
            if let runID, let itemRun, runID != itemRun { return false }
            if let rootID { return item["root_user_message_id"] as? String == rootID }
            if let runID { return itemRun == runID }
            return false
        }
        if replaced { text = ""; awaitingHistory = false }
    }
}

/// A state read can confirm deletion while an older history page is still
/// expanding full records. Neither disk fallback nor a late page may roll back
/// that known generation within the current Agent scope.
struct AgentHistoryGenerationFence {
    private var scope: String?
    private(set) var minimum: Int64?

    mutating func bind(_ key: String?) {
        if scope != key { scope = key; minimum = nil }
    }

    mutating func accept(_ generation: Int64, scopeKey: String) -> Bool {
        guard scope == scopeKey, generation >= 0, minimum.map({ generation >= $0 }) ?? true else { return false }
        minimum = generation
        return true
    }
}
