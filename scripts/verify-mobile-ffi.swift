import Foundation

@main
struct VerifyMobileFFI {
    static func main() throws {
        let bytes = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let replica = TerminalReplica()
        guard try replica.applySnapshot(bytes: bytes) else { fatalError("initial snapshot rejected") }
        guard let frame = try replica.frame() else { fatalError("empty replica") }
        let text = frame.cells.filter { $0.width > 0 }.map(\.text).joined()
        precondition(frame.rows == 12 && frame.cols == 48)
        precondition(text.contains("同一会话，同一屏幕。"))
        precondition(text.contains("e\u{301}"))
        let duplicate = try replica.applySnapshot(bytes: bytes)
        precondition(!duplicate, "duplicate snapshot must be ignored")
        try replica.reset()
        let empty = try replica.frame()
        precondition(empty == nil)
        print("PASS: Swift → UniFFI → Rust replica, Unicode, duplicate frame, reset")
    }
}
