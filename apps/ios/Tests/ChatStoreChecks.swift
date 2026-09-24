import Foundation

@main
enum ChatStoreChecks {
    static func main() throws {
        let alice = ChatIdentity(server: "https://one.invalid", account: "alice")
        let bob = ChatIdentity(server: "https://one.invalid", account: "bob")
        let otherServer = ChatIdentity(server: "https://two.invalid", account: "alice")
        let scope = ChatScope(identity: alice, device: "desktop", session: "shell")
        let variants = [scope, ChatScope(identity: bob, device: "desktop", session: "shell"), ChatScope(identity: otherServer, device: "desktop", session: "shell"), ChatScope(identity: alice, device: "other", session: "shell"), ChatScope(identity: alice, device: "desktop", session: "other")]
        precondition(Set(variants.map(\.key)).count == variants.count)
        precondition(ChatIdentity(server: "a|b", account: "c").key != ChatIdentity(server: "a", account: "b|c").key)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = ChatStore(root: directory)
        var archive = ChatArchive(scope: scope, title: "shell", deviceName: "Desktop")
        archive.messages = [ChatMessage(role: "user", content: "私有消息")]
        archive.pendingID = "pending-request"
        archive.state = "unknown"
        try store.save(archive)
        let restored = try store.load(alice)
        precondition(restored.count == 1 && restored[0].pendingID == "pending-request")
        let bobHistory = try store.load(bob)
        let otherHistory = try store.load(otherServer)
        precondition(bobHistory.isEmpty && otherHistory.isEmpty)
        let histories = (0..<20).map { ChatMessage(role: $0 % 2 == 0 ? "user" : "assistant", content: String(repeating: "中", count: 2000)) }
        let encoded = try ChatRequest.encode(action: "send", requestID: "id", message: String(repeating: "中", count: 4000), history: histories)
        precondition(encoded.utf8.count < 16000)
        let parsed = try JSONSerialization.jsonObject(with: Data(encoded.utf8)) as! [String: Any]
        precondition((parsed["messages"] as! [[String: String]]).count <= 12)
        precondition(parsed["include_screen"] as! Bool == false)
        let withScreen = try ChatRequest.encode(action: "send", requestID: "id", message: "分析", includeScreen: true)
        let explicitContext = try JSONSerialization.jsonObject(with: Data(withScreen.utf8)) as! [String: Any]
        precondition(explicitContext["include_screen"] as! Bool)
        for invalid in [" ", String(repeating: "a", count: 4001), String(repeating: "😀", count: 4000)] {
            do { _ = try ChatRequest.encode(action: "send", requestID: "id", message: invalid); fatalError("Oversized/empty message accepted") }
            catch is ChatFailure {}
        }
        let longReply = ChatMessage(role: "assistant", content: String(repeating: "a", count: 9000))
        let longHistory = try ChatRequest.encode(action: "send", requestID: "id", message: "继续", history: [longReply])
        let longObject = try JSONSerialization.jsonObject(with: Data(longHistory.utf8)) as! [String: Any]
        precondition(longObject["messages"] as! [[String: String]] == [["role": "assistant", "content": longReply.content]])
        let response = try JSONDecoder().decode(AssistantResponse.self, from: Data(#"{"available":false,"state":"unavailable","message":"未配置","request_id":"","reply":""}"#.utf8))
        precondition(!response.available && response.state == "unavailable")
        let monitoring = try JSONDecoder().decode(AssistantResponse.self, from: Data(#"{"available":true,"state":"monitoring","message":"监控中","request_id":"r1","reply":"不重复追加","monitoring":true,"events":[{"id":1,"kind":"input","text":"已入队，不代表完成","revision":0},{"id":2,"kind":"observation","text":"屏幕变化","revision":42}]}"#.utf8))
        archive.appendEvents(monitoring.events!, requestID: "r1")
        archive.appendEvents(monitoring.events!, requestID: "r1")
        precondition(archive.messages.count == 3 && archive.lastEventIDs?["r1"] == 2)
        archive.appendEvents([AssistantEvent(id: 3, kind: "observation", text: "新屏幕", revision: 44)], requestID: "r1")
        archive.appendEvents([AssistantEvent(id: 1, kind: "input", text: "另一请求", revision: 0)], requestID: "r2")
        try store.save(archive)
        var eventRestored = try store.load(alice)[0]
        eventRestored.appendEvents(monitoring.events!, requestID: "r1")
        precondition(eventRestored.messages.count == 5 && eventRestored.lastEventIDs?["r1"] == 3 && eventRestored.lastEventIDs?["r2"] == 1)
        let authorized = try ChatRequest.encode(action: "send", requestID: "r", message: "操作", allowInput: true, monitor: true)
        let authorizedObject = try JSONSerialization.jsonObject(with: Data(authorized.utf8)) as! [String: Any]
        precondition(authorizedObject["include_screen"] as! Bool && authorizedObject["allow_input"] as! Bool && authorizedObject["monitor"] as! Bool)
        let cancel = try ChatRequest.encode(action: "cancel", requestID: "r")
        let cancelObject = try JSONSerialization.jsonObject(with: Data(cancel.utf8)) as! [String: String]
        precondition(cancelObject == ["action": "cancel", "request_id": "r"])
        let suite = "dev.aiterminal.checks." + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let recent = RecentTerminal(device: "desktop", session: "shell")
        recent.save(alice, defaults: defaults)
        precondition(RecentTerminal.load(alice, defaults: defaults) == recent)
        precondition(RecentTerminal.load(bob, defaults: defaults) == nil && RecentTerminal.load(otherServer, defaults: defaults) == nil)
        RecentTerminal.remove(alice, defaults: defaults)
        precondition(RecentTerminal.load(alice, defaults: defaults) == nil)
        print("PASS: account/server/device/session isolation, pending/event cursor persistence, event deduplication, monitor/input/cancel requests, Unicode byte limits, last terminal isolation")
    }
}
