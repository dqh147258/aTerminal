import Foundation
import CoreGraphics
import ImageIO

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() { fatalError(message) }
}

private func rejects(_ message: String, _ work: () throws -> Void) {
    do { try work(); fatalError(message) } catch {}
}

@MainActor private func eventually(_ message: String, _ condition: () -> Bool) async {
    for _ in 0..<300 {
        if condition() { return }
        try? await Task.sleep(nanoseconds: 1_000_000)
    }
    fatalError(message)
}

private let displaysJSON = """
{"screens":[{"id":"a","name":"Built-in","width":2560,"height":1440,"is_primary":true},
            {"id":"b","name":"External","width":1920,"height":1080,"is_primary":false}]}
"""

private func jpeg(width: Int = 8, height: Int = 6) -> Data {
    let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
        bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    context.setFillColor(CGColor(red: 0.2, green: 0.6, blue: 0.8, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let data = NSMutableData()
    let destination = CGImageDestinationCreateWithData(data, "public.jpeg" as CFString, 1, nil)!
    CGImageDestinationAddImage(destination, context.makeImage()!, nil)
    check(CGImageDestinationFinalize(destination), "Cannot prepare JPEG test input")
    return data as Data
}

private func payload(_ overrides: [String: Any] = [:]) throws -> String {
    var value: [String: Any] = ["screen_id": "a", "mime_type": "image/jpeg", "image_base64": jpeg().base64EncodedString(),
                              "width": 8, "height": 6, "captured_at_ms": 1234]
    value.merge(overrides) { _, new in new }
    return String(decoding: try JSONSerialization.data(withJSONObject: value), as: UTF8.self)
}

@MainActor private final class Source: RemoteScreenSource {
    enum Pending {
        case list(CheckedContinuation<[RemoteDisplay], Error>)
        case frame(String, CheckedContinuation<RemoteScreenFrame, Error>)
    }
    private(set) var calls: [String] = []
    private(set) var tickets: [RemoteScreenRequestTicket] = []
    private(set) var maxInFlight = 0
    private var inFlight = 0
    var pending: Pending?
    func remoteScreens(ticket: RemoteScreenRequestTicket) async throws -> [RemoteDisplay] {
        begin("list", ticket)
        return try await withCheckedThrowingContinuation { pending = .list($0) }
    }
    func remoteScreenFrame(screenID: String, ticket: RemoteScreenRequestTicket) async throws -> RemoteScreenFrame {
        begin(screenID, ticket)
        return try await withCheckedThrowingContinuation { pending = .frame(screenID, $0) }
    }
    private func begin(_ call: String, _ ticket: RemoteScreenRequestTicket) {
        check(pending == nil, "Concurrent RPC overwrote the pending request")
        calls.append(call); tickets.append(ticket); inFlight += 1; maxInFlight = max(maxInFlight, inFlight)
    }
    func complete() throws {
        guard let pending else { fatalError("No request to complete") }
        self.pending = nil; inFlight -= 1
        switch pending {
        case .list(let continuation): continuation.resume(returning: try RemoteScreenDecoder.screens(displaysJSON))
        case .frame(let id, let continuation):
            continuation.resume(returning: try RemoteScreenDecoder.frame(payload(["screen_id": id]), screenID: id))
        }
    }
    func fail() {
        guard let pending else { fatalError("No request to fail") }
        self.pending = nil; inFlight -= 1
        let error = RemoteScreenFailure.message("Desktop 版本过旧，请升级")
        switch pending {
        case .list(let continuation): continuation.resume(throwing: error)
        case .frame(_, let continuation): continuation.resume(throwing: error)
        }
    }
}

@main private struct RemoteScreenChecks {
    static func decoding() throws {
        let screens = try RemoteScreenDecoder.screens(displaysJSON)
        check(screens.count == 2 && screens[0].isPrimary && !screens[1].isPrimary, "List contract/main-display flag lost")
        check(screens[0].width == 2560 && screens[0].dimensions == "2560 × 1440", "Physical display dimensions were capped like frames")
        let empty = try RemoteScreenDecoder.screens("{\"screens\":[]}")
        check(empty.isEmpty, "Empty list rejected")
        rejects("Duplicate IDs accepted") { _ = try RemoteScreenDecoder.screens(displaysJSON.replacingOccurrences(of: "\"b\"", with: "\"a\"")) }
        rejects("Missing dimensions accepted") { _ = try RemoteScreenDecoder.screens("{\"screens\":[{\"id\":\"a\",\"name\":\"x\",\"is_primary\":true}]}") }
        let oversizedList = try JSONSerialization.data(withJSONObject: ["screens": (0..<65).map {
            ["id": String($0), "name": "x", "width": 1, "height": 1, "is_primary": false] as [String: Any]
        }])
        rejects("More than 64 displays accepted") { _ = try RemoteScreenDecoder.screens(String(decoding: oversizedList, as: UTF8.self)) }
        rejects("Oversized list JSON accepted") { _ = try RemoteScreenDecoder.screens(String(repeating: " ", count: 164 * 1024 + 1)) }
        for overrides in [["id": String(repeating: "a", count: 129)], ["id": "a\n"],
                          ["name": String(repeating: "界", count: 86)], ["name": "Monitor\u{0000}"]] {
            var display: [String: Any] = ["id": "a", "name": "main", "width": 1, "height": 1, "is_primary": true]
            display.merge(overrides) { _, new in new }
            let data = try JSONSerialization.data(withJSONObject: ["screens": [display]])
            rejects("Unsafe display text accepted") { _ = try RemoteScreenDecoder.screens(String(decoding: data, as: UTF8.self)) }
        }
        let valid = try RemoteScreenDecoder.frame(payload(), screenID: "a")
        check(valid.image.width == 8 && valid.image.height == 6 && valid.capturedAtMS == 1234, "JPEG/timestamp contract lost")
        for (key, value) in [("screen_id", "b"), ("mime_type", "image/png"), ("image_base64", "not base64")] {
            rejects("Invalid \(key) accepted") { _ = try RemoteScreenDecoder.frame(payload([key: value]), screenID: "a") }
        }
        for width in [0, 9, 1921] {
            rejects("Incorrect JPEG width accepted") { _ = try RemoteScreenDecoder.frame(payload(["width": width]), screenID: "a") }
        }
        rejects("Non-JPEG bytes accepted") { _ = try RemoteScreenDecoder.frame(payload(["image_base64": Data("abc".utf8).base64EncodedString()]), screenID: "a") }
        rejects("Oversized base64 accepted") { _ = try RemoteScreenDecoder.frame(payload(["image_base64": String(repeating: "A", count: 160 * 1024 + 4)]), screenID: "a") }
        rejects("Oversized JSON accepted") { _ = try RemoteScreenDecoder.frame(String(repeating: " ", count: 164 * 1024 + 1), screenID: "a") }
        // The JPEG metadata must be checked before allocating/decompressing a large bitmap.
        rejects("Actual JPEG dimensions trusted from JSON") { _ = try RemoteScreenDecoder.frame(payload(["image_base64": jpeg(width: 1930, height: 1).base64EncodedString()]), screenID: "a") }
        rejects("Zero capture timestamp accepted") { _ = try RemoteScreenDecoder.frame(payload(["captured_at_ms": 0]), screenID: "a") }
        for (detail, expected) in [("remote_screens_unavailable", "升级 Desktop"), ("permission_denied", "允许 Desktop 录制屏幕"),
                                   ("allow screen recording", "允许 Desktop 录制屏幕"), ("unknown_screen", "显示器已断开"),
                                   ("screen_busy", "稍后重试"), ("request timeout", "超时"),
                                   ("Wayland unsupported", "X11")] {
            check(RemoteScreenErrorAdvice.message(detail).contains(expected), "Known error lacks actionable Chinese guidance")
        }
        check(RemoteScreenErrorAdvice.message("new-server-error: details") == "new-server-error: details", "Unknown error details discarded")
        check(RemoteScreenErrorAdvice.refreshDisplays("unknown_screen"), "Disconnected display offered only frame retry")
        let paired = RemoteScreenConnection.current(connected: true, reconnecting: false, authenticationRequired: false,
            server: "", account: "", device: "", pairedChannel: "paired:test", generation: 7)
        check(paired?.device == "paired:test" && paired?.account == "" && paired?.generation == 7, "Read-only pair requires account/device identity")
        check(RemoteScreenConnection.current(connected: false, reconnecting: false, authenticationRequired: false,
            server: "", account: "", device: "", pairedChannel: "paired:test", generation: 8) == nil, "Disconnected paired channel exposed")
        check(RemoteScreenConnection.current(connected: true, reconnecting: false, authenticationRequired: false,
            server: "server", account: "new-owner", device: "", pairedChannel: "paired:test", generation: 8) == nil, "New account inherited old paired channel")
        print("PASS: snake_case list/frame contract, primary/dimensions, empty/invalid lists, JPEG type/dimensions and payload limits")
    }

    @MainActor static func lifecycle() async throws {
        let source = Source()
        let model = RemoteScreenModel(intervalNanoseconds: 10_000_000)
        let connection = RemoteScreenConnection(server: "server", account: "account", device: "desktop", generation: 1)
        func configure(_ context: RemoteScreenConnection? = connection, active: Bool = true, visible: Bool = true) {
            model.configure(source: source, connection: context, visible: visible, active: active)
        }
        configure(nil)
        try? await Task.sleep(nanoseconds: 20_000_000)
        check(source.calls.isEmpty && !model.connected, "Disconnected view made an RPC")
        configure()
        await eventually("List never loaded") { source.calls.count == 1 }
        source.fail()
        await eventually("Upgrade error was hidden") { model.listError == "Desktop 版本过旧，请升级" }
        model.refreshList()
        await eventually("List retry never dispatched") { source.calls.count == 2 }
        try source.complete()
        await eventually("List retry did not recover") { model.screens.count == 2 }
        check(source.calls.count == 2, "List automatically captured a frame before user selection")
        model.select("a")
        await eventually("Selected frame never dispatched") { source.calls.count == 3 }
        for _ in 0..<5 { model.select("b"); model.select("a") }
        model.select("b")
        check(!source.tickets.last!.isValid, "Switching failed to invalidate request ticket")
        try? await Task.sleep(nanoseconds: 30_000_000)
        check(source.calls.count == 3 && model.frame == nil, "Switching queued overlapping frames")
        try source.complete()
        await eventually("Latest selected screen not requested after old response") { source.calls.count == 4 }
        check(source.calls.last == "b" && model.frame == nil, "Stale frame leaked across screen selection")
        try source.complete()
        await eventually("New screen not shown") { model.frame?.screenID == "b" }
        await eventually("Periodic polling did not start") { source.calls.count == 5 }
        model.showList()
        check(!source.tickets.last!.isValid && model.frame == nil && model.selectedID == nil, "Returning to list retained capture")
        try source.complete()
        try? await Task.sleep(nanoseconds: 30_000_000)
        check(source.calls.count == 5 && model.frame == nil, "Returning to list kept polling")

        model.select("a")
        await eventually("Frame for close check not dispatched") { source.calls.count == 6 }
        configure(active: false)
        check(!source.tickets.last!.isValid, "Scene inactivity did not stop FFI ticket")
        try source.complete()
        try? await Task.sleep(nanoseconds: 30_000_000)
        check(source.calls.count == 6 && model.frame == nil, "Background published/scheduled a stale frame")
        configure()
        await eventually("Foreground did not resume viewing") { source.calls.count == 7 }
        source.fail()
        await eventually("Frame failure not exposed") { model.frameError != nil }
        try? await Task.sleep(nanoseconds: 30_000_000)
        check(source.calls.count == 7, "Error retried in a tight polling loop")
        model.retryFrame()
        await eventually("Frame retry not dispatched") { source.calls.count == 8 }
        model.stop(); configure() // Rapid close/reopen keeps the old request slot occupied.
        check(!source.tickets.last!.isValid && source.calls.count == 8, "Close/reopen queued another request")
        try source.complete()
        await eventually("Reopen did not start fresh list") { source.calls.count == 9 }
        check(source.calls.last == "list" && model.frame == nil, "Reopen resurrected old selection")

        // Each scope change occurs while a list request is actually in flight.
        for context in [RemoteScreenConnection(server: "server", account: "account", device: "other", generation: 2),
                        RemoteScreenConnection(server: "server", account: "other", device: "other", generation: 3),
                        RemoteScreenConnection(server: "other", account: "other", device: "other", generation: 4)] {
            let count = source.calls.count
            configure(context)
            check(!source.tickets.last!.isValid, "Device/account/server change did not invalidate queued RPC")
            try source.complete()
            await eventually("New connection did not reload list") { source.calls.count == count + 1 }
            check(model.screens.isEmpty && model.frame == nil, "Old owner's screens published after scope change")
        }
        configure(nil)
        try source.complete()
        try? await Task.sleep(nanoseconds: 30_000_000)
        check(!model.connected && model.screens.isEmpty && model.frame == nil, "Disconnect kept old owner's data")
        let count = source.calls.count
        configure(nil, visible: false)
        try? await Task.sleep(nanoseconds: 30_000_000)
        check(source.calls.count == count && source.maxInFlight == 1, "Requests continued after close/disconnect or overlapped")
        configure(); model.stop()
        try? await Task.sleep(nanoseconds: 30_000_000)
        check(source.calls.count == count, "Close before dispatch still entered RPC source")
        print("PASS: no-selection list access, single request, switch/return/close/reopen, background/resume, stale response isolation, errors/retry, device/account/server/disconnect")
    }

    static func main() async throws {
        try decoding()
        try await lifecycle()
    }
}
