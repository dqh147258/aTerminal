import Foundation
import CryptoKit

enum WorkspacePreferences {
    static var fixture: Bool {
        #if DEBUG
        return ProcessInfo.processInfo.arguments.contains("--workspace-fixture") || ProcessInfo.processInfo.arguments.contains("--login-fixture") || ProcessInfo.processInfo.arguments.contains("--settings-fixture")
        #else
        return false
        #endif
    }
    static var defaultServer: String { Bundle.main.object(forInfoDictionaryKey: "ATerminalServerURL") as? String ?? "" }
    static var serverCA: String {
        let encoded = Bundle.main.object(forInfoDictionaryKey: "ATerminalServerCABase64") as? String ?? ""
        return Data(base64Encoded: encoded).flatMap { String(data: $0, encoding: .utf8) } ?? ""
    }

    static var serviceTest: Bool {
        #if DEBUG
        return ProcessInfo.processInfo.arguments.contains("--service-test")
        #else
        return false
        #endif
    }
    private static var authorizationFixtureID: String? {
        #if DEBUG
        guard ProcessInfo.processInfo.arguments.contains("--authorization-fixture"),
              let value = ProcessInfo.processInfo.arguments.first(where: { $0.hasPrefix("--authorization-fixture-id=") })?.split(separator: "=").last,
              UUID(uuidString: String(value)) != nil else { return nil }
        return String(value)
        #else
        return nil
        #endif
    }
    static var defaults: UserDefaults {
        #if DEBUG
        if let authorizationFixtureID { return UserDefaults(suiteName: "dev.aiterminal.authorization-fixture." + authorizationFixtureID)! }
        if fixture { return UserDefaults(suiteName: "dev.aiterminal.ui-fixtures")! }
        #endif
        if serviceTest { return UserDefaults(suiteName: "dev.aiterminal.integration")! }
        return .standard
    }
    static var historyDirectory: URL {
        #if DEBUG
        if let authorizationFixtureID {
            return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("AuthorizationFixtureHistory", isDirectory: true).appendingPathComponent(authorizationFixtureID, isDirectory: true)
        }
        if fixture {
            return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("FixtureAssistantHistory", isDirectory: true)
        }
        #endif
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent(serviceTest ? "IntegrationAssistantHistory" : "AssistantHistory", isDirectory: true)
    }
}

struct RecentTerminal: Codable, Equatable {
    let device: String
    let session: String
    static func load(_ identity: ChatIdentity, defaults: UserDefaults = WorkspacePreferences.defaults) -> RecentTerminal? {
        defaults.data(forKey: "terminal.recent." + identity.key).flatMap { try? JSONDecoder().decode(Self.self, from: $0) }
    }
    func save(_ identity: ChatIdentity, defaults: UserDefaults = WorkspacePreferences.defaults) {
        if let data = try? JSONEncoder().encode(self) { defaults.set(data, forKey: "terminal.recent." + identity.key) }
    }
    static func remove(_ identity: ChatIdentity, defaults: UserDefaults = WorkspacePreferences.defaults) { defaults.removeObject(forKey: "terminal.recent." + identity.key) }
}

struct ChatIdentity: Codable, Hashable {
    let server: String
    let account: String
    var key: String { storageKey([server, account]) }
}

/// Volatile Desktop/session snapshots belong to one server/account identity only.
/// Binding or clearing an owner never reads, migrates or deletes persistent chat history.
struct OwnedSessionSnapshots<Session> {
    private(set) var owner: ChatIdentity?
    private var devices: [String: [Session]] = [:]
    mutating func bind(to identity: ChatIdentity?) {
        guard owner != identity else { return }
        devices.removeAll(); owner = identity
    }
    @discardableResult mutating func record(_ sessions: [Session], device: String, owner expected: ChatIdentity?) -> Bool {
        guard let expected, owner == expected else { return false }
        devices[device] = sessions; return true
    }
    func snapshots(for identity: ChatIdentity?) -> [String: [Session]] {
        guard let identity, owner == identity else { return [:] }
        return devices
    }
}

struct ChatScope: Codable, Hashable {
    let identity: ChatIdentity
    let device: String
    let session: String
    var key: String { storageKey([identity.server, identity.account, device, session]) }
}

private func storageKey(_ components: [String]) -> String {
    // An encoded array preserves component boundaries even for unusual account names.
    let data = try! JSONEncoder().encode(components)
    return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

struct ChatMessage: Codable, Identifiable, Equatable {
    var id = UUID().uuidString
    let role: String
    let content: String
    var date = Date()
    var eventKind: String?
    var revision: UInt64?
    var requestID: String?
}

struct ChatArchive: Codable, Identifiable {
    let scope: ChatScope
    var title: String
    var deviceName: String
    var cwd: String? = nil
    var messages: [ChatMessage] = []
    var updated = Date()
    var pendingID: String?
    var state = "idle"
    var status = ""
    var lastEventIDs: [String: UInt64]?
    var id: String { scope.key }
    var workingDirectory: String { cwd.flatMap { $0.isEmpty ? nil : $0 } ?? title }
    @discardableResult mutating func updateDirectory(_ value: String, title: String, deviceName: String) -> Bool {
        guard !value.isEmpty, cwd != value || self.title != title || self.deviceName != deviceName else { return false }
        cwd = value; self.title = title; self.deviceName = deviceName; updated = Date()
        return true
    }
    mutating func appendEvents(_ events: [AssistantEvent], requestID: String) {
        var last = lastEventIDs?[requestID] ?? 0
        for event in events.sorted(by: { $0.id < $1.id }) where event.id > last {
            messages.append(ChatMessage(id: requestID + ":" + String(event.id), role: "assistant", content: event.text, eventKind: event.kind, revision: event.revision, requestID: requestID))
            last = event.id
        }
        if lastEventIDs == nil { lastEventIDs = [:] }
        lastEventIDs?[requestID] = last
    }
}

struct AssistantEvent: Decodable {
    let id: UInt64
    let kind: String
    let text: String
    let revision: UInt64
}

struct AssistantResponse: Decodable {
    let available: Bool
    let state: String
    let message: String
    let request_id: String
    let reply: String
    let events: [AssistantEvent]?
    let monitoring: Bool?
}

enum ChatRequest {
    static func encode(action: String, requestID: String? = nil, message: String? = nil,
                       includeScreen: Bool = false, allowInput: Bool = false, monitor: Bool = false, history: [ChatMessage] = []) throws -> String {
        var object: [String: Any] = ["action": action]
        if let requestID { object["request_id"] = requestID }
        if let message {
            guard !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, message.unicodeScalars.count <= 4000 else {
                throw ChatFailure.message("消息须为 1–4000 个字符")
            }
            object["message"] = message
            object["include_screen"] = includeScreen || monitor
            object["allow_input"] = allowInput
            object["monitor"] = monitor
            var previous = Array(history.filter { $0.role == "user" || $0.role == "assistant" }.suffix(12))
            while true {
                object["messages"] = previous.map { ["role": $0.role, "content": $0.content] }
                let data = try JSONSerialization.data(withJSONObject: object)
                if data.count < 16000 { return String(decoding: data, as: UTF8.self) }
                guard !previous.isEmpty else { throw ChatFailure.message("消息过长，请缩短后发送") }
                previous.removeFirst()
            }
        }
        return String(decoding: try JSONSerialization.data(withJSONObject: object), as: UTF8.self)
    }
}

enum ChatFailure: LocalizedError {
    case message(String)
    var errorDescription: String? { if case let .message(message) = self { return message }; return nil }
}

struct ChatStore {
    var root: URL = WorkspacePreferences.historyDirectory
    func load(_ identity: ChatIdentity) throws -> [ChatArchive] {
        let folder = root.appendingPathComponent(identity.key, isDirectory: true)
        guard FileManager.default.fileExists(atPath: folder.path) else { return [] }
        return try FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "json" }
            .map { try JSONDecoder().decode(ChatArchive.self, from: Data(contentsOf: $0)) }
            .filter { $0.scope.identity == identity }
            .sorted { $0.updated > $1.updated }
    }
    func save(_ archive: ChatArchive) throws {
        var folder = root.appendingPathComponent(archive.scope.identity.key, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var values = URLResourceValues(); values.isExcludedFromBackup = true
        try folder.setResourceValues(values)
        let file = folder.appendingPathComponent(archive.id + ".json")
        let data = try JSONEncoder().encode(archive)
        #if os(iOS)
        try data.write(to: file, options: [.atomic, .completeFileProtection])
        #else
        try data.write(to: file, options: .atomic)
        #endif
    }
}
