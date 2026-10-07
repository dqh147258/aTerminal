package com.yxf.aterminal

/** UI-thread retry state, separate from navigation/session identity. No user operation is queued. */
internal class WorkspaceReconnect {
    enum class Blocked { AUTHENTICATION, ACCESS, LEGACY_PAIRING }

    @Volatile var epoch = 0
        private set
    var pending = false
        private set
    var inFlight = false
        private set
    var blocked: Blocked? = null
        private set
    private var failures = 0
    val delayMillis: Long get() = minOf(1_000L shl failures.coerceAtMost(4), 15_000L)

    fun interrupt(): Boolean {
        if (pending) return false
        epoch++; pending = true; inFlight = false; blocked = null; failures = 0
        return true
    }

    fun attempt(): Int? {
        if (!pending || inFlight || blocked != null) return null
        inFlight = true
        return ++epoch
    }

    fun isCurrent(ticket: Int) = pending && inFlight && epoch == ticket

    fun fail(ticket: Int, reason: Blocked?): Boolean {
        if (!isCurrent(ticket)) return false
        inFlight = false; blocked = reason; failures = (failures + 1).coerceAtMost(4)
        return true
    }

    fun complete(ticket: Int): Boolean {
        if (!isCurrent(ticket)) return false
        pending = false; inFlight = false; blocked = null; failures = 0
        return true
    }

    fun suspend() { epoch++; inFlight = false }
    fun cancel() { epoch++; pending = false; inFlight = false; blocked = null; failures = 0 }

    companion object {
        // Core currently exposes textual errors. Match explicit account HTTP/auth failures only;
        // an ordinary timeout, read-only terminal or offline Desktop must never log the user out.
        fun blockedBy(message: String): Blocked? = when {
            message.contains("401 Unauthorized", true) || message == "not logged in" -> Blocked.AUTHENTICATION
            message.contains("403 Forbidden", true) || message.contains("device public key changed", true) ||
                message.contains("invalid connection grant", true) || message.contains("refresh changed account identity", true) -> Blocked.ACCESS
            else -> null
        }
    }
}
