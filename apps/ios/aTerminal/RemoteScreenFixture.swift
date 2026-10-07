#if DEBUG
import Foundation
import CoreGraphics
import ImageIO

// Dedicated offline UI fixture. It never touches account state, FFI or a Desktop.
@MainActor final class RemoteScreenFixtureSource: RemoteScreenSource {
    private let worker = DispatchQueue(label: "dev.aiterminal.screen-fixture")
    func remoteScreens(ticket: RemoteScreenRequestTicket) async throws -> [RemoteDisplay] {
        guard ticket.isValid else { throw CancellationError() }
        return try RemoteScreenDecoder.screens("""
        {"screens":[{"id":"fixture-main","name":"主显示器","width":640,"height":360,"is_primary":true},
                    {"id":"fixture-external","name":"外接显示器","width":480,"height":320,"is_primary":false}]}
        """)
    }
    func remoteScreenFrame(screenID: String, ticket: RemoteScreenRequestTicket) async throws -> RemoteScreenFrame {
        try await withCheckedThrowingContinuation { continuation in
            worker.async {
                do {
                    guard ticket.isValid else { throw CancellationError() }
                    let width = screenID == "fixture-main" ? 640 : 480
                    let height = screenID == "fixture-main" ? 360 : 320
                    guard let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                        bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else {
                        throw RemoteScreenFailure.message("Fixture image unavailable")
                    }
                    context.setFillColor(CGColor(red: 0.035, green: 0.05, blue: 0.086, alpha: 1))
                    context.fill(CGRect(x: 0, y: 0, width: width, height: height))
                    context.setFillColor(CGColor(red: 0.22, green: 0.74, blue: 0.97, alpha: 1))
                    context.fill(CGRect(x: 24, y: 24, width: width - 48, height: height - 48))
                    let data = NSMutableData()
                    guard let image = context.makeImage(),
                          let destination = CGImageDestinationCreateWithData(data, "public.jpeg" as CFString, 1, nil) else {
                        throw RemoteScreenFailure.message("Fixture image unavailable")
                    }
                    CGImageDestinationAddImage(destination, image, nil)
                    guard CGImageDestinationFinalize(destination) else { throw RemoteScreenFailure.message("Fixture JPEG unavailable") }
                    let json = try JSONSerialization.data(withJSONObject: ["screen_id": screenID, "mime_type": "image/jpeg",
                        "image_base64": (data as Data).base64EncodedString(), "width": width, "height": height, "captured_at_ms": 1])
                    continuation.resume(returning: try RemoteScreenDecoder.frame(String(decoding: json, as: UTF8.self), screenID: screenID))
                } catch { continuation.resume(throwing: error) }
            }
        }
    }
}
#endif
