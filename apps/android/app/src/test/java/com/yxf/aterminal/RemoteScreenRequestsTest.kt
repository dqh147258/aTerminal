package com.yxf.aterminal

import org.junit.Assert.*
import org.junit.Test
import java.util.ArrayDeque

class RemoteScreenRequestsTest {
    private class Harness {
        val work = ArrayDeque<() -> Unit>()
        val ui = ArrayDeque<() -> Unit>()
        val discarded = mutableListOf<String>()
        val received = mutableListOf<Result<String>>()
        val timers = mutableListOf<Timer>()
        data class Timer(val delay: Long, val action: () -> Unit, var cancelled: Boolean = false)
        val requests = RemoteScreenRequests<String>(
            { work.add(it) }, { ui.add(it) },
            { delay, action ->
                val timer = Timer(delay, action); timers.add(timer)
                val cancel: () -> Unit = { timer.cancelled = true }
                cancel
            }, { discarded.add(it) }
        )
        fun start(value: String, repeat: Boolean = true) = requests.start(if (repeat) 1_000L else null, { value }, { received.add(it) })
        fun finish() { work.removeFirst()(); ui.removeFirst()() }
        fun tick() { val timer = timers.removeAt(0); if (!timer.cancelled) timer.action() }
    }

    @Test fun slowCaptureCannotOverlapOrAccumulateTimerRequests() {
        val h = Harness(); h.start("frame")
        assertEquals(1, h.work.size); assertTrue(h.timers.isEmpty())
        h.work.removeFirst()()
        assertTrue("Still waiting for UI to accept frame", h.timers.isEmpty())
        h.ui.removeFirst()()
        assertEquals(1_000L, h.timers.single().delay)
        h.tick(); assertEquals(1, h.work.size)
        assertTrue(h.timers.isEmpty()); h.finish()
        assertEquals(2, h.received.size)
    }

    @Test fun rapidScreenDeviceOrAccountChangesOnlyQueueTheLatestRequest() {
        val h = Harness(); h.start("old")
        h.work.removeFirst()() // An old frame is decoded but its UI callback has not run.
        repeat(30) { h.start("new-$it") }
        assertTrue(h.work.isEmpty())
        h.ui.removeFirst()()
        assertEquals(listOf("old"), h.discarded)
        assertEquals(1, h.work.size)
        h.finish()
        assertEquals(listOf("new-29"), h.received.map { it.getOrThrow() })
    }

    @Test fun closingWithACompletedDecodeDiscardsItAndCannotReviveThePage() {
        val h = Harness(); h.start("closed-frame")
        h.work.removeFirst()(); h.requests.stop(); h.ui.removeFirst()()
        assertEquals(listOf("closed-frame"), h.discarded)
        assertTrue(h.received.isEmpty()); assertTrue(h.timers.isEmpty()); assertTrue(h.work.isEmpty())
    }

    @Test fun backgroundOrReconnectStopCancelsTheScheduledCapture() {
        val h = Harness(); h.start("frame"); h.finish()
        h.requests.stop(); h.tick()
        assertTrue(h.work.isEmpty())
        h.start("restored"); h.finish()
        assertEquals(listOf("frame", "restored"), h.received.map { it.getOrThrow() })
    }

    @Test fun closeAndReopenBeforeAnOldRpcStartsSkipsTheOldRpc() {
        val h = Harness(); var oldCalls = 0
        h.requests.start(1_000L, { oldCalls++; "old" }, { h.received.add(it) })
        h.requests.stop(); h.start("reopened")
        assertEquals(1, h.work.size)
        h.finish(); assertEquals(0, oldCalls)
        h.finish()
        assertEquals(listOf("reopened"), h.received.map { it.getOrThrow() })
    }

    @Test fun lateFailureDoesNotReplaceTheNewScreensState() {
        val h = Harness()
        h.requests.start(1_000L, { throw IllegalStateException("old transport timeout") }, { h.received.add(it) })
        h.work.removeFirst()(); h.start("new")
        h.ui.removeFirst()(); h.finish()
        assertEquals(listOf("new"), h.received.map { it.getOrThrow() })
    }

    @Test fun visibleFailureCanStopPollingAndExplicitRetryWorks() {
        val h = Harness()
        h.requests.start(1_000L, { error("permission denied") }) { result ->
            h.received.add(result); h.requests.stop()
        }
        h.finish()
        assertTrue(h.received.single().isFailure); assertTrue(h.timers.isEmpty())
        h.start("retried"); h.finish()
        assertEquals("retried", h.received.last().getOrThrow())
    }

    @Test fun listLoadingIsOneShotAndStaleWorkValidityIsRevoked() {
        val h = Harness(); var valid: (() -> Boolean)? = null
        h.requests.start(work = { current -> valid = current; "list" }, result = { h.received.add(it) })
        h.finish()
        assertTrue(valid!!()); assertTrue(h.timers.isEmpty()); assertTrue(h.work.isEmpty())
        h.requests.stop(); assertFalse(valid!!())
    }
}
