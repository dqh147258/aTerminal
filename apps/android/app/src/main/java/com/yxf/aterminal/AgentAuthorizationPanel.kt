package com.yxf.aterminal

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.*
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.Executors

/** One instance owns one immutable account/Desktop/conversation. Never persists full authorization. */
class AgentAuthorizationPanel(
    private val activity: Activity,
    identity: List<String>,
    private val valid: () -> Boolean,
    private val request: (JSONObject) -> JSONObject,
    private val changed: () -> Unit,
    private val resolved: () -> Unit = {},
) {
    private val ui = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private val prefs = activity.getSharedPreferences("agent-authorization-${ChatStore.digest(JSONArray(identity).toString())}", Context.MODE_PRIVATE)
    @Volatile private var closed = false
    private var refreshing = false
    private var mutating = false
    private var permissions: JSONObject? = null
    private var permissionsFresh = false
    private var pending = emptyList<JSONObject>()
    private var pendingSignature = ""
    private var renderedReason: String? = null
    private var dialog: AlertDialog? = null
    private var dialogBody: LinearLayout? = null
    private var ruleRows: List<JSONObject>? = null
    private var rulesLoading = false
    private var error = ""
    private var actionError = ""
    private var uncertainPendingId: String? = null
    // An acknowledgement is not an execution result. Retire it only after an
    // authoritative pending refresh, never merely because time has elapsed.
    private data class ActionReceipt(val pendingId: String, val message: String)
    private var actionReceipt: ActionReceipt? = null
    @Volatile private var connectionRevision = 0
    private data class ApprovalDetails(val fingerprint: String, val text: String)
    private val details = mutableMapOf<String, ApprovalDetails>()
    private val detailsLoading = mutableSetOf<String>()
    private val detailsErrors = mutableMapOf<String, String>()
    val cards = activity.column().apply { tag = "agent-pending" }
    val entry = activity.iconButton(R.drawable.ic_lock, "权限 · 读取中") { showPermissions() }.apply { tag = "agent-permissions" }
    private val notice = activity.label("", 12f, Palette.warning)
    private val rows = activity.column()
    val permissionMode: String get() = permissions?.optString("permission_mode")?.takeIf { it in setOf("ask", "read_only") } ?: "ask"
    val canSend: Boolean get() = permissionsFresh && valid() && permissions?.optBoolean("can_mutate", false) == true

    init {
        cards.addView(notice)
        cards.addView(rows)
        // Cached cards are for recovery only; they cannot be answered until Desktop is read again.
        runCatching { JSONArray(prefs.getString("pending", "[]")) }.getOrNull()?.let { a ->
            pending = (0 until a.length()).map { a.getJSONObject(it) }
        }
        renderPending(true)
    }

    private fun rpc(command: JSONObject, mutation: Boolean = false): JSONObject {
        check(!closed && valid()) { "连接或对话已变化，请回到原对话重试" }
        if (mutation) check(canSend) { "当前设备操作权限尚未确认或仅可查看" }
        val result = request(command)
        check(!closed && valid()) { "连接或对话已变化，请刷新确认结果" }
        if (result.has("error") && !result.isNull("error") && result.optString("error").isNotBlank()) error(result.optString("error"))
        return result
    }

    private fun parsePermissions(value: JSONObject): JSONObject {
        val result = value.optJSONObject("permissions") ?: value
        check(result.optString("permission_mode") in setOf("ask", "read_only") && result.has("revision") && result.has("full_authorization")) {
            "Desktop 不支持新版授权，请更新 Desktop；不会自动启用完全授权"
        }
        result.getLong("revision")
        result.getBoolean("full_authorization")
        return result
    }

    private fun page(action: String): List<JSONObject> {
        val items = mutableListOf<JSONObject>(); val seen = mutableSetOf<String>(); var cursor: String? = null
        do {
            val command = JSONObject().put("action", action)
            cursor?.let { command.put("cursor", it) }
            val result = rpc(command)
            val array = result.optJSONArray("items") ?: result.optJSONArray(action) ?: error("Desktop 未返回${action}列表")
            for (i in 0 until array.length()) {
                val item = array.getJSONObject(i)
                if (action == "pending") check(item.optString("id").isNotBlank() && item.optString("kind") in setOf("approval", "question")) { "Desktop 待办格式不正确，请刷新" }
                items.add(item)
            }
            cursor = result.optString("cursor").takeUnless { it.isBlank() || it == "null" }
            check(!result.optBoolean("has_more") || cursor != null) { "待办分页不完整，请刷新" }
            check(cursor == null || seen.add(cursor)) { "待办分页已失效，请刷新" }
            check(items.size <= 2000 && seen.size <= 100) { "待办列表过大，请在 Desktop 处理后刷新" }
        } while (cursor != null)
        return items
    }

    /** Retain review cards/drafts, but invalidate reads from the old transport. */
    fun connectionInterrupted() {
        connectionRevision++
        refreshing = false
        rulesLoading = false
        detailsLoading.clear()
        permissionsFresh = false
        error = "连接暂时中断 · 待办和回答已保留"
        renderStatus(); renderPending(); renderDialog(); changed()
    }

    fun refresh() {
        if (closed) return
        if (!valid()) {
            permissionsFresh = false; error = "Desktop 未连接 · 已保留待办"
            renderStatus(); renderPending(); changed(); return
        }
        if (refreshing || mutating) return
        refreshing = true
        val revision = connectionRevision
        worker.execute {
            val permissionResult = runCatching { parsePermissions(rpc(JSONObject().put("action", "permissions"))) }
            val pendingResult = runCatching { page("pending") }
            ui.post {
                if (closed || revision != connectionRevision) return@post
                refreshing = false
                permissionsFresh = permissionResult.isSuccess && valid()
                permissionResult.getOrNull()?.let { permissions = it }
                // Only a complete successful page replaces cards. Failed reads retain them.
                pendingResult.getOrNull()?.takeIf { valid() }?.let {
                    pending = it
                    actionReceipt?.let { receipt ->
                        if (it.none { row -> row.optString("id") == receipt.pendingId && row.optString("state") == "resolved" }) actionReceipt = null
                    }
                    uncertainPendingId?.let { id ->
                        val row = it.firstOrNull { item -> item.optString("id") == id }
                        if (row == null || !pendingActive(row)) {
                            // This retires an obsolete retry warning, not a claim
                            // that the approved operation executed successfully.
                            uncertainPendingId = null; actionError = ""
                        }
                    }
                    details.keys.toList().filter { id -> it.none { row -> row.optString("id") == id && row.optString("fingerprint") == details[id]?.fingerprint } }.forEach { id -> details.remove(id) }
                    prefs.edit().putString("pending", JSONArray(it).toString()).apply()
                }
                error = permissionResult.exceptionOrNull()?.message ?: pendingResult.exceptionOrNull()?.message.orEmpty()
                renderStatus(); renderPending(); renderDialog(); changed()
            }
        }
    }

    private fun blocked(): String? = when {
        !valid() -> "连接已变化，等待重新连接"
        !permissionsFresh -> "尚未确认 Desktop 权限，请刷新"
        permissions?.has("can_mutate") != true -> "Desktop 未提供设备操作权限，请更新后重试"
        permissions?.optBoolean("can_mutate", false) != true -> "当前设备只有查看权限"
        mutating -> "正在提交，请稍候"
        else -> null
    }
    private fun renderStatus() {
        entry.contentDescription = when {
            !permissionsFresh -> "权限 · 未同步"
            permissionMode == "read_only" -> "权限 · 只读"
            permissions?.optBoolean("full_authorization") == true -> "权限 · 完全授权"
            else -> "权限 · 按需询问"
        }
        notice.text = listOf(actionReceipt?.message.orEmpty(), actionError, error).filter { it.isNotBlank() }.joinToString("\n")
        notice.visibility = if (notice.text.isBlank()) View.GONE else View.VISIBLE
    }
    private fun button(text: String, tag: String, enabled: Boolean = true, action: () -> Unit) = activity.actionButton(text, action = action).apply {
        this.tag = tag; isEnabled = enabled; alpha = if (enabled) 1f else .45f
        layoutParams = LinearLayout.LayoutParams(-1, -2).apply { topMargin = activity.dp(6) }
    }
    private fun requiresDetails(item: JSONObject) = item.optBoolean("requires_details") || item.optBoolean("arguments_truncated") || item.optBoolean("truncated")
    private fun pendingActive(item: JSONObject) = item.optString("state", "pending") in setOf("pending", "waiting", "waiting_for_user", "waiting_for_approval")
    private fun renderPending(force: Boolean = false) { with(activity) {
        val reason = blocked()
        val signature = JSONObject().put("pending", JSONArray(pending)).put("details", JSONArray(details.keys.sorted()))
            .put("loading", JSONArray(detailsLoading.sorted())).put("errors", JSONObject(detailsErrors.toMap())).toString()
        if (!force && signature == pendingSignature && reason == renderedReason) return
        pendingSignature = signature; renderedReason = reason
        val focused = (rows.findFocus() as? EditText)?.takeIf { it.tag?.toString()?.startsWith("answer:") == true }
        val focusedTag = focused?.tag
        val selectionStart = focused?.selectionStart ?: 0
        val selectionEnd = focused?.selectionEnd ?: 0
        rows.removeAllViews()
        pending.forEach { item ->
            val id = item.getString("id")
            val card = column(12).apply { tag = "pending:$id"; background = shape(Palette.surface, true) }
            card.addView(heading(item.optString("title").ifBlank { if (item.optString("kind") == "question") "需要你的回答" else "操作授权" }))
            card.addView(label("目标 Session：${item.optString("session_id").takeUnless { it.isBlank() || it == "null" } ?: "全局助手"}", 12f, Palette.accent))
            card.addView(label("Run：${item.optString("run_id")}", 12f, Palette.muted))
            item.optString("reason").takeIf { it.isNotBlank() }?.let { card.addView(label(it, 13f, Palette.secondary)) }
            if (!pendingActive(item)) {
                val status = when {
                    item.optString("state") == "denied" || item.optJSONObject("response")?.optString("decision") == "deny" -> "已拒绝本次操作"
                    item.optString("state") == "resolved" -> if (item.optString("kind") == "question") "回答已确认，正在恢复任务" else "授权已确认，正在恢复任务"
                    item.optString("state") == "consumed" -> "答复已处理；执行结果请查看任务进展"
                    item.optString("state") in setOf("expired", "interrupted", "cancelled") -> "请求已失效：${item.optString("state")}；请发送新任务"
                    else -> "请求状态：${item.optString("state")}"
                }
                card.addView(label(status, 13f, Palette.muted))
            } else if (item.optString("kind") == "approval") {
                card.addView(label("工具：${item.optString("tool")}\ncwd：${item.optString("cwd", "未知")}", 13f))
                card.addView(label(item.opt("arguments_preview")?.toString().orEmpty(), 13f).apply { setTextIsSelectable(true) })
                val rulePreview = item.opt("rule_preview")?.toString()?.takeUnless { it == "null" }.orEmpty()
                if (rulePreview.isNotBlank()) card.addView(label("永久规则范围：$rulePreview", 12f, Palette.secondary))
                val needDetails = requiresDetails(item)
                val fullDetails = details[id]?.takeIf { it.fingerprint == item.optString("fingerprint") }
                if (needDetails) {
                    if (fullDetails != null) {
                        card.addView(label("完整操作详情（已脱敏）", 12f, Palette.accent))
                        card.addView(label(fullDetails.text, 13f).apply { setTextIsSelectable(true); tag = "approval-details:$id" })
                    } else {
                        card.addView(label("操作预览已截断。读取完整详情后才能授权一次或永久授权。", 12f, Palette.warning))
                        detailsErrors[id]?.let { card.addView(label(it, 12f, Palette.warning)) }
                        card.addView(button(if (id in detailsLoading) "正在读取完整详情…" else "读取完整操作详情", "details:$id", valid() && id !in detailsLoading) { loadDetails(item) })
                    }
                }
                val canApprove = reason == null && (!needDetails || fullDetails != null)
                card.addView(button("授权一次", "once:$id", canApprove) { resolve(item, "once", null) })
                val canAlways = item.optBoolean("can_always")
                card.addView(button("永久授权", "always:$id", canApprove && canAlways) { resolve(item, "always", null) })
                if (!canAlways) card.addView(label(item.optString("always_unavailable_reason").ifBlank { "无法固定程序、参数、cwd 或工具版本；终端交互仅支持本次授权。" }, 12f, Palette.warning))
                card.addView(button("拒绝", "deny:$id", reason == null) { resolve(item, "deny", null) })
            } else if (item.optString("kind") == "question") {
                card.addView(label(item.optString("question")))
                val input = field("自由输入回答").apply {
                    tag = "answer:$id"; isSingleLine = false; maxLines = 5
                    inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
                    imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_EXTRACT_UI
                    setText(prefs.getString("answer:$id", "")); isEnabled = reason == null
                }
                item.optJSONArray("options")?.let { options -> for (i in 0 until options.length()) {
                    val option = options.get(i)
                    val title = if (option is JSONObject) option.optString("label", option.optString("title")) else option.toString()
                    card.addView(button(title, "option:$id:$i", reason == null) { input.setText(title) })
                } }
                input.addTextChangedListener(watcher { prefs.edit().putString("answer:$id", input.text.toString()).apply() })
                card.addView(input)
                val answerButton = button("提交回答", "answer-submit:$id", reason == null && input.text.isNotBlank()) {
                    resolve(item, null, input.text.toString().trim())
                }
                input.addTextChangedListener(watcher { answerButton.isEnabled = blocked() == null && input.text.isNotBlank(); answerButton.alpha = if (answerButton.isEnabled) 1f else .45f })
                card.addView(answerButton)
            }
            reason?.let { card.addView(label(it, 12f, Palette.warning)) }
            rows.addView(card, LinearLayout.LayoutParams(-1, -2).apply { bottomMargin = dp(12) })
        }
        if (focusedTag != null) (rows.findViewWithTag<View>(focusedTag) as? EditText)?.takeIf { it.isEnabled }?.let { input ->
            input.requestFocus()
            input.setSelection(selectionStart.coerceIn(0, input.length()), selectionEnd.coerceIn(0, input.length()))
        }
    } }

    private fun loadDetails(item: JSONObject) {
        val id = item.getString("id")
        if (closed || !valid() || !detailsLoading.add(id)) return
        detailsErrors.remove(id); renderPending()
        val fingerprint = item.optString("fingerprint")
        val revision = connectionRevision
        worker.execute {
            val result = runCatching {
                check(fingerprint.isNotBlank()) { "Desktop 未提供操作指纹，无法核对完整详情" }
                val text = StringBuilder(); val seen = mutableSetOf<String>(); var cursor: String? = null; var bytes = 0
                do {
                    val command = JSONObject().put("action", "approval_details").put("pending_id", id)
                    cursor?.let { command.put("cursor", it) }
                    val page = rpc(command)
                    check(page.getString("pending_id") == id && page.getString("fingerprint") == fingerprint) { "操作详情已变化，请刷新待办" }
                    check(page.has("truncated") && !page.getBoolean("truncated")) { "Desktop 未提供完整操作详情" }
                    val chunk = page.getString("text")
                    bytes += chunk.toByteArray(Charsets.UTF_8).size
                    check(bytes <= 1024 * 1024) { "操作详情超过展示限制，请在 Desktop 处理" }
                    text.append(chunk)
                    cursor = page.optString("cursor").takeUnless { it.isBlank() || it == "null" }
                    check(!page.getBoolean("has_more") || cursor != null) { "操作详情分页不完整，请重试" }
                    check(cursor == null || seen.add(cursor)) { "操作详情分页已失效，请重试" }
                    check(seen.size <= 256) { "操作详情分页超过限制" }
                } while (cursor != null)
                check(text.isNotEmpty()) { "操作详情为空，请重试" }
                ApprovalDetails(fingerprint, text.toString())
            }
            ui.post {
                if (closed || revision != connectionRevision) return@post
                detailsLoading.remove(id)
                if (valid() && pending.any { it.optString("id") == id && it.optString("fingerprint") == fingerprint }) {
                    result.fold({ details[id] = it }, { detailsErrors[id] = "完整详情读取失败：${it.message}；可重试或拒绝" })
                }
                renderPending()
            }
        }
    }

    /** Persist an exact payload/request pair before sending, so lost acknowledgements survive reopening. */
    private fun idempotencyKey(command: JSONObject) = "request:${ChatStore.digest(JSONObject(command.toString()).apply { remove("request_id") }.toString())}"
    private fun idempotent(command: JSONObject): JSONObject {
        val key = idempotencyKey(command)
        val id = prefs.getString(key, null) ?: UUID.randomUUID().toString().also { prefs.edit().putString(key, it).commit() }
        return command.put("request_id", id)
    }
    private fun mutate(command: JSONObject, done: (JSONObject) -> Unit) {
        if (closed || blocked() != null) return
        mutating = true; actionError = ""; uncertainPendingId = null; actionReceipt = null; renderStatus(); renderPending(); renderDialog()
        val revision = connectionRevision
        val pendingId = command.optString("pending_id").takeIf { command.optString("action") == "resolve" && it.isNotBlank() }
        worker.execute {
            val result = runCatching {
                check(revision == connectionRevision) { "连接已变化；待确认操作未自动重发" }
                rpc(command, true)
            }
            ui.post {
                if (closed) return@post
                mutating = false
                if (!valid() || revision != connectionRevision) {
                    permissionsFresh = false; actionError = "连接已变化；请回到原对话刷新确认结果"; uncertainPendingId = pendingId
                } else result.fold({ value ->
                    runCatching { done(value) }.onFailure {
                        actionError = it.message.orEmpty(); uncertainPendingId = pendingId; permissionsFresh = false
                    }
                }, {
                    actionError = "提交未确认：${it.message}；待办和回答已保留，可重试"; uncertainPendingId = pendingId
                })
                renderStatus(); renderPending(); renderDialog(); changed()
                refresh()
            }
        }
    }
    private fun resolve(item: JSONObject, decision: String?, answer: String?) {
        val command = JSONObject().put("action", "resolve").put("pending_id", item.getString("id"))
        if (decision != null && decision != "deny" && requiresDetails(item)) {
            val fullDetails = details[item.getString("id")]?.takeIf { it.fingerprint == item.optString("fingerprint") } ?: return
            command.put("fingerprint", fullDetails.fingerprint).put("details_ack", true)
        }
        decision?.let { command.put("decision", it) }; answer?.let { if (it.isBlank()) return; command.put("answer", it) }
        mutate(idempotent(command)) { result ->
            val resolved = result.getJSONObject("pending")
            check(resolved.getString("id") == item.getString("id") && !pendingActive(resolved)) { "Desktop 尚未确认本次回答，请刷新重试" }
            pending = pending.map { if (it.getString("id") == resolved.getString("id")) resolved else it }
            prefs.edit().remove("answer:${item.getString("id")}").apply()
            actionReceipt = if (resolved.optString("state") == "resolved") ActionReceipt(
                item.getString("id"), when (decision) {
                    "deny" -> "已拒绝本次操作"
                    null -> "回答已确认，正在恢复任务"
                    else -> "授权已确认，正在恢复任务"
                }
            ) else null
            if (decision == "deny") actionError = "已拒绝本次操作"
            this.resolved()
        }
    }
    private fun showPermissions() {
        if (closed) return
        dialog?.dismiss()
        dialogBody = activity.column(16)
        dialog = AlertDialog.Builder(activity).setTitle("当前对话权限")
            .setView(activity.scroll(dialogBody!!)).setNegativeButton("关闭", null).create()
        val opened = dialog
        opened?.setOnDismissListener { if (dialog === opened) { dialog = null; dialogBody = null } }
        renderDialog(); opened?.show(); refresh()
    }
    private fun setPermissions(mode: String? = null, full: Boolean? = null) {
        val actual = permissions ?: return
        val command = JSONObject().put("action", "set_permissions").put("expected_revision", actual.getLong("revision"))
        mode?.let { command.put("permission_mode", it) }; full?.let { command.put("full_authorization", it) }
        mutate(command) { result -> permissions = parsePermissions(result); permissionsFresh = true }
    }
    private fun loadRules() {
        if (closed || rulesLoading || !valid()) return
        rulesLoading = true; renderDialog()
        val revision = connectionRevision
        worker.execute {
            val result = runCatching { page("rules") }
            ui.post { if (!closed && revision == connectionRevision) {
                rulesLoading = false
                result.fold({ ruleRows = it }, { error = "规则读取失败：${it.message}；原列表已保留" })
                renderDialog()
            } }
        }
    }
    private fun renderDialog() { with(activity) {
        val content = dialogBody ?: return
        content.removeAllViews()
        content.addView(label(entry.contentDescription.toString(), 16f, Palette.accent))
        content.addView(label("按需询问：安全操作自动执行，其他操作逐项授权。\n完全授权：当前对话及委托任务持续生效，直到手动关闭。", 13f, Palette.secondary))
        permissions?.let { content.addView(label("Desktop revision：${it.optLong("revision")}", 12f, Palette.muted)) }
        val reason = blocked()
        reason?.let { content.addView(label(it, 13f, Palette.warning)) }
        if (error.isNotBlank()) content.addView(label(error, 13f, Palette.warning))
        if (actionError.isNotBlank()) content.addView(label(actionError, 13f, Palette.warning))
        content.addView(button("按需询问", "mode:ask", reason == null && permissionMode != "ask") { setPermissions(mode = "ask") })
        content.addView(button("只读", "mode:read_only", reason == null && permissionMode != "read_only") { setPermissions(mode = "read_only", full = false) })
        val full = permissionsFresh && permissions?.optBoolean("full_authorization") == true && permissionMode != "read_only"
        content.addView(Switch(activity).apply {
            text = "完全授权"; tag = "full-authorization"; setTextColor(Palette.text)
            minHeight = dp(56); isChecked = full; isEnabled = reason == null && permissionMode == "ask"
            setOnCheckedChangeListener { _, desired ->
                // Keep displaying the confirmed Desktop value while the request is outstanding.
                isChecked = full
                if (desired != full) setPermissions(full = desired)
            }
        })
        content.addView(button("刷新权限与待办", "authorization-refresh", !refreshing && !mutating) { refresh() })
        content.addView(button(if (rulesLoading) "正在读取规则…" else "永久授权规则", "authorization-rules", !rulesLoading) { loadRules() })
        ruleRows?.let { rules ->
            if (rules.isEmpty()) content.addView(label("没有永久授权规则", 13f, Palette.muted))
            rules.forEach { rule ->
                val id = rule.optString("id", rule.optString("rule_id"))
                content.addView(label(rule.opt("rule_preview")?.toString() ?: rule.opt("preview")?.toString() ?: rule.toString(), 13f).apply { setTextIsSelectable(true) })
                content.addView(button("撤销规则", "revoke:$id", reason == null && id.isNotBlank()) {
                    val operation = JSONObject().put("action", "revoke_rule").put("rule_id", id)
                    val nonceKey = idempotencyKey(operation)
                    val command = idempotent(operation)
                    mutate(command) { result ->
                        check(result.optString("rule_id") == id && result.has("revoked")) { "Desktop 未确认本次撤销，请刷新重试" }
                        result.getBoolean("revoked")
                        // A confirmed revoke finishes this operation. Regranting the same rule needs a new nonce.
                        prefs.edit().remove(nonceKey).commit()
                        loadRules()
                    }
                })
            }
        }
    } }
    fun close() { closed = true; dialog?.dismiss(); worker.shutdown() }
}
