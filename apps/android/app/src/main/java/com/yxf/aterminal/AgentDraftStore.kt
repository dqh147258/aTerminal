package com.yxf.aterminal

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/** Only metadata lives in preferences; attachment bytes stay in private, owner-scoped files. */
class AgentDraftStore(private val context: Context, identity: List<String>) {
    private val namespace = ChatStore.digest(JSONArray(identity).toString())
    private val prefs = context.getSharedPreferences("agent-drafts-$namespace", Context.MODE_PRIVATE)
    // SharedPreferences rewrites its whole XML file. Keep small, frequent edits separate
    // from retained history so typing and persisting a send cannot rewrite large replies.
    private val edits = context.getSharedPreferences("agent-composer-$namespace", Context.MODE_PRIVATE)
    val directory = File(context.filesDir, "agent-attachments/$namespace").apply { mkdirs() }
    init {
        val referenced = (prefs.all.values + edits.all.values).filterIsInstance<String>().flatMap { raw ->
            runCatching { JSONObject(raw).optJSONArray("images")?.let { rows -> (0 until rows.length()).map { rows.getJSONObject(it).getString("file") } }.orEmpty() }.getOrDefault(emptyList())
        }.toSet()
        val now = System.currentTimeMillis()
        directory.listFiles().orEmpty().filter { !it.name.startsWith("record-") && it.name !in referenced && now - it.lastModified() > 86400000 }.forEach { it.delete() }
        val records = directory.listFiles().orEmpty().filter { it.name.startsWith("record-") }.sortedByDescending { it.lastModified() }
        var bytes = 0L
        records.forEach { bytes += it.length(); if (bytes > 64L * 1024 * 1024) it.delete() }
    }
    private val composerFields = setOf("text", "images", "request_id")
    private val viewFields = setOf("run_state", "scroll", "cursor", "has_more", "generation")
    private fun composerKey(session: String) = "composer:${ChatStore.digest(session)}"
    private fun viewKey(session: String) = "view:${ChatStore.digest(session)}"
    private fun json(key: String, store: android.content.SharedPreferences = prefs): JSONObject = runCatching { JSONObject(store.getString(key, "{}")!!) }.getOrDefault(JSONObject())
    private fun subset(value: JSONObject, keys: Set<String>) = JSONObject().also { result -> keys.forEach { if (value.has(it)) result.put(it, value.get(it)) } }
    private fun write(key: String, value: JSONObject, store: android.content.SharedPreferences = prefs) {
        val text = value.toString()
        if (store.getString(key, null) != text) store.edit().putString(key, text).apply()
    }
    /** Legacy combined snapshots remain readable; new composer edits never parse history. */
    fun read(session: String): JSONObject = json(ChatStore.digest(session)).also { value ->
        for (part in listOf(json(viewKey(session), edits), json(composerKey(session), edits))) part.keys().forEach { value.put(it, part.get(it)) }
    }
    fun readComposer(session: String): JSONObject = if (edits.contains(composerKey(session))) json(composerKey(session), edits)
        else subset(json(ChatStore.digest(session)), composerFields)
    fun saveComposer(session: String, value: JSONObject) = write(composerKey(session), JSONObject()
        .put("text", value.optString("text")).put("images", value.optJSONArray("images") ?: JSONArray()).put("request_id", value.optString("request_id")), edits)
    fun saveView(session: String, value: JSONObject) {
        val current = if (edits.contains(viewKey(session))) json(viewKey(session), edits) else subset(json(ChatStore.digest(session)), viewFields)
        viewFields.forEach { if (value.has(it)) current.put(it, value.get(it)) }
        write(viewKey(session), current, edits)
    }
    fun saveHistory(session: String, items: JSONArray, search: String) {
        if (!edits.contains(composerKey(session))) saveComposer(session, readComposer(session))
        if (!edits.contains(viewKey(session))) saveView(session, json(ChatStore.digest(session)))
        write(ChatStore.digest(session), JSONObject().put("items", items).put("search", search))
    }
    /** Compatibility entry point for complete snapshots and archive migration. */
    fun save(session: String, value: JSONObject) {
        if (composerFields.any { value.has(it) }) saveComposer(session, value)
        saveView(session, value)
        val history = JSONObject()
        value.keys().forEach { if (it !in composerFields && it !in viewFields) history.put(it, value.get(it)) }
        write(ChatStore.digest(session), history)
    }
    private fun pendingKey(session: String) = "pending:${ChatStore.digest(session)}"
    fun pending(session: String): JSONObject? = edits.getString(pendingKey(session), null)?.let { runCatching { JSONObject(it) }.getOrNull() }
    fun savePending(session: String, value: JSONObject) {
        val old = pending(session)
        check(old == null || old.optString("request_id") == value.optString("request_id")) { "请先确认上一条发送结果" }
        // Persist the immutable send before any RPC, including while the editable draft changes.
        check(edits.edit().putString(pendingKey(session), value.toString()).commit()) { "待发送消息保存失败，请重试" }
    }
    fun clearPending(session: String, requestId: String) {
        val pending = pending(session) ?: return
        if (pending.optString("request_id") != requestId) return
        edits.edit().remove(pendingKey(session)).apply()
        pending.optJSONArray("images")?.let { images -> (0 until images.length()).forEach { deleteIfUnreferenced(images.getJSONObject(it).getString("file")) } }
    }
    fun deleteIfUnreferenced(name: String) {
        val referenced = (prefs.all.values + edits.all.values).filterIsInstance<String>().any { raw ->
            runCatching { JSONObject(raw).optJSONArray("images")?.let { images ->
                (0 until images.length()).any { images.getJSONObject(it).optString("file") == name }
            } == true }.getOrDefault(false)
        }
        if (!referenced) file(name).delete()
    }
    /** Acknowledgements belong to an immutable scope/request, independently of its current view. */
    fun acknowledge(session: String, requestId: String, state: String): Boolean {
        val saved = readComposer(session)
        if (saved.optString("request_id") != requestId) return false
        val images = saved.optJSONArray("images") ?: JSONArray()
        val files = (0 until images.length()).map { images.getJSONObject(it).getString("file") }
        saved.put("text", "").put("images", JSONArray()).put("request_id", java.util.UUID.randomUUID().toString())
        saveComposer(session, saved); saveView(session, JSONObject().put("run_state", state))
        files.forEach { deleteIfUnreferenced(it) }
        return true
    }
    fun migrateGlobal(id: String) {
        val marker = "migrated-global-$id"
        if (prefs.getBoolean(marker, false)) return
        val edit = prefs.edit()
        val destination = "global:$id"
        if (!prefs.contains(ChatStore.digest(destination))) prefs.getString(ChatStore.digest(""), null)?.let { edit.putString(ChatStore.digest(destination), it) }
        val local = edits.edit()
        for ((source, target) in listOf(composerKey("") to composerKey(destination), viewKey("") to viewKey(destination), pendingKey("") to pendingKey(destination))) {
            if (!edits.contains(target)) edits.getString(source, null)?.let { local.putString(target, it) }
        }
        local.apply()
        edit.putBoolean(marker, true).apply()
    }
    fun search(session: String, query: String) = json(ChatStore.digest(session)).optString("search").contains(query, true)
    fun file(name: String): File { require(name.matches(Regex("[a-zA-Z0-9.-]+"))); return File(directory, name) }
}
