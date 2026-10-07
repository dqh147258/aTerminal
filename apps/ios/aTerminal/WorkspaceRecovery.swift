import Foundation

/// A transport retry must not navigate or grant input. Only an explicitly selected
/// account/device/session can be resumed; all mutation retries remain user initiated.
enum WorkspaceRecovery {
    static func preservesContext(recovering: Bool, established: Bool, device: String, session: String?, targetDevice: String, targetSession: String?) -> Bool {
        recovering && established && !device.isEmpty && device == targetDevice && session == targetSession
    }

    static func canPollDisplay(selected: String?, connected: Bool, busy: Bool, sessionExited: Bool) -> Bool {
        selected != nil && connected && !busy && !sessionExited
    }

    static func controlAfterDisconnect(connected: Bool, hasControl: Bool, previous: Bool) -> Bool {
        connected ? hasControl : previous
    }

    static func isTransportFailure(_ message: String) -> Bool {
        let value = message.lowercased()
        return value == "offline" || ["relay disconnected", "connection closed", "channel closed", "request queue full or closed",
                "request timed out; outcome unknown, reconnect without replaying", "not connected", "network is unreachable", "connection reset",
                "broken pipe", "control lease expired"].contains { value.contains($0) }
    }

    static func requiresAuthentication(_ message: String) -> Bool {
        let value = message.lowercased()
        return ["401 unauthorized", "403 forbidden", "status: 401", "status: 403",
                "not logged in", "invalid connection grant", "refresh changed account identity",
                "device public key changed", "device_revoked", "invalid_token", "token_expired"].contains { value.contains($0) }
    }
}

/// Session creation is not idempotent. Any post-dispatch failure is uncertain,
/// even if the connection is already healthy again when its response arrives.
enum TerminalCreationOutcome {
    case created
    case unavailable(String)
    case unconfirmed(String)

    var uncertain: Bool { if case .unconfirmed = self { return true }; return false }
    var error: String? {
        switch self {
        case .created: return nil
        case .unavailable(let message), .unconfirmed(let message): return message
        }
    }
}
