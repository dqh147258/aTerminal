import Foundation
import Combine
import CoreGraphics
import ImageIO

struct RemoteDisplay: Decodable, Identifiable, Equatable {
    let id: String
    let name: String
    let width: UInt32
    let height: UInt32
    let isPrimary: Bool

    private enum CodingKeys: String, CodingKey {
        case id, name, width, height
        case isPrimary = "is_primary"
    }
    var displayName: String { name.isEmpty ? "显示器 \(id)" : name }
    var dimensions: String { "\(width) × \(height)" }
}

enum RemoteScreenFailure: LocalizedError {
    case message(String)
    var errorDescription: String? { if case .message(let text) = self { return text }; return nil }
}

struct RemoteScreenFrame {
    let screenID: String
    let image: CGImage
    let width: UInt32
    let height: UInt32
    let capturedAtMS: UInt64
}

enum RemoteScreenDecoder {
    static func screens(_ json: String) throws -> [RemoteDisplay] {
        struct List: Decodable { let screens: [RemoteDisplay] }
        guard json.utf8.count <= 164 * 1024 else { throw RemoteScreenFailure.message("显示器响应超过大小限制") }
        let screens = try JSONDecoder().decode(List.self, from: Data(json.utf8)).screens
        guard screens.count <= 64, Set(screens.map(\.id)).count == screens.count,
              screens.allSatisfy({ !$0.id.isEmpty && $0.id.utf8.count <= 128 && $0.name.utf8.count <= 256
                  && !$0.id.unicodeScalars.contains(where: { $0.properties.generalCategory == .control })
                  && !$0.name.unicodeScalars.contains(where: { $0.properties.generalCategory == .control })
                  && $0.width > 0 && $0.height > 0 }) else {
            throw RemoteScreenFailure.message("显示器列表格式无效")
        }
        return screens
    }

    // Invoked on the transport worker: force JPEG decoding before publishing to SwiftUI.
    static func frame(_ json: String, screenID: String) throws -> RemoteScreenFrame {
        struct Payload: Decodable {
            let screen_id: String
            let mime_type: String
            let image_base64: String
            let width: UInt32
            let height: UInt32
            let captured_at_ms: UInt64
        }
        guard json.utf8.count <= 164 * 1024 else { throw RemoteScreenFailure.message("屏幕响应超过大小限制") }
        let value = try JSONDecoder().decode(Payload.self, from: Data(json.utf8))
        guard value.screen_id == screenID, value.mime_type == "image/jpeg",
              (1...1920).contains(value.width), (1...1920).contains(value.height), value.captured_at_ms > 0,
              value.image_base64.utf8.count <= 160 * 1024,
              let bytes = Data(base64Encoded: value.image_base64), bytes.count <= 120 * 1024,
              bytes.starts(with: [0xff, 0xd8]), bytes.suffix(2) == Data([0xff, 0xd9]),
              let source = CGImageSourceCreateWithData(bytes as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              CGImageSourceGetType(source) as String? == "public.jpeg",
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? NSNumber,
              let height = properties[kCGImagePropertyPixelHeight] as? NSNumber,
              width.intValue == Int(value.width), height.intValue == Int(value.height),
              let image = CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary),
              image.width == Int(value.width), image.height == Int(value.height) else {
            throw RemoteScreenFailure.message("屏幕画面格式无效，请重试")
        }
        return RemoteScreenFrame(screenID: value.screen_id, image: image, width: value.width,
                                 height: value.height, capturedAtMS: value.captured_at_ms)
    }
}

struct RemoteScreenConnection: Equatable {
    let server: String
    let account: String
    let device: String
    let generation: Int

    static func current(connected: Bool, reconnecting: Bool, authenticationRequired: Bool,
                        server: String, account: String, device: String, pairedChannel: String?, generation: Int) -> Self? {
        guard connected, !reconnecting, !authenticationRequired else { return nil }
        if !server.isEmpty, !account.isEmpty, !device.isEmpty {
            return Self(server: server, account: account, device: device, generation: generation)
        }
        if server.isEmpty, account.isEmpty, let pairedChannel {
            return Self(server: "", account: "", device: pairedChannel, generation: generation)
        }
        return nil
    }
}

enum RemoteScreenErrorAdvice {
    static func message(_ detail: String) -> String {
        let value = detail.lowercased()
        if value.contains("remote_screens_unavailable") { return "当前 Desktop 版本不支持远程屏幕，请升级 Desktop 后重试" }
        if value.contains("wayland"), value.contains("unsupported") { return "暂不支持 Wayland，请在电脑使用 X11 会话后重试" }
        if value.contains("permission_denied") || value.contains("permission denied") || value.contains("allow screen recording") {
            return "请在电脑的系统设置中允许 Desktop 录制屏幕，然后重试"
        }
        if refreshDisplays(detail) { return "该显示器已断开，请刷新显示器列表" }
        if value.contains("busy") { return "电脑正在处理屏幕请求，请稍后重试" }
        if value.contains("timeout") || value.contains("timed out") { return "获取屏幕超时，请重试" }
        return detail
    }
    static func refreshDisplays(_ detail: String) -> Bool { detail.lowercased().contains("unknown_screen") }
}

// Can be invalidated by the main actor while a synchronous RPC is on the worker.
final class RemoteScreenRequestTicket {
    let connection: RemoteScreenConnection
    private let lock = NSLock()
    private var valid = true
    init(connection: RemoteScreenConnection) { self.connection = connection }
    var isValid: Bool { lock.lock(); defer { lock.unlock() }; return valid }
    func cancel() { lock.lock(); defer { lock.unlock() }; valid = false }
}

@MainActor protocol RemoteScreenSource: AnyObject {
    func remoteScreens(ticket: RemoteScreenRequestTicket) async throws -> [RemoteDisplay]
    func remoteScreenFrame(screenID: String, ticket: RemoteScreenRequestTicket) async throws -> RemoteScreenFrame
}

@MainActor final class RemoteScreenModel: ObservableObject {
    @Published private(set) var screens: [RemoteDisplay] = []
    @Published private(set) var selectedID: String?
    @Published private(set) var frame: RemoteScreenFrame?
    @Published private(set) var listLoading = false
    @Published private(set) var frameLoading = false
    @Published private(set) var listError: String?
    @Published private(set) var frameError: String?
    @Published private(set) var frameNeedsListRefresh = false
    @Published private(set) var connected = false
    var selected: RemoteDisplay? { screens.first { $0.id == selectedID } }

    private weak var source: RemoteScreenSource?
    private var connection: RemoteScreenConnection?
    private var visible = false
    private var active = false
    private var needsList = true
    private var request: Task<Void, Never>?
    private var delay: Task<Void, Never>?
    private var ticket: RemoteScreenRequestTicket?
    private let interval: UInt64

    init(intervalNanoseconds: UInt64 = 1_000_000_000) { interval = intervalNanoseconds }
    deinit { ticket?.cancel(); delay?.cancel() }

    func configure(source: RemoteScreenSource, connection: RemoteScreenConnection?, visible: Bool, active: Bool) {
        self.source = source
        guard self.connection != connection || self.visible != visible || self.active != active else { return }
        let reset = self.connection != connection || !visible || !self.visible
        invalidate()
        self.connection = connection; self.visible = visible; self.active = active; connected = connection != nil
        if reset {
            screens = []; selectedID = nil; needsList = true; listError = nil; frameError = nil; frameNeedsListRefresh = false
        }
        frame = nil
        listLoading = visible && active && connection != nil && needsList
        frameLoading = visible && active && connection != nil && selectedID != nil && !needsList
        drive()
    }

    func stop() {
        invalidate(); visible = false; screens = []; selectedID = nil; frame = nil
        listLoading = false; frameLoading = false; needsList = true
    }

    func select(_ screenID: String) {
        guard screens.contains(where: { $0.id == screenID }), selectedID != screenID else { return }
        invalidate(); selectedID = screenID; frame = nil; frameError = nil; frameLoading = true; frameNeedsListRefresh = false
        drive()
    }

    func showList() {
        invalidate(); selectedID = nil; frame = nil; frameLoading = false; frameError = nil; frameNeedsListRefresh = false
    }

    func refreshList() {
        invalidate(); needsList = true; listLoading = true; listError = nil; frameError = nil
        drive()
    }

    func retryFrame() {
        invalidate(); frameError = nil; frameLoading = frame == nil
        drive()
    }

    private func invalidate() {
        ticket?.cancel(); delay?.cancel(); delay = nil
        // Keep the request slot until synchronous FFI actually returns. New intent
        // is picked up then, so even rapid close/reopen or switching never queues RPCs.
    }

    private func drive() {
        guard visible, active, let connection, let source, request == nil, delay == nil else { return }
        guard needsList || (selectedID != nil && frameError == nil) else { return }
        let listing = needsList
        let screenID = selectedID
        let ticket = RemoteScreenRequestTicket(connection: connection)
        self.ticket = ticket
        if listing { listLoading = true } else { frameLoading = frame == nil }
        request = Task { [weak self] in
            do {
                guard ticket.isValid else { throw CancellationError() }
                if listing {
                    let screens = try await source.remoteScreens(ticket: ticket)
                    guard let self else { return }
                    self.request = nil; self.ticket = nil
                    guard ticket.isValid else { self.drive(); return }
                    self.screens = screens; self.needsList = false; self.listLoading = false; self.listError = nil
                    if !screens.contains(where: { $0.id == self.selectedID }) { self.selectedID = nil; self.frame = nil }
                    self.drive()
                } else if let screenID {
                    let frame = try await source.remoteScreenFrame(screenID: screenID, ticket: ticket)
                    guard let self else { return }
                    self.request = nil; self.ticket = nil
                    guard ticket.isValid else { self.drive(); return }
                    self.frame = frame; self.frameLoading = false; self.frameError = nil
                    self.scheduleNextFrame()
                }
            } catch {
                guard let self else { return }
                self.request = nil; self.ticket = nil
                guard ticket.isValid else { self.drive(); return }
                let detail = error.localizedDescription
                if listing { self.needsList = false; self.listLoading = false; self.listError = RemoteScreenErrorAdvice.message(detail) }
                else {
                    self.frameLoading = false; self.frameError = RemoteScreenErrorAdvice.message(detail)
                    self.frameNeedsListRefresh = RemoteScreenErrorAdvice.refreshDisplays(detail)
                }
            }
        }
    }

    private func scheduleNextFrame() {
        delay = Task { [weak self, interval] in
            do { try await Task.sleep(nanoseconds: interval) } catch { return }
            guard !Task.isCancelled, let self else { return }
            self.delay = nil; self.drive()
        }
    }
}
