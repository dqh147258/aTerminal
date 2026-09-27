package com.yxf.aterminal

import android.app.Activity
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.*
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.Executors

class GlobalConversationPanel(private val activity: Activity, private val body: LinearLayout,
    private val identity: List<String>, private val deviceName: String,
    private val valid: () -> Boolean, private val request: (String) -> String,
    private val back: () -> Unit, private val open: (JSONObject) -> Unit) {
    private val store = GlobalConversationStore(activity, identity)
    private val worker = Executors.newSingleThreadExecutor()
    private val ui = Handler(Looper.getMainLooper())
    private var closed = false
    private var busy = false
    private var creating = false
    private var confirmed = false
    private var rendered = ""
    private val list = activity.column(16)
    private val scroll = activity.scroll(list)
    private val status = activity.label("", 12f, Palette.muted)
    private val add = activity.iconButton(R.drawable.ic_plus, "新增会话") { create() }
    private val tick = object : Runnable { override fun run() { if (!closed) { refresh(); ui.postDelayed(this, 1500) } } }
    init { with(activity) {
        body.setBackgroundColor(Palette.background)
        (body.getChildAt(0) as LinearLayout).apply {
            removeAllViews(); addView(iconButton(R.drawable.ic_arrow_left, "返回首页", back).apply { background = null })
            fill(heading("全局AI助手").apply { setPadding(dp(16), 0, 0, 0) }); addView(add.apply { background = shape(Palette.surface, true) })
        }
        body.grow(scroll)
        body.addView(status.apply { setPadding(dp(16), dp(8), dp(16), dp(8)) })
        render(); scroll.post { scroll.scrollTo(0, store.scroll) }; ui.post(tick)
    } }
    private fun render() { with(activity) {
        val rows = store.rows(); val signature = rows.joinToString { it.toString() + store.unread(it) } + confirmed + valid()
        add.isEnabled = valid() && !creating
        if (rendered == signature) return
        rendered = signature; val y = scroll.scrollY; list.removeAllViews()
        list.addView(label("$deviceName · ${if (valid()) "在线" else "离线"}", 14f, Palette.secondary)); list.gap(12)
        list.addView(label("独立对话，集中掌握", 24f).apply { setTypeface(typeface, android.graphics.Typeface.BOLD) })
        list.addView(label("跨终端的任务，在这里继续。", 16f, Palette.muted)); list.gap(24)
        list.addView(label("${rows.size} 个会话 · ${rows.count { store.unread(it) }} 个未读", 12f, Palette.muted)); list.gap(8)
        if (rows.isEmpty()) list.addView(label("暂无会话", 14f, Palette.muted))
        rows.forEach { item ->
            list.addView(View(activity).apply { setBackgroundColor(Palette.line) }, LinearLayout.LayoutParams(-1, dp(1)))
            list.addView(row().apply {
                tag = "global-row:${item.getJSONObject("scope").getString("agent")}"
                setPadding(0, dp(20), 0, dp(20)); isClickable = true; isFocusable = true
                setOnClickListener { store.scroll = scroll.scrollY; open(item) }
                addView(iconButton(R.drawable.ic_message_circle, "打开 ${item.optString("title")}") { store.scroll = scroll.scrollY; open(item) }.apply { background = shape(Palette.surface, true) })
                fill(column().apply {
                    setPadding(dp(12), 0, dp(8), 0)
                    addView(row().apply {
                        fill(heading(item.optString("title", "新会话")).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END })
                        if (item.optLong("updated_at") > 0) addView(label(android.text.format.DateFormat.format("MM-dd HH:mm", item.optLong("updated_at")).toString(), 11f, Palette.muted))
                    })
                    addView(label(item.optString("preview"), 16f, Palette.secondary).apply { maxLines = 2; ellipsize = android.text.TextUtils.TruncateAt.END })
                    addView(row().apply {
                        val state = item.optString("state", "idle")
                        if (confirmed && valid() && GlobalConversationStore.running(state)) addView(ProgressBar(activity).apply { contentDescription = "执行中" }, LinearLayout.LayoutParams(dp(16), dp(16)))
                        addView(label(if (!confirmed || !valid()) "状态待同步" else GlobalConversationStore.stateLabel(state), 12f, Palette.muted))
                        if (store.unread(item)) addView(label(" 未读 ", 12f, Palette.accent).apply { background = shape(Palette.control) }, LinearLayout.LayoutParams(-2, -2).apply { marginStart = dp(8) })
                    })
                })
                addView(label("›", 22f, Palette.muted))
            })
        }
        scroll.post { if (!closed) scroll.scrollTo(0, y) }
    } }
    private fun refresh() {
        render()
        if (busy || closed || !valid()) return
        busy = true
        worker.execute { try {
            val rows = mutableListOf<JSONObject>(); var cursor: Long? = null
            do {
                val command = JSONObject().put("version", 1).put("action", "global_list"); cursor?.let { command.put("cursor", it) }
                val page = JSONObject(request(command.toString())); val array = page.getJSONArray("conversations")
                for (i in 0 until array.length()) rows.add(array.getJSONObject(i))
                cursor = if (page.isNull("cursor")) null else page.getLong("cursor")
            } while (cursor != null && !closed)
            ui.post { busy = false; if (!closed && valid()) { store.merge(rows); confirmed = true; status.text = "全局AI助手会话独立于终端 Session"; render() } }
        } catch (e: Exception) { ui.post { busy = false; if (!closed) { confirmed = false; status.text = "会话同步失败：${e.message}"; render() } } } }
    }
    private fun create() {
        if (creating || !valid()) return
        creating = true; add.isEnabled = false
        val id = store.pendingCreate.ifEmpty { UUID.randomUUID().toString().also { store.pendingCreate = it } }
        worker.execute { try {
            val result = JSONObject(request(JSONObject().put("version", 1).put("action", "global_create").put("request_id", id).toString()))
            val row = JSONObject().put("scope", result.getJSONObject("scope")).put("title", "新会话").put("state", "idle").put("updated_at", System.currentTimeMillis())
            store.merge(listOf(row)); store.pendingCreate = ""
            ui.post { if (!closed && valid()) { creating = false; store.scroll = scroll.scrollY; open(row) } }
        } catch (e: Exception) { ui.post { if (!closed) { creating = false; status.text = "创建失败：${e.message}"; render() } } } }
    }
    fun pause() { store.scroll = scroll.scrollY; ui.removeCallbacks(tick) }
    fun resume() { if (!closed) { ui.removeCallbacks(tick); ui.post(tick) } }
    fun close() { pause(); closed = true; worker.shutdown() }
}
