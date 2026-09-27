package com.yxf.aterminal

import org.json.JSONObject

/** Presentation-only compaction: original Desktop events and page cursors stay intact. */
object AgentTimeline {
    fun text(item: JSONObject): String {
        val value = item.optJSONObject("value") ?: JSONObject()
        val kind = item.optString("kind")
        if (kind == "user" && value.optString("message").isEmpty() && value.optJSONArray("images") != null) return "图片"
        val summary = value.optJSONArray("updates")?.let { rows ->
            (rows.length() - 1 downTo 0).firstNotNullOfOrNull { rows.optJSONObject(it)?.optString("summary")?.takeIf(String::isNotEmpty) }
        }.orEmpty()
        return value.optString("message").ifEmpty { value.optString("text") }.ifEmpty { value.optString("summary") }.ifEmpty { summary }.ifEmpty {
            if (kind.startsWith("pty_status")) "终端状态：" + (value.optJSONObject("session_process")?.optString("state") ?: value.optString("state", "已更新"))
            else if (kind == "interaction") "终端或扩展操作记录，可查看关联证据。" else "Agent 状态：" + value.optString("state", "已更新")
        }
    }
    data class Evidence(val id: String, val label: String)
    fun evidence(item: JSONObject): List<Evidence> {
        val found = linkedMapOf<String, Evidence>()
        fun collect(value: Any?) {
            if (value is JSONObject) value.keys().forEach { key ->
                val id = value.opt(key)
                if (key == "images") return@forEach
                if ((key == "record_id" || key.endsWith("_record_id")) && id is String && id.isNotBlank()) {
                    val label = when {
                        key == "call_record_id" || value.optString("source") == "tool_call" -> "调用参数"
                        key == "result_record_id" || value.optString("source") == "tool_result" -> "执行结果"
                        else -> "关联证据"
                    }
                    if (found[id] == null || found[id]?.label == "关联证据") found[id] = Evidence(id, label)
                } else collect(id)
            }
            if (value is org.json.JSONArray) (0 until value.length()).forEach { collect(value.opt(it)) }
        }
        collect(item.optJSONObject("value")); return found.values.toList()
    }
    fun toolSummary(item: JSONObject): String {
        val value = item.optJSONObject("value") ?: JSONObject()
        val names = value.optJSONArray("tools")?.let { rows -> (0 until rows.length()).mapNotNull { rows.optJSONObject(it)?.optString("name")?.takeIf { name -> name.isNotBlank() } }.distinct() }.orEmpty()
        val title = names.joinToString("、").ifEmpty { "工具操作" }
        val updates = value.optJSONArray("updates")
        val summary = updates?.let { rows -> (rows.length()-1 downTo 0).firstNotNullOfOrNull { index ->
            val row = rows.optJSONObject(index) ?: return@firstNotNullOfOrNull null
            row.optString("summary").takeIf { it.isNotBlank() && it != "null" }
                ?: row.optJSONObject("digest")?.optString("summary")?.takeIf { it.isNotBlank() }
                ?: row.optString("error").takeIf { it.isNotBlank() && it != "null" }
        } } ?: value.optString("summary").takeIf { it.isNotBlank() }
        val count = evidence(item).size
        return title + if (summary != null) " · " + summary.replace(Regex("\\s+"), " ").take(180)
            else if (count > 0) " · $count 项记录" else " · 执行记录"
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
