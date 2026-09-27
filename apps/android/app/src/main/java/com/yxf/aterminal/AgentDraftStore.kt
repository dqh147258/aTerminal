package com.yxf.aterminal

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/** Only metadata lives in preferences; attachment bytes stay in private, owner-scoped files. */
class AgentDraftStore(private val context: Context, identity: List<String>) {
    private val namespace = ChatStore.digest(JSONArray(identity).toString())
    private val prefs = context.getSharedPreferences("agent-drafts-$namespace", Context.MODE_PRIVATE)
    val directory = File(context.filesDir, "agent-attachments/$namespace").apply { mkdirs() }
    init {
        val referenced = prefs.all.values.filterIsInstance<String>().flatMap { raw ->
            runCatching { JSONObject(raw).optJSONArray("images")?.let { rows -> (0 until rows.length()).map { rows.getJSONObject(it).getString("file") } }.orEmpty() }.getOrDefault(emptyList())
        }.toSet()
        val now = System.currentTimeMillis()
        directory.listFiles().orEmpty().filter { !it.name.startsWith("record-") && it.name !in referenced && now - it.lastModified() > 86400000 }.forEach { it.delete() }
        val records = directory.listFiles().orEmpty().filter { it.name.startsWith("record-") }.sortedByDescending { it.lastModified() }
        var bytes = 0L
        records.forEach { bytes += it.length(); if (bytes > 64L * 1024 * 1024) it.delete() }
    }
    fun read(session: String): JSONObject = runCatching { JSONObject(prefs.getString(ChatStore.digest(session), "{}")!!) }.getOrDefault(JSONObject())
    fun save(session: String, value: JSONObject) { prefs.edit().putString(ChatStore.digest(session), value.toString()).apply() }
    fun migrateGlobal(id: String) {
        val marker = "migrated-global-$id"
        if (prefs.getBoolean(marker, false)) return
        val target = ChatStore.digest("global:$id")
        val edit = prefs.edit()
        if (!prefs.contains(target)) prefs.getString(ChatStore.digest(""), null)?.let { edit.putString(target, it) }
        edit.putBoolean(marker, true).apply()
    }
    fun search(session: String, query: String) = read(session).optString("search").contains(query, true)
    fun file(name: String): File { require(name.matches(Regex("[a-zA-Z0-9.-]+"))); return File(directory, name) }
}
