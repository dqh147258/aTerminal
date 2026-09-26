package com.yxf.aterminal

import org.json.JSONObject

/** Presentation-only compaction: original Desktop events and page cursors stay intact. */
object AgentTimeline {
    fun text(item: JSONObject): String {
        val value = item.optJSONObject("value") ?: JSONObject()
        val kind = item.optString("kind")
        val summary = value.optJSONArray("updates")?.let { rows ->
            (rows.length() - 1 downTo 0).firstNotNullOfOrNull { rows.optJSONObject(it)?.optString("summary")?.takeIf(String::isNotEmpty) }
        }.orEmpty()
        return value.optString("message").ifEmpty { value.optString("text") }.ifEmpty { value.optString("summary") }.ifEmpty { summary }.ifEmpty {
            if (kind.startsWith("pty_status")) "终端状态：" + (value.optJSONObject("session_process")?.optString("state") ?: value.optString("state", "已更新"))
            else if (kind == "interaction") "终端或扩展操作记录，可查看关联证据。" else "Agent 状态：" + value.optString("state", "已更新")
        }
    }
    /** Input pages are newest first. Compare chronologically, including across page edges. */
    fun visible(newestFirst: List<JSONObject>, browsing: Boolean): List<JSONObject> {
        val last = mutableMapOf<String, String>()
        val chronological = newestFirst.distinctBy { it.optString("id") }.reversed().filter { item ->
            if (!item.optString("kind").startsWith("pty_status")) true
            else {
                val key = item.optJSONObject("value")?.optString("session_id").orEmpty()
                last.put(key, text(item)) != text(item)
            }
        }
        return if (browsing) chronological.reversed() else chronological
    }
}
