package com.yxf.aterminal

/** Called on the UI thread, except work and the supplied validity check. One gate per Activity. */
internal class RemoteScreenRequests<T>(
    private val execute: (() -> Unit) -> Unit,
    private val post: (() -> Unit) -> Unit,
    private val schedule: (Long, () -> Unit) -> (() -> Unit),
    private val discard: (T) -> Unit = {}
) {
    @Volatile private var epoch = 0L
    private var busy = false
    private var cancelScheduled: (() -> Unit)? = null
    private var request: ((() -> Boolean) -> T)? = null
    private var receive: ((Result<T>) -> Unit)? = null
    private var interval: Long? = null

    fun start(intervalMillis: Long? = null, work: (() -> Boolean) -> T, result: (Result<T>) -> Unit) {
        stop()
        interval = intervalMillis
        request = work; receive = result
        launch()
    }

    fun stop() {
        epoch++
        cancelScheduled?.invoke(); cancelScheduled = null
        request = null; receive = null
        // Keep busy until the old task drains. Closing/reopening cannot stack queued RPCs.
    }

    private fun launch() {
        if (busy) return
        val work = request ?: return
        val ticket = epoch
        busy = true
        execute {
            val value = runCatching {
                check(ticket == epoch) { "屏幕请求已取消" }
                work { ticket == epoch }
            }
            post {
                busy = false
                if (ticket != epoch) {
                    value.getOrNull()?.let(discard)
                    launch()
                } else {
                    val callback = receive
                    if (interval == null) { request = null; receive = null }
                    callback?.invoke(value)
                    if (ticket == epoch && request != null) {
                        cancelScheduled = schedule(interval ?: 0L) {
                            cancelScheduled = null
                            launch()
                        }
                    }
                }
            }
        }
    }
}
