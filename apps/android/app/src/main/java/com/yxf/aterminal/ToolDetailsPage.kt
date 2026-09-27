package com.yxf.aterminal

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.res.ColorStateList
import android.graphics.BitmapFactory
import android.graphics.Typeface
import android.view.Gravity
import android.view.View
import android.widget.*
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

data class ToolRecord(val text: String, val image: ByteArray? = null)

/** A full-page reader. Every record expands in place; leaving discards late UI callbacks. */
class ToolDetailsPage(private val activity: Activity, private val hostContainer: FrameLayout,
    initial: JSONObject, private val load: (String, (Result<ToolRecord>) -> Unit) -> Unit,
    private val back: () -> Unit) {
    private val page = activity.column().apply { setBackgroundColor(Palette.background); isClickable = true; isFocusableInTouchMode = true }
    private val content = activity.column(16)
    private val viewport = activity.scroll(content)
    private val markdown = ChatMarkdown(activity)
    private var item = initial
    private var closed = false
    private val expanded = mutableSetOf<String>()
    private val loading = mutableSetOf<String>()
    private val records = mutableMapOf<String, ToolRecord>()
    private val errors = mutableMapOf<String, String>()
    val itemId: String = initial.optString("id")

    init { with(activity) {
        page.addView(row().apply {
            setPadding(dp(8), dp(8), dp(16), dp(8)); setBackgroundColor(Palette.surface)
            addView(iconButton(R.drawable.ic_arrow_left, "返回对话", back).apply { background = null })
            fill(heading(if (item.optString("kind") == "interaction") "工具操作详情" else "状态详情").apply { setPadding(dp(8), 0, 0, 0) })
        })
        page.addView(line())
        page.grow(viewport)
        hostContainer.addView(page, FrameLayout.LayoutParams(-1, -1))
        render(); page.requestFocus()
    } }

    fun update(next: JSONObject) {
        if (!closed && next.toString() != item.toString()) { item = next; render() }
    }
    fun close() { if (!closed) { closed = true; hostContainer.removeView(page) } }
    private fun line() = View(activity).apply { setBackgroundColor(Palette.line); layoutParams = LinearLayout.LayoutParams(-1, activity.dp(1)) }
    private fun icon(resource: Int, color: Int = Palette.accent) = ImageView(activity).apply {
        setImageResource(resource); imageTintList = ColorStateList.valueOf(color)
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        layoutParams = LinearLayout.LayoutParams(activity.dp(18), activity.dp(18))
    }
    private fun clean(value: JSONObject, key: String) = value.optString(key).takeUnless { it.isBlank() || it == "null" }
    private fun updates(): List<JSONObject> = item.optJSONObject("value")?.optJSONArray("updates")?.let { rows -> (0 until rows.length()).mapNotNull { rows.optJSONObject(it) } }.orEmpty()
    private fun toolNames(): List<String> = item.optJSONObject("value")?.optJSONArray("tools")?.let { rows -> (0 until rows.length()).mapNotNull { rows.optJSONObject(it)?.let { clean(it, "name") } }.distinct() }.orEmpty()
    private fun title(name: String) = mapOf("read_terminal" to "读取终端", "terminal_input" to "输入终端", "list_sessions" to "查看终端会话", "get_agent_state" to "读取任务状态", "send_agent_message" to "发送任务消息", "mcp_call" to "调用扩展工具")[name] ?: name
    private fun status(): Pair<String, Int> {
        val entries = updates()
        if (entries.any { clean(it, "error") != null }) return "执行失败" to Palette.danger
        val state = entries.asReversed().firstNotNullOfOrNull { clean(it, "state") }
            ?: item.optJSONObject("value")?.let { clean(it, "state") }
        return when (state) {
            "finished", "analyzed", "completed" -> "已完成" to Palette.green
            "analyzing" -> "分析中" to Palette.accent
            "running" -> "执行中" to Palette.accent
            "cancelled", "stopped" -> "已停止" to Palette.muted
            "failed", "error" -> "执行失败" to Palette.danger
            else -> "操作记录" to Palette.muted
        }
    }
    private fun render() { if (closed) return; with(activity) {
        val y = viewport.scrollY
        content.removeAllViews()
        val value = item.optJSONObject("value") ?: JSONObject()
        val names = toolNames(); val evidence = AgentTimeline.evidence(item); val status = status()
        content.addView(row().apply {
            val symbol = icon(R.drawable.ic_terminal).apply { setPadding(dp(10), dp(10), dp(10), dp(10)); background = shape(Palette.control) }
            addView(symbol, LinearLayout.LayoutParams(dp(42), dp(42)).apply { marginEnd = dp(12) })
            fill(column().apply {
                addView(label(if (names.size == 1) title(names.first()) else if (names.isNotEmpty()) "${names.size} 项工具调用" else "操作记录", 20f).apply { setTypeface(typeface, Typeface.BOLD) })
                if (names.isNotEmpty()) addView(label(names.joinToString(" · "), 11f, Palette.muted).apply { typeface = Typeface.MONOSPACE })
            })
            addView(label(status.first, 11f, status.second).apply { background = shape(Palette.surface); setPadding(dp(8), dp(5), dp(8), dp(5)) })
        })
        if (item.optLong("created_at") > 0) content.addView(label(android.text.format.DateFormat.format("MM-dd HH:mm:ss", item.optLong("created_at")).toString(), 11f, Palette.muted).apply { setPadding(0, dp(12), 0, 0) })
        val narration = clean(value, "text")
        val summary = updates().asReversed().firstNotNullOfOrNull { clean(it, "summary") ?: it.optJSONObject("digest")?.let { digest -> clean(digest, "summary") } }
        if (narration != null || summary != null) {
            content.gap(16)
            content.addView(markdown.view(summary ?: narration!!).apply { textSize = 14f; setTextColor(Palette.secondary) })
        }
        updates().mapNotNull { clean(it, "error") }.distinct().forEach { error ->
            content.gap(8); content.addView(label(error, 13f, Palette.danger).apply { setTextIsSelectable(true) })
        }
        content.gap(24)
        content.addView(row().apply { fill(label("调用记录", 12f, Palette.muted)); addView(label("${evidence.size} 项", 12f, Palette.muted)) })
        content.gap(8)
        evidence.forEachIndexed { index, entry ->
            val relatedName = updates().firstOrNull { update -> update.keys().asSequence().any { update.optString(it) == entry.id } }?.let { clean(it, "name") }
            val repeated = evidence.count { it.label == entry.label } > 1
            val ordinal = evidence.take(index + 1).count { it.label == entry.label }
            val recordLabel = if (entry.label == "关联证据") "关联记录" else entry.label
            accordion(entry.id, recordLabel + if (repeated) " · $ordinal" else "", relatedName ?: names.singleOrNull(),
                "${entry.label} ${index + 1}", if (entry.label == "调用参数") R.drawable.ic_sliders_horizontal else if (entry.label == "执行结果") R.drawable.ic_terminal else R.drawable.ic_history) { holder ->
                val record = records[entry.id]
                when {
                    record != null -> recordView(holder, record, recordLabel)
                    errors.containsKey(entry.id) -> {
                        holder.addView(label("读取失败：${errors[entry.id]}", 13f, Palette.danger).apply { setTextIsSelectable(true) })
                        holder.addView(actionButton("重新读取") { errors.remove(entry.id); fetch(entry.id); render() })
                    }
                    else -> {
                        holder.addView(row().apply {
                            addView(ProgressBar(activity).apply { indeterminateTintList = ColorStateList.valueOf(Palette.accent) }, LinearLayout.LayoutParams(dp(16), dp(16)).apply { marginEnd = dp(8) })
                            addView(label("正在读取…", 12f, Palette.muted))
                        })
                        fetch(entry.id)
                    }
                }
            }
            content.gap(10)
        }
        if (evidence.isEmpty()) content.addView(label("此记录没有关联的参数或结果。", 13f, Palette.muted))
        content.gap(14)
        accordion("raw", "完整记录", "JSON", "完整记录", R.drawable.ic_more_horizontal) { holder -> recordView(holder, ToolRecord(value.toString(2)), "完整记录") }
        content.gap(12)
        viewport.post { if (!closed) viewport.scrollTo(0, y) }
    } }
    private fun accordion(key: String, title: String, subtitle: String?, accessible: String, resource: Int, populate: (LinearLayout) -> Unit) { with(activity) {
        val open = key in expanded
        content.addView(column().apply {
            background = shape(Palette.surface, true)
            addView(row().apply {
                tag = "record-toggle:$key"; minimumHeight = dp(60); setPadding(dp(14), dp(8), dp(12), dp(8))
                isClickable = true; isFocusable = true; contentDescription = (if (open) "收起" else "展开") + accessible
                accessibilityDelegate = object : View.AccessibilityDelegate() {
                    override fun onInitializeAccessibilityNodeInfo(host: View, info: android.view.accessibility.AccessibilityNodeInfo) {
                        super.onInitializeAccessibilityNodeInfo(host, info); info.className = Button::class.java.name
                        if (android.os.Build.VERSION.SDK_INT >= 30) info.stateDescription = if (open) "已展开" else "已收起"
                    }
                }
                addView(icon(resource).apply { layoutParams = LinearLayout.LayoutParams(dp(18), dp(18)).apply { marginEnd = dp(12) } })
                fill(column().apply {
                    addView(label(title, 14f).apply { setTypeface(typeface, Typeface.BOLD) })
                    subtitle?.let { addView(label(it, 11f, Palette.muted).apply { typeface = Typeface.MONOSPACE }) }
                })
                addView(icon(R.drawable.ic_chevron_right, Palette.muted).apply { rotation = if (open) 270f else 90f })
                setOnClickListener { if (!expanded.add(key)) expanded.remove(key); render() }
            })
            if (open) { addView(line()); addView(column(12).apply { tag = "record-content:$key"; populate(this) }) }
        })
    } }
    private fun fetch(id: String) {
        if (closed || !loading.add(id)) return
        load(id) { result ->
            if (!closed) {
                loading.remove(id)
                result.onSuccess { records[id] = it }.onFailure { errors[id] = it.message ?: "记录不可用" }
                render()
            }
        }
    }
    private fun recordView(holder: LinearLayout, record: ToolRecord, title: String) { with(activity) {
        if (record.image != null) {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }; BitmapFactory.decodeByteArray(record.image, 0, record.image.size, bounds)
            val options = BitmapFactory.Options().apply { inSampleSize = (maxOf(bounds.outWidth, bounds.outHeight) / 1400).coerceAtLeast(1) }
            val bitmap = BitmapFactory.decodeByteArray(record.image, 0, record.image.size, options)
            if (bitmap != null) holder.addView(ImageView(activity).apply { setImageBitmap(bitmap); adjustViewBounds = true; contentDescription = "记录图片" }, LinearLayout.LayoutParams(-1, -2))
            else holder.addView(label("二进制记录 · ${record.image.size} 字节", 13f, Palette.muted))
            return
        }
        val parsed = runCatching {
            val reader = JSONTokener(record.text); val value = reader.nextValue()
            if (reader.nextClean() == '\u0000') value else null
        }.getOrNull()
        val pretty = when (parsed) { is JSONObject -> parsed.toString(2); is JSONArray -> parsed.toString(2); else -> record.text }
        val format = label(if (parsed is JSONObject || parsed is JSONArray) "JSON" else "文本", 10f, Palette.muted)
        holder.addView(row().apply {
            fill(format)
            addView(iconButton(R.drawable.ic_copy, "复制$title") {
                (getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText(title, record.text))
                format.text = "已复制"
            }.apply { background = null })
        })
        val code = label(pretty, 13f, Palette.secondary).apply {
            typeface = Typeface.MONOSPACE; setTextIsSelectable(true); setHorizontallyScrolling(true)
            setPadding(dp(10), dp(12), dp(10), dp(12)); background = shape(Palette.background)
        }
        holder.addView(HorizontalScrollView(activity).apply { isFillViewport = true; addView(code) }, LinearLayout.LayoutParams(-1, -2))
    } }
}
