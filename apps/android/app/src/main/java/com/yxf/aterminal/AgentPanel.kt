package com.yxf.aterminal

import android.app.Activity
import android.app.AlertDialog
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.*
import org.json.JSONArray
import org.json.JSONObject
import uniffi.ai_terminal_mobile.AgentCache
import java.util.UUID
import java.util.concurrent.Executors

/** A panel owns one paging chain; background polling never moves its history anchor. */
class AgentPanel(private val activity: Activity, private val body: LinearLayout,
    private val identity: List<String>, private val initialSession: String,
    private val valid: () -> Boolean, private val request: (String, String) -> String,
    private val settings: () -> Unit, history: Boolean = false,
    cachePath: String = activity.filesDir.resolve("agent-history.sqlite3").path) {
    private val worker = Executors.newSingleThreadExecutor()
    private val ui = Handler(Looper.getMainLooper())
    private val cache = AgentCache.open(cachePath)
    private var closed = false
    private var epoch = 0
    private var session = initialSession
    private var browsing = history
    private var loading = false
    private var stateLoading = false
    private var cursor: String? = null
    private var hasMore = true
    private var generation: Long? = null
    private val pages = ArrayDeque<List<JSONObject>>()
    private val state = activity.label("正在连接…", 12f, Palette.muted)
    private val messages = activity.column(12)
    private val scroll = activity.scroll(messages)
    private val draft = activity.field("发送任务或追加消息")
    private val allow = CheckBox(activity).apply { text = "允许操作终端与扩展"; setTextColor(Palette.muted) }
    private val tick = object : Runnable { override fun run() { if (!closed) { refresh(); ui.postDelayed(this, 1500) } } }
    private fun scope() = JSONArray(identity + session).toString()
    init { with(activity) {
        val scopes = row().apply {
            fill(actionButton("当前终端") { change(initialSession) }.apply { isEnabled = initialSession.isNotEmpty() })
            fill(actionButton("全局 Agent") { change("") })
            addView(actionButton("设置") { settings() })
        }
        body.addView(scopes)
        body.addView(state)
        val navigation = row().apply {
            fill(actionButton("对话") { browsing = false; reset() })
            fill(actionButton("历史") { browsing = true; reset() })
            addView(actionButton("停止") { send(JSONObject().put("action", "cancel")) })
        }
        body.addView(navigation)
        body.grow(scroll)
        scroll.setOnScrollChangeListener { _, _, y, _, oldY ->
            if (browsing && y > oldY && y + scroll.height >= messages.height - dp(32)) load(false)
        }
        val sendButton = actionButton("发送 / 追加", true) {
            val text = draft.text.toString().trim()
            if (text.isNotEmpty()) send(JSONObject().put("action", "send").put("request_id", UUID.randomUUID().toString()).put("message", text).put("allow_input", allow.isChecked)) { draft.setText("") }
        }
        val composer = column(8).apply {
            addView(allow)
            addView(row().apply {
                fill(draft.apply {
                    isSingleLine = false; maxLines = 4
                    imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI or android.view.inputmethod.EditorInfo.IME_FLAG_NO_FULLSCREEN
                })
                addView(sendButton)
            })
        }
        body.addView(composer)
        val header = body.getChildAt(0) as? LinearLayout
        val more = iconButton(R.drawable.ic_more_horizontal, "Agent 操作") {}
        more.setOnClickListener {
            PopupMenu(activity, more).apply {
                menu.add(0, 1, 0, "当前终端").isEnabled = initialSession.isNotEmpty()
                menu.add(0, 2, 1, "全局 Agent")
                menu.add(0, 3, 2, "对话"); menu.add(0, 4, 3, "历史")
                menu.add(0, 5, 4, "设置"); menu.add(0, 6, 5, "停止")
                setOnMenuItemClickListener { item ->
                    when (item.itemId) {
                        1 -> change(initialSession); 2 -> change("")
                        3 -> { browsing = false; reset() }; 4 -> { browsing = true; reset() }
                        5 -> settings(); 6 -> send(JSONObject().put("action", "cancel"))
                    }
                    true
                }
            }.show()
        }
        header?.addView(more, (header.childCount - 1).coerceAtLeast(0))
        var compactBefore: Boolean? = null
        fun adapt(height: Int) {
            if (closed || height <= 0) return
            val compact = height < dp(300)
            if (compactBefore == compact) return
            compactBefore = compact
            scopes.visibility = if (compact) View.GONE else View.VISIBLE
            navigation.visibility = if (compact) View.GONE else View.VISIBLE
            state.visibility = if (compact) View.GONE else View.VISIBLE
            more.visibility = if (compact) View.VISIBLE else View.GONE
            header?.setPadding(dp(16), if (compact) 0 else dp(10), dp(12), if (compact) 0 else dp(10))
            allow.textSize = 12f; allow.minHeight = 0
            allow.layoutParams = LinearLayout.LayoutParams(-1, if (compact) dp(32) else dp(40))
            composer.setPadding(dp(8), dp(if (compact) 4 else 8), dp(8), dp(if (compact) 4 else 8))
            draft.maxLines = if (compact) 2 else 4
            draft.minHeight = dp(44)
            draft.setPadding(dp(8), dp(6), dp(8), dp(6))
            sendButton.minHeight = dp(44)
        }
        body.addOnLayoutChangeListener { _, _, top, _, bottom, _, _, _, _ -> adapt(bottom - top) }
        body.post { adapt(body.height) }
        reset(); ui.post(tick)
    } }
    fun close() { closed = true; epoch++; ui.removeCallbacks(tick); worker.shutdown() }
    private fun change(id: String) { epoch++; session = id; reset() }
    private fun reset() { epoch++; loading = false; cursor = null; hasMore = true; generation = null; pages.clear(); messages.removeAllViews(); load(true) }
    private fun rpc(command: JSONObject) = JSONObject(request(session, command.put("version", 1).toString()))
    private fun send(command: JSONObject, success: () -> Unit = {}) {
        if (closed || !valid()) { state.text = "Desktop 未连接"; return }
        val version = epoch; val target = session
        worker.execute { try {
            request(target, command.put("version", 1).toString())
            ui.post { if (version == epoch && !closed) { success(); if (!browsing) reset(); refresh() } }
        } catch (e: Exception) { ui.post { if (version == epoch && !closed) state.text = "请求未确认：${e.message}，请查询状态" } } }
    }
    private fun refresh() {
        if (closed || !valid() || stateLoading) return
        stateLoading = true
        val version = epoch; val target = session; val key = scope()
        worker.execute { try {
            val result = JSONObject(request(target, JSONObject().put("version", 1).put("action", "state").toString()))
            val newGeneration = result.optLong("history_generation")
            cache.reconcile(key, newGeneration)
            ui.post { stateLoading = false; if (version == epoch && !closed) {
                state.text = (if (session.isEmpty()) "全局" else "当前终端") + " · " + result.optString("state") + result.optString("error", "").let { if (it == "null") "" else " · $it" }
                if (generation != null && generation != newGeneration) { state.text = "历史已清理，正在刷新"; reset() }
                else if (!browsing && !loading) load(true)
            } }
        } catch (e: Exception) { ui.post { stateLoading = false; if (version == epoch && !closed) state.text = "离线缓存 · ${e.message}" } } }
    }
    private fun load(first: Boolean) {
        if (closed || loading || (!first && !hasMore)) return
        loading = true
        val version = epoch; val target = session; val key = scope(); val before = if (first) null else cursor
        worker.execute {
            var offline = false
            try {
                val command = JSONObject().put("version", 1).put("action", "history")
                before?.let { command.put("cursor", it) }
                val text = try { request(target, command.toString()).also { cache.storePage(key, before, it) } }
                    catch (e: Exception) { offline = true; cache.page(key, before) ?: throw e }
                val page = JSONObject(text); val array = page.getJSONArray("items")
                val items = (0 until array.length()).map { array.getJSONObject(it) }
                ui.post { if (version == epoch && !closed) {
                    loading = false; generation = page.getLong("generation"); hasMore = page.optBoolean("has_more")
                    cursor = page.optString("cursor").takeUnless { it.isEmpty() || it == "null" }
                    if (first) pages.clear()
                    pages.addLast(items); while (pages.size > 3) pages.removeFirst()
                    render(); if (offline) state.text = "离线缓存 · 删除状态尚未同步"
                } }
            } catch (e: Exception) { ui.post { if (version == epoch && !closed) { loading = false; state.text = "历史加载失败：${e.message}" } } }
        }
    }
    private fun render() { with(activity) {
        val oldY = scroll.scrollY
        messages.removeAllViews()
        val all = AgentTimeline.visible(pages.flatMap { it }, browsing)
        for (item in all) {
            val value = item.optJSONObject("value") ?: JSONObject()
            val kind=item.optString("kind")
            val text = AgentTimeline.text(item)
            messages.addView(label(when(kind){"user"->"你";"assistant"->"AI Agent";"interaction"->"操作与证据";else->"状态"}, 12f, Palette.accent))
            messages.addView(label(text).apply { setTextIsSelectable(true) })
            val ids = mutableSetOf<String>(); fun collect(v: Any?) {
                if (v is JSONObject) v.keys().forEach { k -> if ((k == "record_id" || k.endsWith("_record_id")) && v.opt(k) is String) ids.add(v.getString(k)) else collect(v.opt(k)) }
                if (v is JSONArray) (0 until v.length()).forEach { collect(v.opt(it)) }
            }; collect(value)
            ids.forEach { id -> messages.addView(actionButton("查看证据") { record(id) }) }; messages.gap(16)
        }
        if (browsing && hasMore) messages.addView(actionButton("加载更早的 50 条") { load(false) })
        if (browsing) messages.addView(actionButton("返回最新历史") { reset() })
        scroll.post {
            if (!closed) scroll.scrollTo(0, if (browsing) oldY else (messages.height - scroll.height + scroll.paddingTop + scroll.paddingBottom).coerceAtLeast(0))
        }
    } }
    private fun record(id: String) {
        val version = epoch; val target = session
        worker.execute { try {
            var next: String? = null; val text = StringBuilder(); val bytes = java.io.ByteArrayOutputStream()
            do {
                val command = JSONObject().put("version", 1).put("action", "record").put("record_id", id).put("part", "body")
                next?.let { command.put("cursor", it) }
                val result = JSONObject(request(target, command.toString()))
                if (result.optString("encoding") == "base64url") bytes.write(android.util.Base64.decode(result.optString("body"), android.util.Base64.URL_SAFE or android.util.Base64.NO_PADDING)) else text.append(result.optString("body"))
                next = result.optString("cursor").takeUnless { it.isEmpty() || it == "null" }
            } while (next != null && bytes.size() + text.length < 4 * 1024 * 1024 && !closed)
            ui.post { if (version == epoch && !closed) {
                if (bytes.size() > 0) { val data = bytes.toByteArray(); val image = ImageView(activity).apply { setImageBitmap(android.graphics.BitmapFactory.decodeByteArray(data, 0, data.size)); adjustViewBounds = true }; AlertDialog.Builder(activity).setTitle("终端画面").setView(image).setPositiveButton("关闭", null).show() }
                else AlertDialog.Builder(activity).setTitle("证据原文").setView(activity.scroll(activity.column(12).apply { addView(activity.label(text.toString()).apply { setTextIsSelectable(true) }) })).setPositiveButton("关闭", null).show()
            } }
        } catch (e: Exception) { ui.post { if (!closed) state.text = "证据不可用：${e.message}" } } }
    }
}
