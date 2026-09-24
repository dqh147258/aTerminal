package dev.aiterminal.app

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.text.InputFilter
import android.text.InputType
import android.view.View
import android.widget.*
import org.json.JSONObject
import java.text.DateFormat
import java.util.Date
import java.util.Locale
import java.util.UUID
import java.util.concurrent.Executors

class AssistantPanel(
    private val activity: Activity,
    private val body: LinearLayout,
    private var chat: Conversation,
    private val storage: ChatStore,
    private val writable: Boolean,
    private val isCurrent: () -> Boolean,
    private val request: (String) -> String,
    private val continueChat: () -> Unit,
    private val terminalDraft: (String) -> Unit,
    private val dispatch: (((() -> Unit)) -> Unit)? = null,
    private val stateChanged: (Conversation) -> Unit = {}
) {
    private val worker = if (dispatch == null) Executors.newSingleThreadExecutor() else null
    private val ui = Handler(Looper.getMainLooper())
    @Volatile private var closed = false
    private var paused = false
    private var busy = false
    private var available = false
    private var pendingVoice = false
    private var recognizing = false
    private var recognizer: SpeechRecognizer? = null
    private lateinit var state: TextView
    private lateinit var messages: LinearLayout
    private lateinit var messageScroll: ScrollView
    private lateinit var draft: EditText
    private lateinit var send: ImageButton
    private lateinit var microphone: ImageButton
    private lateinit var cancelVoice: ImageButton
    private lateinit var voiceState: TextView
    private lateinit var includeScreen: CheckBox
    private lateinit var allowInput: CheckBox
    private lateinit var stop: Button
    private val poll = Runnable { if (valid() && chat.state in AssistantRequest.pollingStates) query("poll") }
    private fun valid() = !closed && !paused && writable && isCurrent()
    private fun save() { storage.saveDraft(chat) }

    init {
        with(activity) {
            val title = label(chat.title, 12f, Palette.muted).apply { setBackgroundColor(Palette.surface); setPadding(dp(16), dp(4), dp(16), dp(4)); maxLines = 1 }
            body.addView(title)
            state = label("", 12f, Palette.accent).apply { accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE; setPadding(dp(4), dp(8), dp(4), dp(8)) }
            stop = actionButton("停止") { query("cancel") }.apply { contentDescription = "停止监控"; visibility = View.GONE }
            body.addView(row().apply {
                setBackgroundColor(Palette.surface)
                setPadding(dp(12), 0, dp(12), 0)
                fill(state)
                if (writable) addView(stop)
                if (writable) addView(iconButton(R.drawable.ic_refresh_cw, "刷新 AI 请求状态") {
                    query(if (chat.requestId.isNotEmpty() && chat.state in AssistantRequest.pollingStates) "poll" else "status")
                })
            })
            messages = column(16)
            messageScroll = scroll(messages); body.grow(messageScroll)
            val composer = column(12).apply { setBackgroundColor(Palette.surface) }
            draft = field("发送给 AI").apply {
                inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
                isSingleLine = false; minLines = 2; maxLines = 4; filters = arrayOf(InputFilter.LengthFilter(4000))
                setText(chat.draft)
                addTextChangedListener(watcher { chat.draft = text.toString(); ui.removeCallbacks(saveDraft); ui.postDelayed(saveDraft, 350) })
            }
            composer.addView(draft)
            includeScreen = CheckBox(activity).apply {
                text = "监控当前终端"; setTextColor(Palette.muted); textSize = 12f
                buttonTintList = ColorStateList.valueOf(Palette.accent); isChecked = true
            }
            allowInput = CheckBox(activity).apply {
                text = "允许操作"; setTextColor(Palette.muted); textSize = 12f
                buttonTintList = ColorStateList.valueOf(Palette.accent); isChecked = true
            }
            composer.addView(row().apply { fill(allowInput); fill(includeScreen) })
            val controls = row()
            microphone = iconButton(R.drawable.ic_mic, "语音输入") { startVoice() }
            cancelVoice = iconButton(R.drawable.ic_x, "取消录音") { stopVoice("已取消录音") }.apply { visibility = View.GONE }
            controls.addView(microphone); controls.addView(cancelVoice)
            controls.fill(label("", 12f, Palette.muted))
            send = iconButton(R.drawable.ic_arrow_up, "发送给 AI") { sendMessage() }.apply {
                background = shape(Palette.accent); imageTintList = ColorStateList.valueOf(Palette.background)
            }
            controls.addView(send); composer.addView(controls)
            voiceState = label("", 12f, Palette.muted).apply { accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE }
            composer.addView(voiceState)
            if (writable) {
                val composerScroll = scroll(composer)
                body.addView(composerScroll, LinearLayout.LayoutParams(-1, dp(196)))
                body.addOnLayoutChangeListener { _, _, top, _, bottom, _, oldTop, _, oldBottom ->
                    if (bottom - top != oldBottom - oldTop) {
                        title.visibility = if (bottom - top < dp(340)) View.GONE else View.VISIBLE
                        composerScroll.layoutParams = (composerScroll.layoutParams as LinearLayout.LayoutParams).apply {
                            height = dp(196).coerceAtMost((bottom - top - dp(if (title.visibility == View.VISIBLE) 160 else 124)).coerceAtLeast(dp(48)))
                        }
                    }
                }
            } else {
                body.addView(label("离线历史", 12f, Palette.muted).apply { setBackgroundColor(Palette.surface); setPadding(dp(16), dp(8), dp(16), dp(8)) })
                body.addView(actionButton("继续对话", true) { continueChat() })
            }
            renderMessages(); updateState()
            if (writable) query(if (chat.requestId.isNotEmpty() && chat.state in AssistantRequest.pollingStates) "poll" else "status")
        }
    }
    private val saveDraft = Runnable { if (!closed) save() }

    private fun renderMessages() {
        messages.removeAllViews()
        with(activity) {
            if (chat.messages.isEmpty()) messages.addView(label("暂无对话", 14f, Palette.muted).apply { setBackgroundColor(Palette.surface) })
            for (message in chat.messages) {
                val entry = column(12).apply { background = shape(if (message.role == "user") Palette.control else Palette.surface, true) }
                val source = when (message.source) { "input" -> "终端输入"; "output" -> "终端输出 #${message.revision}"; "observation" -> "屏幕观察 #${message.revision}"; "error" -> "执行状态"; else -> "AI Agent" }
                entry.addView(label((if (message.role == "user") "你" else source) + " · " + DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(message.time)), 12f, Palette.muted))
                entry.addView(label(message.content, 16f).apply { setTextIsSelectable(true) })
                if (message.role == "assistant") {
                    entry.addView(row().apply {
                        addView(iconButton(R.drawable.ic_copy, "复制回答") {
                            (getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("AI", message.content))
                            Toast.makeText(activity, "已复制", Toast.LENGTH_SHORT).show()
                        })
                        if (writable) addView(actionButton("放入终端草稿") {
                            AlertDialog.Builder(activity).setTitle("放入终端输入框？").setMessage("回答将作为草稿，你可以编辑后手动发送。")
                                .setNegativeButton("取消", null).setPositiveButton("放入草稿") { _, _ -> terminalDraft(message.content) }.show()
                        })
                    })
                }
                messages.addView(entry); messages.gap(16)
            }
        }
        messageScroll.post { if (!closed) messageScroll.fullScroll(View.FOCUS_DOWN) }
    }
    private fun updateState() {
        state.text = when (chat.state) {
            "running" -> "模型请求中"
            "monitoring" -> "正在监控终端"
            "stopping" -> "正在停止监控"
            "stopped" -> "监控已停止"
            "unknown" -> "请求结果待确认 · ${chat.detail}"
            "completed" -> "模型已回复"
            "failed" -> "模型请求失败 · ${chat.detail}"
            "unavailable" -> chat.detail.ifEmpty { "Desktop 尚未配置 AI" }
            else -> if (available) "AI 可用" else if (writable) "正在检查 AI 状态" else "历史记录"
        }
        send.isEnabled = valid() && available && !busy && chat.state !in setOf("running", "stopping", "unknown")
        send.alpha = if (send.isEnabled) 1f else .4f
        draft.isEnabled = writable && !paused
        microphone.isEnabled = writable && !paused && !recognizing
        allowInput.isEnabled = !busy && chat.state !in setOf("running", "stopping", "unknown")
        includeScreen.isEnabled = allowInput.isEnabled
        stop.visibility = if (writable && chat.requestId.isNotEmpty() && chat.state in AssistantRequest.pollingStates) View.VISIBLE else View.GONE
        stop.isEnabled = valid() && !busy && chat.state != "stopping"
    }
    private fun sendMessage() {
        if (!valid() || !available || busy || chat.state in setOf("running", "stopping", "unknown")) return
        val text = draft.text.toString().trim()
        try {
            val id = UUID.randomUUID().toString()
            chat = storage.get(chat.deviceId, chat.sessionId, chat.title)
            val payload = AssistantRequest.send(id, text, includeScreen.isChecked, chat.messages, allowInput.isChecked, includeScreen.isChecked)
            chat.requestId = id; chat.state = "running"; chat.detail = ""
            chat.messages.add(ChatMessage("user", text)); chat.draft = ""
            storage.save(chat); draft.setText(""); renderMessages(); execute(payload, "send")
        } catch (e: Exception) { draft.error = e.message }
    }
    private fun query(action: String) {
        if (!valid() || busy) return
        val payload = JSONObject().put("action", action)
        if (action == "poll" || action == "cancel") payload.put("request_id", chat.requestId)
        if (action == "cancel") { chat.state = "stopping"; storage.save(chat); stateChanged(chat) }
        execute(payload.toString(), action)
    }
    private fun execute(payload: String, action: String) {
        busy = true; updateState()
        val expectedId = chat.requestId
        val task: () -> Unit = {
            try {
                if (!closed) {
                    val result = JSONObject(request(payload))
                    ui.post {
                        if (valid()) {
                            save()
                            available = result.optBoolean("available", false)
                            chat = storage.response(chat.deviceId, chat.sessionId, chat.title, result, if (action == "status") null else expectedId)
                            busy = false; renderMessages(); updateState(); stateChanged(chat)
                            if (chat.state in AssistantRequest.pollingStates) ui.postDelayed(poll, 1000)
                        } else busy = false
                    }
                }
            } catch (e: Exception) {
                ui.post {
                    if (valid()) {
                        save()
                        chat = storage.get(chat.deviceId, chat.sessionId, chat.title)
                        if (action == "status" || chat.requestId == expectedId) {
                            chat.state = if (action == "status") "unavailable" else "unknown"
                            chat.detail = e.cause?.message ?: e.message ?: "连接失败"
                            storage.save(chat)
                        }
                        busy = false; updateState(); stateChanged(chat)
                    }
                }
            }
        }
        dispatch?.invoke(task) ?: worker!!.execute(task)
    }

    private fun startVoice() {
        if (!valid()) return
        if (!SpeechRecognizer.isRecognitionAvailable(activity)) { voiceState.text = "此设备未提供语音识别服务"; return }
        if (activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            pendingVoice = true; activity.requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), VOICE_PERMISSION); return
        }
        try {
            recognizer?.destroy()
            val speech = SpeechRecognizer.createSpeechRecognizer(activity); recognizer = speech
            speech.setRecognitionListener(object : RecognitionListener {
                override fun onReadyForSpeech(params: Bundle?) { if (recognizing) voiceState.text = "正在聆听…" }
                override fun onBeginningOfSpeech() {}
                override fun onRmsChanged(rmsdB: Float) {}
                override fun onBufferReceived(buffer: ByteArray?) {}
                override fun onEndOfSpeech() { if (recognizing) voiceState.text = "正在识别…" }
                override fun onError(error: Int) {
                    if (!recognizing) return
                    stopVoice(when (error) {
                        SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> "麦克风权限被拒绝"
                        SpeechRecognizer.ERROR_NO_MATCH, SpeechRecognizer.ERROR_SPEECH_TIMEOUT -> "未识别到语音，请重试"
                        SpeechRecognizer.ERROR_NETWORK, SpeechRecognizer.ERROR_NETWORK_TIMEOUT -> "语音服务网络不可用"
                        SpeechRecognizer.ERROR_RECOGNIZER_BUSY -> "语音识别服务忙碌"
                        else -> "语音识别不可用（$error）"
                    })
                }
                override fun onResults(results: Bundle?) {
                    if (!recognizing || !valid()) return
                    val text = results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull()
                    if (!text.isNullOrBlank()) { draft.setText((draft.text.toString() + if (draft.length() > 0) "\n$text" else text).take(4000)); draft.setSelection(draft.length()) }
                    stopVoice(if (text.isNullOrBlank()) "未识别到语音" else "已填入草稿")
                }
                override fun onPartialResults(partialResults: Bundle?) {}
                override fun onEvent(eventType: Int, params: Bundle?) {}
            })
            recognizing = true; voiceState.text = "正在启动语音识别…"; cancelVoice.visibility = View.VISIBLE; updateState()
            speech.startListening(Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
                putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
                putExtra(RecognizerIntent.EXTRA_LANGUAGE, Locale.getDefault().toLanguageTag())
                putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, false)
                putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 1)
            })
        } catch (e: Exception) { stopVoice(e.message ?: "无法启动语音识别") }
    }
    private fun stopVoice(message: String) {
        recognizing = false; pendingVoice = false
        recognizer?.cancel(); recognizer?.destroy(); recognizer = null
        cancelVoice.visibility = View.GONE; voiceState.text = message; updateState()
    }
    fun permissionResult(code: Int, results: IntArray) {
        if (code != VOICE_PERMISSION || !pendingVoice || closed) return
        pendingVoice = false
        if (results.firstOrNull() == PackageManager.PERMISSION_GRANTED) startVoice()
        else voiceState.text = "麦克风权限被拒绝，可在系统设置中开启"
    }
    fun pause() { paused = true; ui.removeCallbacks(poll); stopVoice("语音输入已停止"); save(); updateState() }
    fun close() {
        if (closed) return
        closed = true; ui.removeCallbacks(poll); ui.removeCallbacks(saveDraft)
        stopVoice(""); save(); worker?.shutdown()
    }
    companion object { const val VOICE_PERMISSION = 701 }
}
