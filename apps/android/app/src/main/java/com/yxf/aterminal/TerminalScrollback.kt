package com.yxf.aterminal

import android.os.Handler
import java.util.concurrent.ExecutorService
import uniffi.ai_terminal_mobile.*

/** UI-thread controller. Pending gestures are coalesced while one immutable viewport is read. */
class TerminalScrollback(
    private var remote: RemoteTerminal, private val worker: ExecutorService, private val ui: Handler,
    private val current: () -> Boolean, private val display: (RenderFrame?) -> Unit,
    private val status: (String?) -> Unit, private val failed: (String) -> Unit
) {
    private var cursor: TerminalHistoryCursor? = null
    private var pending = 0
    private var loading = false
    private var version = 0
    private var retainedOffset = 0u
    val reading get() = cursor != null || loading || retainedOffset > 0u
    private fun release(value: TerminalHistoryCursor?, transport: RemoteTerminal = remote) {
        if (value != null && !worker.isShutdown) worker.execute { runCatching { transport.releaseHistory(value) } }
    }
    fun suspendTransport() {
        version++; pending = 0; loading = false
        retainedOffset = cursor?.offset ?: retainedOffset
        release(cursor); cursor = null
        // Keep the immutable history frame visible; old server cursors cannot cross a channel.
    }
    fun resumeTransport(transport: RemoteTerminal) {
        remote = transport
    }
    fun live() {
        val wasReading = reading
        version++; pending = 0; loading = false; retainedOffset = 0u
        release(cursor); cursor = null
        if (wasReading) display(null)
        status(null)
    }
    fun scroll(lines: Int) {
        if (!current()) return
        pending += lines
        drain()
    }
    private fun drain() {
        if (loading || pending == 0 || !current()) return
        val target = ((cursor?.offset ?: retainedOffset).toLong() + pending).coerceIn(0, UInt.MAX_VALUE.toLong()).toUInt()
        pending = 0
        if (target == 0u) { live(); return }
        loading = true
        val requestVersion = version; val prior = cursor; val transport = remote
        worker.execute {
            try {
                val page = transport.readHistoryViewport(prior, target)
                ui.post {
                    if (!current() || requestVersion != version) { release(page.cursor, transport); return@post }
                    loading = false; cursor = page.cursor; retainedOffset = 0u
                    if (page.cursor.offset == 0u) { live(); return@post }
                    display(page.frame); status("历史 ${page.cursor.offset} / ${page.total} 行 · 输入返回实时")
                    drain()
                }
            } catch (error: Exception) {
                ui.post { if (current() && requestVersion == version) {
                    live(); failed("历史滚动失败：${error.message}")
                } }
            }
        }
    }
}
