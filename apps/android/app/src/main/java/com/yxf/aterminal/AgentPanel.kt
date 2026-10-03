package com.yxf.aterminal

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.view.View
import android.widget.*
import org.json.JSONArray
import org.json.JSONObject
import uniffi.ai_terminal_mobile.AgentCache
import java.util.UUID
import java.util.concurrent.Executors

/** Identity, paging, drafts and late callbacks are always bound to the originating scope. */
class AgentPanel(private val activity: Activity, private val body: LinearLayout,
    private val identity: List<String>, private val initialSession: String,
    private val valid: () -> Boolean, private val request: (String, String) -> String,
    @Suppress("UNUSED_PARAMETER") settings: () -> Unit, @Suppress("UNUSED_PARAMETER") history: Boolean = false,
    cachePath: String = activity.filesDir.resolve("agent-history.sqlite3").path,
    private val workingPath: () -> String = { "路径不可用" },
    private val pickImages: (((List<Uri>) -> Unit) -> Unit)? = null,
    private val globalConversation: JSONObject? = null, private val back: (() -> Unit)? = null) {
    private val worker = Executors.newSingleThreadExecutor()
    private val ui = Handler(Looper.getMainLooper())
    private val cache = AgentCache.open(cachePath)
    private val drafts = AgentDraftStore(activity, identity)
    private val markdown = ChatMarkdown(activity)
    private var details: ToolDetailsPage? = null
    @Volatile private var closed = false
    private var epoch = 0
    private val globalId = globalConversation?.getJSONObject("scope")?.getString("agent")
    private val globalStore = GlobalConversationStore(activity, identity)
    private var visible = true
    private var compactLayoutListener: android.view.ViewTreeObserver.OnGlobalLayoutListener? = null
    private var session = globalId?.let { "global:$it" } ?: initialSession
    private var loading = false
    private var olderPageRequested = false
    private var stateLoading = false
    private val sending = mutableSetOf<String>()
    private val importing = mutableSetOf<String>()
    private val submitting get() = session in sending
    private val readingImages get() = session in importing
    private var cursor: String? = null
    private var hasMore = true
    private var generation: Long? = null
    private var runState = "idle"
    private var liveText = ""
    private var nextLiveText: String? = null
    private var liveRoot = ""
    private var renderVersion = 0
    private var requestId = UUID.randomUUID().toString()
    private var restoring = false
    private var initialScroll: Int? = null
    private var answerFocusSignature = ""
    private var answerScrollHeld = false
    private var manualScrollRevision = 0L
    private val items = linkedMapOf<String, JSONObject>()
    private val attachments = mutableListOf<JSONObject>()
    private val state = activity.label("", 12f, Palette.muted)
    private val path = activity.label("", 12f, Palette.muted)
    private val taskState = activity.label("", 12f, Palette.muted)
    private val messages = activity.column(16)
    private val scroll = activity.scroll(messages)
    private val draft = object : EditText(activity) {
        override fun onTextContextMenuItem(id: Int): Boolean {
            if (id == android.R.id.paste) {
                val clip = (activity.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager).primaryClip
                val uris = (0 until (clip?.itemCount ?: 0)).mapNotNull { clip?.getItemAt(it)?.uri }
                if (uris.isNotEmpty()) { importImages(uris); return true }
            }
            return super.onTextContextMenuItem(id)
        }
    }
    private val preview = activity.row()
    private val latest = activity.actionButton("↓ 最新消息") { scroll.post { scroll.scrollTo(0, (messages.height - scroll.height).coerceAtLeast(0)) }; latestVisible(false) }
    private val sendButton = activity.iconButton(R.drawable.ic_arrow_up, "发送") { submit() }
    private val conversationTitle = activity.heading(globalConversation?.optString("title", "新会话") ?: workingPath().substringAfterLast('/').ifEmpty { "终端" })
    private var recognizer: SpeechRecognizer? = null
    private var voiceEpoch = 0
    private var pendingVoice = false
    private val voice = activity.iconButton(R.drawable.ic_mic, "语音输入") { if (recognizer == null) startVoice() else stopVoice() }
    private val authorization = AgentAuthorizationPanel(activity, identity + session,
        { !closed && valid() },
        { command -> rpc(session, command) }, { updateButton() })
    private val tick = object : Runnable { override fun run() { if (!closed) { updatePath(); markRead(); syncDraft(); refresh(); authorization.refresh(); ui.postDelayed(this, 1500) } } }
    private fun scope(target: String = session) = JSONArray(identity + target).toString()
    private fun rpc(target: String, command: JSONObject): JSONObject {
        globalId?.let { command.put("agent_id", it) }
        return JSONObject(request(if (globalId != null) "" else target, command.put("version", 1).toString()))
    }
    private fun updatePath() { path.text = workingPath(); taskState.text = GlobalConversationStore.stateLabel(runState) }
    private fun markRead() {
        if (globalId == null || closed || !visible || loading || initialScroll != null || !activity.hasWindowFocus() || !scroll.isShown || scroll.height == 0 || messages.isLayoutRequested || scroll.isLayoutRequested || !atBottom()) return
        if (items.values.any { it.optString("kind") in setOf("assistant", "interaction") && it.optJSONObject("value")?.optBoolean("partial") == true }) return
        val sequence = items.values.filter { it.optString("kind") in setOf("assistant", "interaction") && it.optJSONObject("value")?.optString("text").orEmpty().isNotBlank() }.maxOfOrNull { it.optLong("sequence") } ?: return
        globalStore.read(globalId, sequence)
    }
    init { with(activity) {
        val header = body.getChildAt(0) as LinearLayout
        val close = header.getChildAt(header.childCount - 1)
        header.removeAllViews()
        val scopeLabel = label(if (globalId != null) "全局AI助手" else "Session", 12f, Palette.accent)
        if (globalId != null) {
            body.setBackgroundColor(Palette.background)
            header.addView(iconButton(R.drawable.ic_arrow_left, "返回全局AI助手") { back?.invoke() }.apply { background = null })
        }
        header.addView(column().apply {
            addView(scopeLabel)
            addView(conversationTitle)
            if (globalId == null) addView(path)
        }, LinearLayout.LayoutParams(0, -2, 1f).apply { marginStart = dp(12) })
        header.addView(authorization.entry)
        if (globalId == null) header.addView(close)
        if (globalId != null) body.addView(row().apply {
            setPadding(dp(16), 0, dp(16), dp(12)); setBackgroundColor(Palette.surface)
            fill(path); addView(taskState)
        }, 1)
        header.setPadding(dp(4), dp(4), dp(8), dp(4)); updatePath(); path.maxLines = 1; path.ellipsize = android.text.TextUtils.TruncateAt.END
        if (globalId == null) path.setOnClickListener { AlertDialog.Builder(activity).setTitle("当前终端路径").setMessage(workingPath()).setPositiveButton("关闭", null).show() }
        if (globalConversation?.optBoolean("legacy") == true) drafts.migrateGlobal(globalId!!)
        latest.visibility = View.GONE
        body.grow(FrameLayout(activity).apply {
            addView(scroll, FrameLayout.LayoutParams(-1, -1))
            addView(latest, FrameLayout.LayoutParams(-2, dp(44), android.view.Gravity.BOTTOM or android.view.Gravity.END).apply { rightMargin = dp(12); bottomMargin = dp(8) })
        })
        state.setPadding(dp(16), 0, dp(16), 0); state.visibility = View.GONE; body.addView(state)
        scroll.setOnTouchListener { _, event ->
            if (event.actionMasked == android.view.MotionEvent.ACTION_MOVE) {
                manualScrollRevision++; initialScroll = null
                focusedAnswer()?.let { answerFocusSignature = answerSignature(it); answerScrollHeld = true }
            }
            false
        }
        scroll.setOnScrollChangeListener { _, _, y, _, old ->
            if (y < old && y < dp(80)) load(false)
            if (atBottom()) { latestVisible(false); markRead() }
        }
        draft.apply {
            hint = "你想做什么？"; contentDescription = "发送任务或追加消息"; textSize = 16f
            setTextColor(Palette.text); setHintTextColor(Palette.muted); background = shape(Palette.background, true)
            inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE or android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
            minHeight = dp(44); maxHeight = dp(112); maxLines = 4; isVerticalScrollBarEnabled = false
            setPadding(dp(8), dp(8), dp(8), dp(8))
            imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI or android.view.inputmethod.EditorInfo.IME_FLAG_NO_FULLSCREEN
        }
        val composerLabel = label("发送任务或追加消息", 12f, Palette.secondary).apply { setPadding(dp(8), 0, 0, dp(8)) }
        body.addView(column(8).apply {
            setBackgroundColor(Palette.surface)
            addView(composerLabel)
            addView(preview)
            addView(row().apply {
                val image = iconButton(R.drawable.ic_image, "选择图片；长按粘贴图片") {
                    val target = session
                    pickImages?.invoke { uris -> importImages(uris, target) } ?: feedback("图片选择器不可用")
                }
                image.setOnLongClickListener { val clip = (getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager).primaryClip
                    val uris = (0 until (clip?.itemCount ?: 0)).mapNotNull { clip?.getItemAt(it)?.uri }
                    if (uris.isEmpty()) feedback("剪贴板中没有可读取的图片") else importImages(uris); true }
                listOf(image, voice, sendButton).forEach { it.setPadding(dp(14), dp(14), dp(14), dp(14)); it.background = shape(android.graphics.Color.TRANSPARENT); it.layoutParams = LinearLayout.LayoutParams(dp(44), dp(44)).apply { marginStart = 0 } }
                sendButton.background = shape(Palette.accent); sendButton.imageTintList = android.content.res.ColorStateList.valueOf(Palette.background)
                addView(image); addView(voice)
                addView(draft, LinearLayout.LayoutParams(0, -2, 1f).apply { marginStart = dp(4); marginEnd = dp(4) })
                addView(sendButton)
            })
        })
        // Adapt after the layout traversal so Android remeasures every affected parent.
        compactLayoutListener = android.view.ViewTreeObserver.OnGlobalLayoutListener {
            val visibility = if (body.height < dp(280)) View.GONE else View.VISIBLE
            val changed = listOf(composerLabel, scopeLabel, path, taskState).filter { it.visibility != visibility }
            changed.forEach { it.visibility = visibility }
            keepAnswerControlsVisible()
        }
        body.viewTreeObserver.addOnGlobalLayoutListener(compactLayoutListener)
        draft.addTextChangedListener(watcher { if (!restoring) { requestId = UUID.randomUUID().toString(); saveComposer() }; updateButton() })
        restore(); load(true); ui.post(tick)
    } }
    private fun focusedAnswer(): EditText? = (authorization.cards.findFocus() as? EditText)?.takeIf { it.tag?.toString()?.startsWith("answer:") == true }
    private fun answerSignature(input: EditText) = "${input.tag}:${input.text}:${input.selectionStart}:${input.selectionEnd}"
    private fun keepAnswerControlsVisible() {
        val input = focusedAnswer() ?: run { answerFocusSignature = ""; answerScrollHeld = false; return }
        val signature = answerSignature(input)
        if (signature != answerFocusSignature) { answerFocusSignature = signature; answerScrollHeld = false }
        if (closed || answerScrollHeld || scroll.isLayoutRequested || messages.isLayoutRequested) return
        val id = input.tag.toString().substringAfter("answer:")
        val submit = authorization.cards.findViewWithTag<View>("answer-submit:$id") ?: return
        if (!input.isShown || !submit.isShown || submit.parent !== input.parent) return
        val pairHeight = submit.bottom - input.top
        if (pairHeight <= 0 || pairHeight > scroll.height) return
        // Ask the measured parent to reveal both controls, preserving the focused field and its selection.
        submit.requestRectangleOnScreen(android.graphics.Rect(0, input.top - submit.top, submit.width, submit.height), true)
    }

    private fun feedback(text: String) { state.text = text; state.visibility = if (text.isEmpty()) View.GONE else View.VISIBLE }
    private fun latestVisible(show: Boolean) { latest.visibility = if (show) View.VISIBLE else View.GONE }
    private fun atBottom() = scroll.scrollY + scroll.height >= messages.height - activity.dp(48)
    private fun snapshot() = JSONObject().put("text", draft.text.toString()).put("images", JSONArray(attachments)).put("request_id", requestId)
        .put("run_state", runState).put("scroll", scroll.scrollY).put("cursor", cursor).put("has_more", hasMore).put("generation", generation)
        .put("items", JSONArray(items.values.toList())).put("search", items.values.joinToString("\n") { AgentTimeline.text(it) })
    private fun saveComposer() { if (!closed && !restoring) drafts.save(session, snapshot()) }
    private fun save() {
        if (closed || restoring) return
        val published = drafts.read(session)
        syncDraft(published)
        val viewState = snapshot()
        // History/scroll callbacks must not overwrite a send acknowledgement from a closed panel.
        // Only explicit edits, attachment changes and the matching send acknowledgement own these fields.
        if (published.has("request_id")) for (key in listOf("text", "images", "request_id")) {
            if (published.has(key)) viewState.put(key, published.get(key)) else viewState.remove(key)
        }
        drafts.save(session, viewState)
    }
    private fun syncDraft(saved: JSONObject = drafts.read(session)) {
        val id = saved.optString("request_id")
        if (id.isNotEmpty() && id != requestId && !submitting && !readingImages) {
            restoring = true; draft.setText(saved.optString("text")); draft.setSelection(draft.length()); requestId = id
            attachments.clear(); saved.optJSONArray("images")?.let { a -> (0 until a.length()).forEach { attachments.add(a.getJSONObject(it)) } }
            restoring = false; renderAttachments(); updateButton()
        }
    }
    private fun restore() {
        restoring = true
        val saved = drafts.read(session)
        draft.setText(saved.optString("text")); draft.setSelection(draft.length())
        attachments.clear(); saved.optJSONArray("images")?.let { a -> (0 until a.length()).forEach { attachments.add(a.getJSONObject(it)) } }
        requestId = saved.optString("request_id").ifEmpty { UUID.randomUUID().toString() }
        items.clear(); saved.optJSONArray("items")?.let { a -> (0 until a.length()).forEach { val item = a.getJSONObject(it); items[item.getString("id")] = item } }
        cursor = saved.optString("cursor").takeUnless { it.isEmpty() || it == "null" }; hasMore = saved.optBoolean("has_more", true)
        generation = saved.optLong("generation").takeIf { saved.has("generation") && !saved.isNull("generation") }
        initialScroll = saved.optInt("scroll").takeIf { saved.has("scroll") }; runState = saved.optString("run_state", "idle"); liveText = ""; nextLiveText = null; liveRoot = ""
        restoring = false; render(false); renderAttachments(); updateButton()
    }
    fun close() {
        details?.close(); details = null
        save(); closed = true; authorization.close(); epoch++; stopVoice(); ui.removeCallbacks(tick); worker.shutdown()
        compactLayoutListener?.let { if (body.viewTreeObserver.isAlive) body.viewTreeObserver.removeOnGlobalLayoutListener(it) }
        compactLayoutListener = null
    }
    fun pause() { visible = false; save(); stopVoice(); ui.removeCallbacks(tick) }
    fun resume() { visible = details == null; if (!closed) { ui.removeCallbacks(tick); ui.post(tick) } }
    private fun updateButton() { with(activity) {
        val running = GlobalConversationStore.active(runState)
        val content = draft.text.toString().isNotBlank() || attachments.isNotEmpty()
        val cancelling = runState in setOf("cancelling", "stopping")
        val reason = if (valid()) null else "设备离线或对话已变化 · 只读缓存"
        val icon = if (running && !content) R.drawable.ic_square else if (running) R.drawable.ic_plus else R.drawable.ic_arrow_up
        sendButton.setImageResource(icon); sendButton.contentDescription = if (cancelling) "取消中" else if (running && !content) "停止" else if (running) "追加" else "发送"
        sendButton.isEnabled = !submitting && !readingImages && !cancelling && reason == null && authorization.canSend && (running || content)
        draft.isEnabled = !submitting
        sendButton.alpha = if (sendButton.isEnabled) 1f else .4f
        updatePath()
        if (reason != null) feedback(reason)
    } }
    private fun submit() {
        if (closed) return
        syncDraft()
        if (!sendButton.isEnabled) return
        val text = draft.text.toString().trim(); val pictures = attachments.map { JSONObject(it.toString()) }
        val target = session; val mode = authorization.permissionMode; val id = requestId
        val cancel = text.isEmpty() && pictures.isEmpty() && GlobalConversationStore.active(runState)
        save(); sending.add(target); updateButton()
        worker.execute { val uploaded = mutableListOf<String>(); try {
            val command = JSONObject().put("action", if (cancel) "cancel" else "send")
            if (!cancel) {
                val uploads = JSONArray()
                pictures.forEach { picture ->
                    val file = drafts.file(picture.getString("file")); check(file.isFile) { "图片文件不可用，请移除后重新选择" }
                    val upload = rpc(target, JSONObject().put("action", "image_begin").put("media_type", picture.getString("mime")).put("size", file.length())).getString("upload_id")
                    uploaded.add(upload)
                    file.inputStream().use { input -> var offset = 0; val buffer = ByteArray(32768)
                        while (true) { val count = input.read(buffer); if (count < 0) break
                            rpc(target, JSONObject().put("action", "image_chunk").put("upload_id", upload).put("offset", offset).put("data", android.util.Base64.encodeToString(buffer, 0, count, android.util.Base64.NO_WRAP))); offset += count
                        }
                    }; uploads.put(upload)
                }
                // Request identity includes the stable file identities, while transport upload IDs are retryable.
                command.put("request_id", id).put("message", text).put("permission_mode", mode)
                if (uploads.length() > 0) command.put("images", uploads)
            }
            check(!closed && valid()) { "连接或对话已变化" }
            val result = rpc(target, command)
            ui.post {
                sending.remove(target)
                val resultState = result.optString("state", if (cancel) "stopping" else "running")
                if (!closed && target == session) {
                    runState = resultState
                    if (!cancel && requestId == id) {
                        attachments.clear(); restoring = true; draft.setText(""); restoring = false
                        requestId = UUID.randomUUID().toString(); saveComposer(); pictures.forEach { drafts.file(it.getString("file")).delete() }; renderAttachments()
                    }
                    feedback(if (cancel) "正在请求停止；已执行的操作不会撤销" else ""); updateButton(); load(true); refresh()
                } else {
                    val saved = drafts.read(target).put("run_state", resultState)
                    if (!cancel && saved.optString("request_id") == id) {
                        saved.put("text", "").put("images", JSONArray()).put("request_id", UUID.randomUUID().toString())
                        pictures.forEach { drafts.file(it.getString("file")).delete() }
                    }
                    drafts.save(target, saved)
                }
            }
        } catch (e: Exception) { ui.post { sending.remove(target); if (!closed && target == session) { feedback(errorText(e)); updateButton() } } } finally { uploaded.forEach { upload -> runCatching { rpc(target, JSONObject().put("action", "image_release").put("upload_id", upload)) } } } }
    }
    private fun errorText(e: Exception): String {
        val message = e.message.orEmpty()
        return if (message.contains("vision", true)) "当前模型不支持图片。请到设置 → LLM 大模型更换支持图片的模型，草稿已保留。"
        else "未完成：$message；草稿已保留，可重试"
    }
    private fun refresh() {
        if (closed || !valid() || stateLoading) { updateButton(); return }
        stateLoading = true
        val version = epoch; val target = session; val key = scope()
        worker.execute { try {
            val result = rpc(target, JSONObject().put("action", "state"))
            val newGeneration = result.optLong("history_generation"); cache.reconcile(key, newGeneration)
            ui.post { if (version == epoch && !closed) {
                stateLoading = false; runState = result.optString("state", "idle")
                val live = result.optString("live_text").takeUnless { it == "null" }.orEmpty()
                val root = result.optString("root_user_message_id").takeUnless { it == "null" }.orEmpty()
                if (root.isNotEmpty() && root != liveRoot) { liveRoot = root; liveText = "" }
                nextLiveText = live
                val error = result.optString("error").takeUnless { it.isEmpty() || it == "null" }
                if (error != null) feedback(error)
                if (generation != null && generation != newGeneration) { items.clear(); cursor = null; hasMore = true; generation = newGeneration; feedback("历史已清理，正在刷新") }
                updateButton(); if (!loading) load(true)
            } }
        } catch (e: Exception) { ui.post { if (version == epoch && !closed) { stateLoading = false; feedback("离线缓存 · ${e.message}"); updateButton() } } } }
    }
    private fun load(first: Boolean) {
        if (closed || (!first && !hasMore)) return
        if (loading) { if (!first) olderPageRequested = true; return }
        loading = true
        val version = epoch; val target = session; val key = scope(); val before = if (first) null else cursor
        worker.execute { var offline = false; try {
            val command = JSONObject().put("action", "history"); before?.let { command.put("cursor", it) }
            val text = try { rpc(target, command).toString().also { cache.storePage(key, before, it) } }
                catch (e: Exception) { offline = true; cache.page(key, before) ?: throw e }
            val page = JSONObject(text); val array = page.getJSONArray("items")
            for (i in 0 until array.length()) {
                val item = array.getJSONObject(i)
                try { completeMessage(item, target) } catch (e: Exception) { item.put("full_message_error", e.message) }
            }
            ui.post { if (version == epoch && !closed) {
                loading = false; generation = page.getLong("generation")
                // Refreshing the newest page must never replace an older paging chain.
                if (!first || cursor == null) { hasMore = page.optBoolean("has_more"); cursor = page.optString("cursor").takeUnless { it.isEmpty() || it == "null" } }
                var changed = false
                for (i in 0 until array.length()) { val item = array.getJSONObject(i); val id = item.getString("id"); if (items[id]?.toString() != item.toString()) changed = true; items[id] = item }
                val next = nextLiveText
                if (next != null) {
                    // Swap streaming text and durable history in one render, avoiding a blank frame.
                    if (next.isNotBlank() || hasPersistedLive()) { if (next != liveText) changed = true; liveText = next }
                    nextLiveText = null
                }
                if (changed) render(!first); details?.let { page -> items[page.itemId]?.let { page.update(it) } }; save(); scroll.post { markRead() }
                if (offline) feedback("离线缓存 · 删除状态尚未同步")
                if (olderPageRequested) { olderPageRequested = false; if (hasMore) load(false) }
            } }
        } catch (e: Exception) { ui.post { if (version == epoch && !closed) { loading = false; olderPageRequested = false; feedback("历史加载失败：${e.message} · 向上滑动重试") } } } }
    }
    private fun hasPersistedLive() = liveText.isNotBlank() && items.values.any {
        (liveRoot.isEmpty() || it.optString("root_user_message_id") == liveRoot) &&
            it.optString("kind") in setOf("assistant", "interaction") && AgentTimeline.text(it) == liveText
    }
    /** The history endpoint bounds large values; its record contains the original value JSON. */
    private fun completeMessage(item: JSONObject, target: String) {
        val value = item.optJSONObject("value") ?: return
        if (!value.optBoolean("partial") || item.optString("kind") !in setOf("assistant", "user", "interaction")) return
        val id = value.getString("record_id")
        val file = drafts.file("record-$id")
        val recordCache = android.util.AtomicFile(file)
        val original = if (file.isFile) JSONObject(recordCache.openRead().bufferedReader().use { it.readText() }) else {
            val text = StringBuilder(); var cursor: String? = null; var bytes = 0
            do {
                check(!closed) { "会话已关闭" }
                val command = JSONObject().put("action", "record").put("record_id", id).put("part", "body")
                cursor?.let { command.put("cursor", it) }
                val part = rpc(target, command)
                check(part.optString("kind") == "history_event") { "消息原文格式不正确" }
                val chunk = part.getString("body"); bytes += chunk.toByteArray(Charsets.UTF_8).size
                check(bytes <= 4 * 1024 * 1024) { "消息原文超过记录大小限制" }
                text.append(chunk); cursor = part.optString("cursor").takeUnless { it.isEmpty() || it == "null" }
            } while (cursor != null)
            JSONObject(text.toString()).also { original ->
                val output = recordCache.startWrite()
                try { output.write(original.toString().toByteArray(Charsets.UTF_8)); recordCache.finishWrite(output) }
                catch (e: Exception) { recordCache.failWrite(output); throw e }
            }
        }
        item.put("value", original); item.remove("full_message_error")
    }
    private fun retryCompleteMessage(item: JSONObject) {
        val copy = JSONObject(item.toString()); val target = session; val version = epoch; val historyVersion = generation
        worker.execute { try {
            completeMessage(copy, target)
            ui.post { if (!closed && epoch == version && generation == historyVersion) {
                items[copy.getString("id")] = copy; render(false); save()
            } }
        } catch (e: Exception) { ui.post { if (!closed && epoch == version) feedback("完整消息加载失败：${e.message}") } } }
    }
    private fun showDetails(item: JSONObject) {
        details?.close(); save(); stopVoice()
        (activity.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager).hideSoftInputFromWindow(body.windowToken, 0)
        draft.clearFocus(); body.visibility = View.INVISIBLE; visible = false
        details = ToolDetailsPage(activity, body.parent as FrameLayout, item, { id, callback ->
            val version = epoch; val target = session
            worker.execute {
                val result = runCatching { readToolRecord(id, target) }
                ui.post { if (!closed && version == epoch) callback(result) }
            }
        }, { closeDetails() })
    }
    fun closeDetails(): Boolean {
        val page = details ?: return false
        page.close(); details = null; body.visibility = View.VISIBLE; visible = true
        return true
    }
    private fun readToolRecord(id: String, target: String): ToolRecord {
        var next: String? = null; val text = StringBuilder(); val bytes = java.io.ByteArrayOutputStream(); var size = 0
        do {
            check(!closed) { "会话已关闭" }
            val command = JSONObject().put("action", "record").put("record_id", id).put("part", "body")
            next?.let { command.put("cursor", it) }
            val result = rpc(target, command)
            if (result.optString("encoding") == "base64url") {
                val chunk = android.util.Base64.decode(result.getString("body"), android.util.Base64.URL_SAFE or android.util.Base64.NO_PADDING)
                size += chunk.size; bytes.write(chunk)
            } else { val chunk = result.getString("body"); size += chunk.toByteArray(Charsets.UTF_8).size; text.append(chunk) }
            check(size <= 4 * 1024 * 1024) { "记录超过大小限制" }
            next = result.optString("cursor").takeUnless { it.isEmpty() || it == "null" }
        } while (next != null)
        return ToolRecord(text.toString(), if (bytes.size() > 0) bytes.toByteArray() else null)
    }
    private fun render(older: Boolean) { with(activity) {
        val version = ++renderVersion
        val scopeVersion = epoch
        val scrollRevision = manualScrollRevision
        val bottom = atBottom(); val oldY = scroll.scrollY
        val anchor = (0 until messages.childCount).map { messages.getChildAt(it) }.firstOrNull { it.bottom > oldY }
        val anchorId = anchor?.tag; val offset = oldY - (anchor?.top ?: 0)
        // Pending answer fields remain attached while other runs update chat history.
        for (index in messages.childCount - 1 downTo 0) if (messages.getChildAt(index) !== authorization.cards) messages.removeViewAt(index)
        val newest = items.values.sortedByDescending { it.optLong("sequence", it.optLong("created_at")) }
        val all = AgentTimeline.visible(newest, false)
        if (globalId != null && conversationTitle.text == "新会话") all.firstOrNull { it.optString("kind") == "user" }?.let {
            conversationTitle.text = AgentTimeline.text(it).take(48).ifEmpty { "新会话" }
        }
        fun addMessage(text: String, owner: String, container: LinearLayout) {
            container.addView(label(owner, 12f, Palette.muted))
            container.addView(markdown.view(text))
        }
        for (item in all) {
            val kind = item.optString("kind"); val value = item.optJSONObject("value") ?: JSONObject()
            val block = column().apply { tag = item.optString("id") }
            if (kind == "interaction") {
                // Assistant narration is a message, even when the same turn invokes tools.
                value.optString("text").takeIf { it.isNotBlank() }?.let { text ->
                    addMessage(text, "aTerminal", block)
                }
                block.addView(row().apply {
                    background = shape(Palette.surface, true); setPadding(dp(10), dp(4), dp(2), dp(4))
                    addView(ImageView(activity).apply { setImageResource(R.drawable.ic_terminal); imageTintList = android.content.res.ColorStateList.valueOf(Palette.muted); importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO }, LinearLayout.LayoutParams(dp(18), dp(18)).apply { marginEnd = dp(10) })
                    fill(label(AgentTimeline.toolSummary(item), 13f, Palette.secondary).apply { maxLines = 2; ellipsize = android.text.TextUtils.TruncateAt.END })
                    addView(iconButton(R.drawable.ic_chevron_right, "查看工具详情") { showDetails(item) }.apply { tag = item.optString("id"); background = null })
                    setOnClickListener { showDetails(item) }
                })
            } else if (kind == "user" || kind == "assistant") {
                addMessage(AgentTimeline.text(item), if (kind == "user") "你" else "aTerminal", block)
            } else {
                block.addView(row().apply {
                    fill(label(AgentTimeline.text(item), 12f, Palette.muted).apply { maxLines = 2; ellipsize = android.text.TextUtils.TruncateAt.END })
                    addView(iconButton(R.drawable.ic_more_horizontal, "查看状态详情") { showDetails(item) }.apply { background = null })
                })
            }
            if (value.optBoolean("partial") && kind in setOf("assistant", "user", "interaction")) {
                block.addView(actionButton("完整消息暂不可用，点击重试") { retryCompleteMessage(item) })
            }
            value.optJSONArray("images")?.let { array -> for (i in 0 until array.length()) {
                val id = array.getJSONObject(i).getString("record_id")
                val holder = column(); block.addView(holder)
                val cached = drafts.file("record-$id")
                if (cached.exists()) holder.addView(pictureView(cached) { showImage(cached) })
                else { holder.addView(actionButton("加载图片") { record(id, holder) }); record(id, holder) }
            } }
            block.background = shape(if (kind == "user") Palette.control else android.graphics.Color.TRANSPARENT, kind == "user")
            block.setPadding(dp(12), dp(6), dp(8), dp(6))
            messages.addView(block, messages.indexOfChild(authorization.cards).takeIf { it >= 0 } ?: messages.childCount, LinearLayout.LayoutParams(if (kind == "user") -2 else -1, -2).apply {
                gravity = android.view.Gravity.END; bottomMargin = dp(12); if (kind == "user") marginStart = dp(24)
            })
        }
        if (liveText.isNotBlank() && !hasPersistedLive()) {
            val live = column(12).apply { tag = "live"; background = shape(Palette.surface) }
            addMessage(liveText, "aTerminal", live)
            messages.addView(live, messages.indexOfChild(authorization.cards).takeIf { it >= 0 } ?: messages.childCount)
        }
        if (authorization.cards.parent == null) messages.addView(authorization.cards)
        val observer = messages.viewTreeObserver
        val positioned = object : android.view.ViewTreeObserver.OnPreDrawListener {
            override fun onPreDraw(): Boolean {
                if (observer.isAlive) observer.removeOnPreDrawListener(this)
                if (!closed && version == renderVersion && scopeVersion == epoch) {
                    if (scrollRevision == manualScrollRevision) {
                        val restored = initialScroll; if (items.isNotEmpty()) initialScroll = null
                        val anchorView = (0 until messages.childCount).map { messages.getChildAt(it) }.firstOrNull { it.tag != null && it.tag == anchorId }
                        if (restored != null) scroll.scrollTo(0, restored)
                        else if (bottom && !older) scroll.scrollTo(0, (messages.height - scroll.height).coerceAtLeast(0))
                        else { scroll.scrollTo(0, anchorView?.let { it.top + offset } ?: oldY); if (!older) latestVisible(true) }
                    }
                    keepAnswerControlsVisible(); markRead()
                }
                return true
            }
        }
        observer.addOnPreDrawListener(positioned)
    } }
    private fun renderAttachments() { with(activity) {
        preview.removeAllViews(); preview.visibility = if (attachments.isEmpty()) View.GONE else View.VISIBLE
        attachments.toList().forEach { picture ->
            val file = drafts.file(picture.getString("file"))
            preview.addView(column().apply {
                addView(pictureView(file) { showImage(file) }, LinearLayout.LayoutParams(dp(64), dp(60)))
                addView(iconButton(R.drawable.ic_x, "移除图片") { if (!submitting) { attachments.remove(picture); file.delete(); requestId = UUID.randomUUID().toString(); saveComposer(); renderAttachments(); updateButton() } })
            })
        }
    } }
    private fun pictureView(file: java.io.File, click: () -> Unit) = ImageView(activity).apply {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }; BitmapFactory.decodeFile(file.path, bounds)
        val options = BitmapFactory.Options().apply { inSampleSize = (maxOf(bounds.outWidth, bounds.outHeight) / 1400).coerceAtLeast(1) }
        setImageBitmap(BitmapFactory.decodeFile(file.path, options)); adjustViewBounds = true; scaleType = ImageView.ScaleType.FIT_CENTER
        contentDescription = "图片，点击查看完整内容"; setOnClickListener { click() }
    }
    private fun showImage(file: java.io.File) { AlertDialog.Builder(activity).setTitle("图片预览").setView(activity.scroll(pictureView(file) {})).setPositiveButton("关闭", null).show() }
    private fun importImages(uris: List<Uri>, target: String = session) {
        if (closed || uris.isEmpty() || target in importing || target in sending) return
        importing.add(target); feedback("正在读取图片…"); updateButton()
        save()
        worker.execute { val imported = mutableListOf<JSONObject>(); try {
            val saved = drafts.read(target); val pictures = saved.optJSONArray("images") ?: JSONArray()
            check(pictures.length() + uris.size <= 4) { "每条消息最多 4 张图片" }
            var total = (0 until pictures.length()).sumOf { drafts.file(pictures.getJSONObject(it).getString("file")).length() }
            for (uri in uris) {
                val mime = activity.contentResolver.getType(uri).orEmpty(); check(mime in listOf("image/png", "image/jpeg", "image/webp", "image/gif")) { "仅支持 PNG、JPEG、WebP、GIF" }
                val file = drafts.file(UUID.randomUUID().toString()); val picture = JSONObject().put("file", file.name).put("mime", mime); imported.add(picture)
                activity.contentResolver.openInputStream(uri)!!.use { input -> file.outputStream().use { output ->
                    val buffer = ByteArray(32768); var size = 0L
                    while (true) { val count = input.read(buffer); if (count < 0) break; size += count; total += count
                        check(size <= 4 * 1024 * 1024 && total <= 8 * 1024 * 1024) { "Desktop 限制：单张 4 MiB、每条共 8 MiB" }; output.write(buffer, 0, count)
                    }
                } }
                val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }; BitmapFactory.decodeFile(file.path, bounds)
                check(bounds.outWidth > 0 && bounds.outHeight > 0) { "无法读取图片" }
            }
            ui.post { if (!closed) {
                importing.remove(target)
                if (target == session) { syncDraft(); attachments.addAll(imported); requestId = UUID.randomUUID().toString(); saveComposer(); renderAttachments(); feedback("") }
                else { val current = drafts.read(target); val array = current.optJSONArray("images") ?: JSONArray(); imported.forEach { array.put(it) }; drafts.save(target, current.put("images", array).put("request_id", UUID.randomUUID().toString())) }
                updateButton()
            } else {
                val current = drafts.read(target); val array = current.optJSONArray("images") ?: JSONArray(); imported.forEach { array.put(it) }; drafts.save(target, current.put("images", array).put("request_id", UUID.randomUUID().toString()))
            } }
        } catch (e: Exception) { imported.forEach { drafts.file(it.getString("file")).delete() }; ui.post { importing.remove(target); if (!closed) { feedback(errorText(e)); updateButton() } } } }
    }
    private fun record(id: String, holder: LinearLayout) {
        val version = epoch; val target = session
        worker.execute { try {
            val result = readToolRecord(id, target)
            val bytes = checkNotNull(result.image) { "图片记录不可用" }
            val file = drafts.file("record-$id").apply { writeBytes(bytes) }
            ui.post { if (version == epoch && !closed) {
                holder.removeAllViews(); holder.addView(pictureView(file) { showImage(file) })
            } }
        } catch (e: Exception) { ui.post { if (!closed && version == epoch) feedback("图片不可用：${e.message}") } } }
    }
    private fun startVoice() {
        if (!SpeechRecognizer.isRecognitionAvailable(activity)) { feedback("此设备未提供语音识别服务"); return }
        if (activity.checkSelfPermission(android.Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) { pendingVoice = true; activity.requestPermissions(arrayOf(android.Manifest.permission.RECORD_AUDIO), 832); return }
        val version = ++voiceEpoch; val target = session
        fun current() = !closed && version == voiceEpoch && target == session
        try {
            val speech = SpeechRecognizer.createSpeechRecognizer(activity); recognizer = speech
            speech.setRecognitionListener(object : RecognitionListener {
                override fun onReadyForSpeech(params: Bundle?) { if (current()) feedback("正在聆听… 点击麦克风取消") }
                override fun onBeginningOfSpeech() {}
                override fun onRmsChanged(rmsdB: Float) {}
                override fun onBufferReceived(buffer: ByteArray?) {}
                override fun onEndOfSpeech() { if (current()) feedback("正在识别…") }
                override fun onError(error: Int) { if (current()) { stopVoice(); feedback("语音识别失败（$error），可重试或输入文字") } }
                override fun onResults(results: Bundle?) { if (current()) { val text = results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull(); if (!text.isNullOrBlank()) draft.append((if (draft.length() > 0) " " else "") + text); stopVoice(); feedback("") } }
                override fun onPartialResults(partialResults: Bundle?) {}
                override fun onEvent(eventType: Int, params: Bundle?) {}
            })
            voice.contentDescription = "取消录音"; voice.setImageResource(R.drawable.ic_x)
            speech.startListening(Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM))
        } catch (e: Exception) { stopVoice(); feedback("语音不可用：${e.message}") }
    }
    private fun stopVoice() { voiceEpoch++; pendingVoice = false; recognizer?.cancel(); recognizer?.destroy(); recognizer = null; voice.setImageResource(R.drawable.ic_mic); voice.contentDescription = "语音输入" }
    fun permissionResult(code: Int, results: IntArray) { if (code == 832 && pendingVoice && !closed) { pendingVoice = false; if (results.firstOrNull() == PackageManager.PERMISSION_GRANTED) startVoice() else feedback("麦克风权限未开启，仍可输入文字") } }
}
