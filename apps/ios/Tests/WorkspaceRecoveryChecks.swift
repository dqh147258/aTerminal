import Foundation

private func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    guard condition() else { fatalError(message) }
}

@main private struct WorkspaceRecoveryChecks {
    static func main() {
        check(WorkspaceRecovery.preservesContext(recovering: true, established: true, device: "desktop-a", session: "session-a", targetDevice: "desktop-a", targetSession: "session-a"), "Same workspace lost context")
        check(WorkspaceRecovery.preservesContext(recovering: true, established: true, device: "desktop-a", session: nil, targetDevice: "desktop-a", targetSession: nil), "Global-only workspace lost context")
        check(!WorkspaceRecovery.preservesContext(recovering: false, established: true, device: "desktop-a", session: "session-a", targetDevice: "desktop-a", targetSession: "session-a"), "Explicit selection treated as retry")
        check(!WorkspaceRecovery.preservesContext(recovering: true, established: false, device: "desktop-a", session: nil, targetDevice: "desktop-a", targetSession: nil), "First connection treated as established")
        check(!WorkspaceRecovery.preservesContext(recovering: true, established: true, device: "desktop-a", session: "session-a", targetDevice: "desktop-b", targetSession: "session-a"), "Recovery switched desktops")
        check(!WorkspaceRecovery.preservesContext(recovering: true, established: true, device: "desktop-a", session: "session-a", targetDevice: "desktop-a", targetSession: "session-b"), "Recovery switched sessions")
        for failure in ["HTTP status client error (401 Unauthorized)", "HTTP status client error (403 Forbidden)", "not logged in", "device public key changed; revoke and enroll the device again", "invalid connection grant", "refresh changed account identity"] {
            check(WorkspaceRecovery.requiresAuthentication(failure), "Authentication/security failure retried as transport")
        }
        for failure in ["offline", "request timed out; outcome unknown, reconnect without replaying", "relay disconnected", "relay control connection closed", "remote request timed out; outcome unknown, reconnect without replaying", "connection reset by peer", "relay control lease expired; reconnect to revalidate authorization"] {
            check(!WorkspaceRecovery.requiresAuthentication(failure), "Transient failure destroyed authentication")
            check(WorkspaceRecovery.isTransportFailure(failure), "Transport failure not detected in agent-only workspace")
        }
        check(!WorkspaceRecovery.isTransportFailure("permission_revision_conflict"), "Permission conflict retried as transport")
        check(!WorkspaceRecovery.isTransportFailure("tool command failed"), "Tool failure retried as transport")
        check(!WorkspaceRecovery.controlAfterDisconnect(connected: true, hasControl: false, previous: true), "Reconnect reclaimed lost control")
        check(WorkspaceRecovery.controlAfterDisconnect(connected: true, hasControl: true, previous: false), "Current control was not captured")
        check(!WorkspaceRecovery.controlAfterDisconnect(connected: false, hasControl: false, previous: false), "Offline repeated lifecycle granted control")
        check(TerminalCreationOutcome.unconfirmed("lost reply").uncertain, "Post-dispatch timeout permitted duplicate create")
        check(TerminalCreationOutcome.unconfirmed("request failed").error == "request failed", "Uncertain create lost its error")
        check(!TerminalCreationOutcome.unavailable("offline before dispatch").uncertain, "Known pre-dispatch failure became uncertain")
        check(!TerminalCreationOutcome.created.uncertain && TerminalCreationOutcome.created.error == nil, "Confirmed creation did not finish")
        print("PASS: same-session/global-only recovery, first-connect and scope isolation, auth versus transient failures, actual control retention")
    }
}
