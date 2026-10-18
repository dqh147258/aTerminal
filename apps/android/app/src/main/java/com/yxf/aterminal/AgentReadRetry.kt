package com.yxf.aterminal

import java.util.concurrent.ThreadLocalRandom

/** Only repeat observations. A lost acknowledgement must never replay a user operation. */
internal object AgentReadRetry {
    private val actions = setOf("state", "history", "record", "context", "permissions", "pending", "rules", "approval_details", "global_list")

    fun transient(error: Exception): Boolean {
        val text = generateSequence<Throwable>(error) { it.cause }.take(5).joinToString(" ") { it.message.orEmpty() }.lowercase()
        if (listOf("401", "403", "unauthorized", "forbidden", "permission", "invalid", "revoked", "not logged in", "account changed").any { text.contains(it) }) return false
        return listOf("timeout", "timed out", "connection reset", "connection closed", "connection aborted", "broken pipe",
            "temporarily unavailable", "service unavailable", "502 bad gateway", "503", "504", "request queue full", "offline", "not connected").any { text.contains(it) }
    }

    fun <T> run(action: String, current: () -> Boolean,
        pause: (Long) -> Unit = { Thread.sleep(it) },
        jitter: (Long) -> Long = { base -> ThreadLocalRandom.current().nextLong(base / 4 + 1) },
        request: () -> T): T {
        require(action in actions) { "Only read-only Agent requests may retry" }
        repeat(3) { attempt ->
            check(current()) { "连接或对话已变化" }
            try {
                val value = request()
                check(current()) { "连接或对话已变化" }
                return value
            } catch (error: Exception) {
                if (error is InterruptedException) { Thread.currentThread().interrupt(); throw error }
                if (attempt == 2 || !current() || !transient(error)) throw error
                val base = 300L shl attempt
                // Short sleeps only run on a background reader; recheck the lease before retrying.
                try { pause(base + jitter(base).coerceIn(0, base / 4)) }
                catch (interrupted: InterruptedException) { Thread.currentThread().interrupt(); throw interrupted }
            }
        }
        error("Read retry exhausted")
    }
}
