package com.yxf.aterminal

import org.junit.Assert.*
import org.junit.Test
import java.io.IOException

class AgentReadRetryTest {
    @Test fun transientReadsRetryWithBoundedExponentialDelay() {
        val delays = mutableListOf<Long>(); var calls = 0
        val value = AgentReadRetry.run("history", { true }, { delays.add(it) }, { it / 4 }) {
            if (++calls < 3) throw IOException("request timeout")
            "page"
        }
        assertEquals("page", value); assertEquals(3, calls)
        assertEquals(listOf(375L, 750L), delays)
    }

    @Test fun exhaustedReadsDoNotContinueRetrying() {
        var calls = 0; val delays = mutableListOf<Long>()
        assertThrows(IOException::class.java) {
            AgentReadRetry.run("record", { true }, { delays.add(it) }, { 0L }) { calls++; throw IOException("connection reset") }
        }
        assertEquals(3, calls); assertEquals(listOf(300L, 600L), delays)
    }

    @Test fun staleConnectionStopsBeforeAnotherAttempt() {
        var current = true; var calls = 0
        assertThrows(IllegalStateException::class.java) {
            AgentReadRetry.run("state", { current }, { current = false }, { 0L }) { calls++; throw IOException("request timeout") }
        }
        assertEquals(1, calls)
    }

    @Test fun permanentErrorsAndUnknownErrorsAreNotRetried() {
        for (message in listOf("401 Unauthorized", "403 Forbidden", "permission denied", "invalid history cursor", "history_generation_stale", "unexpected response")) {
            var calls = 0
            assertThrows(IllegalStateException::class.java) {
                AgentReadRetry.run("history", { true }, { fail("Permanent error slept: $message") }) { calls++; error(message) }
            }
            assertEquals(message, 1, calls)
        }
    }

    @Test fun mutationsNeverReachTheRetryExecutor() {
        for (action in listOf("send", "cancel", "resolve", "set_permissions", "image_chunk", "global_create")) {
            var called = false
            assertThrows(IllegalArgumentException::class.java) { AgentReadRetry.run(action, { true }) { called = true } }
            assertFalse(action, called)
        }
    }

    @Test fun lateSuccessCannotCrossConnectionIdentity() {
        var current = true
        assertThrows(IllegalStateException::class.java) {
            AgentReadRetry.run("record", { current }) { current = false; "old result" }
        }
    }

    @Test fun interruptionDuringBackoffPreservesThreadCancellation() {
        try {
            assertThrows(InterruptedException::class.java) {
                AgentReadRetry.run("state", { true }, { throw InterruptedException("cancel") }) { throw IOException("timeout") }
            }
            assertTrue(Thread.currentThread().isInterrupted)
        } finally { Thread.interrupted() }
    }
}
