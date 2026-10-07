package com.yxf.aterminal

import org.junit.Assert.*
import org.junit.Test

class WorkspaceReconnectTest {
    @Test fun repeatedDisconnectAndRetryCannotStartParallelAttempts() {
        val state = WorkspaceReconnect()
        assertNull("Cold entry is not a reconnect", state.attempt())
        assertTrue(state.interrupt())
        val first = state.attempt()!!
        repeat(10) {
            assertFalse(state.interrupt())
            assertNull(state.attempt())
            assertEquals(first, state.epoch)
        }
        assertTrue(state.complete(first))
        assertFalse(state.pending)
        assertFalse(state.complete(first))
    }

    @Test fun transientFailuresBackOffAndSuccessfulRecoveryResetsDelay() {
        val state = WorkspaceReconnect()
        state.interrupt()
        assertEquals(1_000L, state.delayMillis)
        for (delay in listOf(2_000L, 4_000L, 8_000L, 15_000L, 15_000L)) {
            assertTrue(state.fail(state.attempt()!!, null))
            assertEquals(delay, state.delayMillis)
            assertNull(state.blocked)
        }
        assertTrue(state.complete(state.attempt()!!))
        state.interrupt()
        assertEquals(1_000L, state.delayMillis)
    }

    @Test fun logoutOrDeviceSwitchFencesLateSuccessAndAuthenticationFailure() {
        val state = WorkspaceReconnect()
        state.interrupt()
        val old = state.attempt()!!
        state.cancel()
        state.interrupt()
        val current = state.attempt()!!
        assertFalse(state.complete(old))
        assertFalse(state.fail(old, WorkspaceReconnect.Blocked.AUTHENTICATION))
        assertNull(state.blocked)
        assertTrue(state.isCurrent(current))
        assertTrue(state.complete(current))
    }

    @Test fun backgroundingInvalidatesInflightAttemptButKeepsRecoveryPending() {
        val state = WorkspaceReconnect()
        state.interrupt()
        val old = state.attempt()!!
        state.suspend()
        assertTrue(state.pending)
        assertFalse(state.inFlight)
        assertFalse(state.complete(old))
        assertFalse(state.fail(old, null))
        assertTrue(state.complete(state.attempt()!!))
    }

    @Test fun authenticationAndSecurityFailuresStopAutomaticRetry() {
        val state = WorkspaceReconnect()
        state.interrupt()
        assertTrue(state.fail(state.attempt()!!, WorkspaceReconnect.Blocked.AUTHENTICATION))
        assertTrue(state.pending)
        assertNull(state.attempt())
        state.suspend()
        assertNull("Foregrounding must not retry revoked authentication", state.attempt())
    }

    @Test fun timeoutAndReadOnlyErrorsAreNotAuthenticationFailures() {
        assertEquals(WorkspaceReconnect.Blocked.AUTHENTICATION, WorkspaceReconnect.blockedBy("HTTP status client error (401 Unauthorized) for url (https://example.test/v2/auth/refresh)"))
        assertEquals(WorkspaceReconnect.Blocked.AUTHENTICATION, WorkspaceReconnect.blockedBy("not logged in"))
        assertEquals(WorkspaceReconnect.Blocked.ACCESS, WorkspaceReconnect.blockedBy("HTTP status client error (403 Forbidden)"))
        assertEquals(WorkspaceReconnect.Blocked.ACCESS, WorkspaceReconnect.blockedBy("device public key changed; revoke and enroll the device again"))
        for (message in listOf("offline", "not connected", "request timeout", "Desktop offline", "terminal is currently read-only", "session exited", "HTTP status server error (503 Service Unavailable)", "request queue full or closed")) {
            assertNull(message, WorkspaceReconnect.blockedBy(message))
        }
    }
}
