import Foundation

@main struct SessionSnapshotOwnerChecks {
    static func require(_ value: Bool, _ reason: String) throws {
        if !value { throw NSError(domain: "SnapshotOwnerChecks", code: 1, userInfo: [NSLocalizedDescriptionKey: reason]) }
    }
    static func main() throws {
        let alice = ChatIdentity(server: "https://one.invalid", account: "alice")
        let bob = ChatIdentity(server: "https://one.invalid", account: "bob")
        let otherServer = ChatIdentity(server: "https://two.invalid", account: "alice")
        var store = OwnedSessionSnapshots<String>()
        store.bind(to: alice)
        try require(store.record(["alice-title /alice/private/cwd"], device: "desktop", owner: alice), "owner write")
        store.bind(to: alice)
        try require(store.snapshots(for: alice)["desktop"]?.count == 1, "same-owner refresh retains offline snapshots")
        try require(store.snapshots(for: bob).isEmpty && store.snapshots(for: otherServer).isEmpty && store.snapshots(for: nil).isEmpty, "getter cannot relabel owner")
        store.bind(to: bob)
        try require(store.snapshots(for: bob).isEmpty && store.snapshots(for: alice).isEmpty, "account switch clears volatile values")
        try require(!store.record(["late Alice"], device: "desktop", owner: alice), "late prior-owner response")
        try require(store.record(["Bob only"], device: "desktop", owner: bob), "new owner writes")
        store.bind(to: nil) // Success paths for password change, self-revoke, and logout use this operation.
        try require(!store.record(["late Bob"], device: "desktop", owner: bob) && store.snapshots(for: bob).isEmpty, "signed-out owner cannot refill snapshots")
        store.bind(to: alice)
        try require(store.snapshots(for: alice).isEmpty, "relogin does not resurrect old volatile snapshots")
        store.record(["old server"], device: "desktop", owner: alice)
        store.bind(to: otherServer)
        try require(store.snapshots(for: otherServer).isEmpty && !store.record(["late server"], device: "desktop", owner: alice), "server identity switch")

        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let history = ChatStore(root: root)
        let archive = ChatArchive(scope: ChatScope(identity: alice, device: "desktop", session: "fixture-session"), title: "retained-history", deviceName: "fixture")
        try history.save(archive)
        store.bind(to: alice); store.record(["volatile"], device: "desktop", owner: alice); store.bind(to: nil); store.bind(to: bob)
        try require(history.load(alice).first?.title == "retained-history" && history.load(bob).isEmpty, "persistent history remains scoped and unchanged")
        print("PASS: server/account owner, same-owner retention, cross-owner hidden reads, transition clear, stale write rejection, nil-owner clear, persistent history unchanged")
    }
}
