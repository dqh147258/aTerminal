package dev.aiterminal.app

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.net.URI
import java.security.MessageDigest

class DisplayPreferences(context: Context, namespace: String = "display") {
    private val prefs = context.getSharedPreferences(namespace, Context.MODE_PRIVATE)
    var fontSize: Int
        get() = prefs.getInt("font", 16).coerceIn(12, 24)
        set(value) { prefs.edit().putInt("font", value.coerceIn(12, 24)).apply() }
    var opacity: Int
        get() = prefs.getInt("opacity", 88).coerceIn(60, 96)
        set(value) { prefs.edit().putInt("opacity", value.coerceIn(60, 96)).apply() }
    fun reset() { prefs.edit().clear().apply() }
}

data class ChatMessage(val role: String, val content: String, val time: Long = System.currentTimeMillis(), val source: String = "", val revision: Long = 0) {
    fun json() = JSONObject().put("role", role).put("content", content).put("time", time).put("source", source).put("revision", revision)
}

data class Conversation(
    val deviceId: String,
    val sessionId: String,
    var title: String,
    val messages: MutableList<ChatMessage> = mutableListOf(),
    var draft: String = "",
    var requestId: String = "",
    var state: String = "idle",
    var detail: String = "",
    var updated: Long = System.currentTimeMillis(),
    val eventCursors: MutableMap<String, Long> = mutableMapOf(),
    val repliedRequests: MutableSet<String> = mutableSetOf(),
    var monitoring: Boolean = false
) {
    fun json() = JSONObject().put("device", deviceId).put("session", sessionId).put("title", title)
        .put("messages", JSONArray(messages.map { it.json() })).put("draft", draft)
        .put("request", requestId).put("state", state).put("detail", detail).put("updated", updated)
        .put("event_cursors", JSONObject(eventCursors.toMap())).put("replied", JSONArray(repliedRequests.toList())).put("monitoring", monitoring)
    companion object {
        fun from(json: JSONObject): Conversation {
            val messages = json.optJSONArray("messages") ?: JSONArray()
            return Conversation(json.getString("device"), json.getString("session"), json.optString("title"),
                (0 until messages.length()).map { messages.getJSONObject(it).let { item ->
                    ChatMessage(item.getString("role"), item.getString("content"), item.optLong("time"), item.optString("source"), item.optLong("revision"))
                } }.toMutableList(), json.optString("draft"), json.optString("request"),
                json.optString("state", "idle"), json.optString("detail"), json.optLong("updated"),
                (json.optJSONObject("event_cursors") ?: JSONObject()).let { cursors -> cursors.keys().asSequence().associateWith { cursors.getLong(it) }.toMutableMap() },
                (json.optJSONArray("replied") ?: JSONArray()).let { items -> (0 until items.length()).map { items.getString(it) }.toMutableSet() }, json.optBoolean("monitoring"))
        }
    }
}

/** The owner namespace is never selected from a visible session title or a device name. */
class ChatStore(context: Context, server: String, username: String) {
    private val prefs = context.getSharedPreferences("chats-" + digest(canonicalServer(server) + "\u0000" + username), Context.MODE_PRIVATE)
    fun get(device: String, session: String, title: String): Conversation = prefs.getString(key(device, session), null)?.let {
        Conversation.from(JSONObject(it))
    } ?: Conversation(device, session, title)
    fun save(chat: Conversation) {
        chat.updated = System.currentTimeMillis()
        check(prefs.edit().putString(key(chat.deviceId, chat.sessionId), chat.json().toString()).commit())
    }
    fun saveDraft(chat: Conversation) {
        val latest = get(chat.deviceId, chat.sessionId, chat.title)
        latest.draft = chat.draft; save(latest)
    }
    fun response(device: String, session: String, title: String, result: JSONObject, expectedId: String? = null): Conversation {
        val latest = get(device, session, title)
        val incomingId = result.optString("request_id")
        if (expectedId != null && (latest.requestId != expectedId || (incomingId.isNotEmpty() && incomingId != expectedId))) return latest
        if (incomingId.isEmpty() && result.optString("state") == "idle" && latest.requestId.isNotEmpty() && latest.state in setOf("completed", "failed", "stopped")) return latest
        if (incomingId.isNotEmpty()) latest.requestId = incomingId
        latest.state = result.optString("state", "failed"); latest.detail = result.optString("message")
        latest.monitoring = result.optBoolean("monitoring", latest.state == "monitoring")
        val events = result.optJSONArray("events") ?: JSONArray()
        if (events.length() > 0 && latest.requestId.isNotEmpty()) {
            var cursor = latest.eventCursors[latest.requestId] ?: 0L
            for (index in 0 until events.length()) {
                val event = events.getJSONObject(index); val id = event.optLong("id")
                if (id > cursor) {
                    val text = event.optString("text")
                    if (text.isNotEmpty()) latest.messages.add(ChatMessage("assistant", text, source = event.optString("kind"), revision = event.optLong("revision")))
                    cursor = id
                }
            }
            latest.eventCursors[latest.requestId] = cursor
            latest.repliedRequests.add(latest.requestId)
        } else if (latest.state == "completed" && latest.requestId.isNotEmpty() && latest.requestId !in latest.repliedRequests) {
            val reply = result.optString("reply")
            if (reply.isNotEmpty()) { latest.messages.add(ChatMessage("assistant", reply)); latest.repliedRequests.add(latest.requestId) }
        }
        save(latest); return latest
    }
    fun list(query: String = ""): List<Conversation> = prefs.all.values.filterIsInstance<String>().map {
        Conversation.from(JSONObject(it))
    }.filter { chat -> chat.messages.isNotEmpty() && (query.isBlank() || chat.title.contains(query, true) || chat.messages.any { it.content.contains(query, true) }) }
        .sortedByDescending { it.updated }
    fun clear() { check(prefs.edit().clear().commit()) }
    private fun key(device: String, session: String) = digest(device + "\u0000" + session)
    companion object {
        fun canonicalServer(value: String): String {
            val uri = URI(value.trim())
            require(uri.scheme in listOf("http", "https") && !uri.host.isNullOrBlank() && uri.userInfo == null && uri.query == null && uri.fragment == null) { "请输入有效的服务地址" }
            return URI(uri.scheme.lowercase(), null, uri.host.lowercase(), uri.port, uri.path.trimEnd('/'), null, null).toString()
        }
        fun digest(value: String) = MessageDigest.getInstance("SHA-256").digest(value.toByteArray()).joinToString("") { "%02x".format(it) }
    }
}

object AssistantRequest {
    val pollingStates = setOf("running", "monitoring", "stopping", "unknown")
    fun send(id: String, message: String, includeScreen: Boolean, history: List<ChatMessage>, allowInput: Boolean = true, monitor: Boolean = includeScreen): String {
        require(message.isNotBlank() && message.length <= 4000) { "消息须为 1-4000 个字符" }
        val context = history.filter { it.role == "user" || it.role == "assistant" }.takeLast(12).toMutableList()
        fun payload() = JSONObject().put("action", "send").put("request_id", id).put("message", message)
            .put("include_screen", includeScreen).put("allow_input", allowInput).put("monitor", monitor)
            .put("messages", JSONArray(context.map { JSONObject().put("role", it.role).put("content", it.content) })).toString()
        var result = payload()
        while (result.toByteArray(Charsets.UTF_8).size >= 16000 && context.isNotEmpty()) {
            context.removeAt(0); result = payload()
        }
        require(result.toByteArray(Charsets.UTF_8).size < 16000) { "消息编码后过长，请缩短内容" }
        return result
    }
}

class WorkspaceMemory(context: Context, server: String, username: String) {
    private val prefs = context.getSharedPreferences("workspace-" + ChatStore.digest(ChatStore.canonicalServer(server) + "\u0000" + username), Context.MODE_PRIVATE)
    fun remember(device: String, session: String) { prefs.edit().putString("device", device).putString("session", session).apply() }
    fun last(): Pair<String, String>? {
        val device = prefs.getString("device", null) ?: return null
        return device to (prefs.getString("session", null) ?: return null)
    }
    fun record(device: String, sessions: Map<String, Boolean>) {
        val key = "sessions-" + ChatStore.digest(device)
        val known = JSONObject(prefs.getString(key, "{}")!!)
        known.keys().asSequence().toList().forEach { if (it !in sessions) known.put(it, true) }
        sessions.forEach { (id, exited) -> known.put(id, exited) }
        prefs.edit().putString(key, known.toString()).apply()
    }
    fun closed(device: String, session: String) = JSONObject(prefs.getString("sessions-" + ChatStore.digest(device), "{}")!!).optBoolean(session, false)
    fun clear() { prefs.edit().clear().commit() }
}
