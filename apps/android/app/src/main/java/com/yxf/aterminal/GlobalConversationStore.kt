package com.yxf.aterminal

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/** Device/account-scoped list cache and local reading watermarks; history remains on Desktop. */
class GlobalConversationStore(context: Context, identity: List<String>) {
    private val prefs = context.getSharedPreferences("global-conversations-${ChatStore.digest(JSONArray(identity).toString())}", Context.MODE_PRIVATE)
    fun rows(): List<JSONObject> = prefs.all.filterKeys { it.startsWith("row:") }.values.filterIsInstance<String>()
        .mapNotNull { runCatching { JSONObject(it) }.getOrNull() }.sortedByDescending { it.optLong("updated_at") }
    fun merge(rows: List<JSONObject>) {
        val edit = prefs.edit()
        rows.forEach { edit.putString("row:${it.getJSONObject("scope").getString("agent")}", it.toString()) }
        edit.apply()
    }
    fun unread(row: JSONObject): Boolean = row.optLong("last_reply_sequence") > prefs.getLong("read:${row.getJSONObject("scope").getString("agent")}", 0)
    fun read(id: String, sequence: Long) {
        val key = "read:$id"
        if (sequence > prefs.getLong(key, 0)) prefs.edit().putLong(key, sequence).apply()
    }
    var scroll: Int
        get() = prefs.getInt("scroll", 0)
        set(value) { prefs.edit().putInt("scroll", value).apply() }
    var pendingCreate: String
        get() = prefs.getString("pending-create", "").orEmpty()
        set(value) { prefs.edit().putString("pending-create", value).apply() }
    companion object {
        fun running(state: String) = state in setOf("running", "monitoring", "stopping", "cancelling")
        fun stateLabel(state: String) = when (state) {
            "running", "monitoring" -> "执行中"
            "stopping", "cancelling" -> "正在停止"
            "completed" -> "✓ 已完成"
            "failed" -> "执行失败"
            "cancelled" -> "已停止"
            "orphaned" -> "任务已中断"
            "paused" -> "等待处理"
            else -> "— 就绪"
        }
    }
}
