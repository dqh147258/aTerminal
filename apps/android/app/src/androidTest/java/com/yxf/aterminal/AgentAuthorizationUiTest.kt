package com.yxf.aterminal

import android.content.Intent
import android.graphics.Rect
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.*
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Test
import java.util.UUID
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Native UI behavior against isolated, deterministic RPCs. No accounts, PTYs or models. */
class AgentAuthorizationUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private var authorization: AgentAuthorizationPanel? = null
    private var chat: AgentPanel? = null
    private lateinit var root: LinearLayout
    private lateinit var scroll: ScrollView
    private val identity = listOf("authorization-ui", UUID.randomUUID().toString(), "desktop", "session")
    private val connected = AtomicBoolean(true)
    private val calls = CopyOnWriteArrayList<JSONObject>()
    private var permissions = JSONObject().put("permission_mode", "ask").put("full_authorization", false).put("revision", 0).put("can_mutate", true)
    private var pending = mutableListOf<JSONObject>()
    private var rules = mutableListOf<JSONObject>()
    private var intercept: ((JSONObject) -> JSONObject?)? = null

    private fun <T> main(action: () -> T): T {
        var value: T? = null; var failure: Throwable? = null
        instrumentation.runOnMainSync { try { value = action() } catch (e: Throwable) { failure = e } }
        failure?.let { throw it }; @Suppress("UNCHECKED_CAST") return value as T
    }
    private fun all(v: View): List<View> = listOf(v) + if (v is ViewGroup) (0 until v.childCount).flatMap { all(v.getChildAt(it)) } else emptyList()
    private fun views(): List<View> = all(root) + windows().flatMap { all(it) }
    private fun windows(): List<View> = if (::activity.isInitialized) listOfNotNull(dialog()?.window?.decorView) else emptyList()
    private fun dialog(): android.app.AlertDialog? = authorization?.let { AgentAuthorizationPanel::class.java.getDeclaredField("dialog").apply { isAccessible = true }.get(it) as? android.app.AlertDialog }
    private fun tagged(tag: String) = views().first { it.tag == tag }
    private fun waitFor(name: String, condition: () -> Boolean) {
        val end = SystemClock.elapsedRealtime() + 10000
        while (SystemClock.elapsedRealtime() < end) { if (condition()) return; Thread.sleep(30) }
        fail("Timed out: $name")
    }
    private fun page(items: List<JSONObject>) = JSONObject().put("items", JSONArray(items)).put("cursor", JSONObject.NULL).put("has_more", false)
    private fun rpc(command: JSONObject): JSONObject {
        calls.add(JSONObject(command.toString()))
        intercept?.invoke(command)?.let { return it }
        return synchronized(this) {
            when (command.getString("action")) {
                "permissions" -> JSONObject(permissions.toString())
                "pending" -> page(pending)
                "rules" -> page(rules)
                "set_permissions" -> {
                    check(command.getLong("expected_revision") == permissions.getLong("revision")) { "permission_revision_conflict" }
                    for (key in listOf("permission_mode", "full_authorization")) if (command.has(key)) permissions.put(key, command.get(key))
                    permissions.put("revision", permissions.getLong("revision") + 1)
                    JSONObject(permissions.toString())
                }
                "resolve" -> {
                    val item = pending.first { it.getString("id") == command.getString("pending_id") }
                    val result = JSONObject(item.toString()).put("state", if (command.optString("decision") == "deny") "denied" else "resolved")
                    pending.remove(item)
                    JSONObject().put("pending", result).put("duplicate", false)
                }
                "revoke_rule" -> { rules.removeAll { it.getString("id") == command.getString("rule_id") }; JSONObject().put("revoked", true).put("rule_id", command.getString("rule_id")) }
                "state" -> JSONObject().put("state", "waiting_for_user").put("history_generation", 0)
                "history" -> page(emptyList()).put("generation", 0)
                "send" -> JSONObject().put("state", "running")
                "cancel" -> JSONObject().put("state", "stopping")
                else -> error("Unexpected ${command.getString("action")}")
            }
        }
    }
    private fun approval(id: String, always: Boolean = true) = JSONObject().put("id", id).put("kind", "approval").put("state", "pending")
        .put("title", "需要授权：执行命令").put("reason", "命令需要用户确认").put("session_id", "delegated-session")
        .put("run_id", "child-run").put("tool", "run_program").put("arguments_preview", "{\"program\":\"/usr/bin/tee\",\"args\":[\"-a\",\"report.txt\"],\"stdin\":\"line\\n\"}")
        .put("cwd", "/tmp/fixture").put("can_always", always).put("rule_preview", "相同命令、参数、目录和版本")
    private fun question() = JSONObject().put("id", "q").put("kind", "question").put("state", "pending")
        .put("title", "请补充需求").put("question", "选择输出格式").put("options", JSONArray(listOf("JSON", "文字"))).put("session_id", "s").put("run_id", "r")
    private fun launch(useChat: Boolean = false, sessionChat: Boolean = false) {
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui", true))
        waitFor("activity") { main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull { it.hasWindowFocus() }?.also { activity = it } != null } }
        main {
            root = activity.column().apply { setBackgroundColor(Palette.background) }
            activity.setContentView(root)
            if (useChat || sessionChat) {
                root.addView(activity.row().apply { addView(activity.heading("Agent")); addView(activity.label("关闭")) })
                val global = JSONObject().put("scope", JSONObject().put("agent", "global-agent")).put("title", "测试对话")
                chat = AgentPanel(activity, root, identity, if (sessionChat) "s" else "", { connected.get() }, { session, raw ->
                    assertEquals(if (sessionChat) "s" else "", session)
                    val command = JSONObject(raw)
                    if (sessionChat) assertFalse(command.has("agent_id")) else assertEquals("global-agent", command.getString("agent_id"))
                    rpc(command).toString()
                }, {}, cachePath = context.cacheDir.resolve("authorization-${UUID.randomUUID()}.sqlite3").path, globalConversation = if (sessionChat) null else global)
                authorization = AgentPanel::class.java.getDeclaredField("authorization").apply { isAccessible = true }.get(chat) as AgentAuthorizationPanel
            } else mount()
        }
        waitFor("permissions loaded") { main { authorization!!.canSend } }
    }
    private fun mount() {
        root.removeAllViews()
        authorization = AgentAuthorizationPanel(activity, identity, { connected.get() }, ::rpc, {})
        root.addView(authorization!!.entry)
        scroll = activity.scroll(authorization!!.cards)
        root.grow(scroll)
        authorization!!.refresh()
    }
    @After fun close() { if (::activity.isInitialized) main { chat?.close(); authorization?.close(); activity.finish() } }

    @Test fun decisionsShowScopeDisableUnstableRulesAndRevoke() {
        pending.addAll(listOf(approval("once"), approval("always"), approval("deny", false)))
        rules.add(JSONObject().put("id", "rule").put("rule_preview", "touch report.txt /tmp/fixture"))
        launch()
        main {
            assertTrue(views().filterIsInstance<TextView>().any { it.text.contains("delegated-session") })
            assertFalse(tagged("always:deny").isEnabled)
            assertTrue(views().filterIsInstance<TextView>().any { it.text.contains("仅支持本次授权") })
            tagged("once:once").performClick()
        }
        waitFor("once resolved") { calls.any { it.optString("decision") == "once" } && main { views().none { it.tag == "once:once" } } }
        main { tagged("always:always").performClick() }
        waitFor("always resolved") { calls.any { it.optString("decision") == "always" } && main { views().none { it.tag == "always:always" } } }
        main { tagged("deny:deny").performClick() }
        waitFor("denied explicitly") { main { views().filterIsInstance<TextView>().any { it.text.contains("已拒绝本次操作") } } }
        main { authorization!!.entry.performClick(); tagged("authorization-rules").performClick() }
        waitFor("rules loaded") { main { views().any { it.tag == "revoke:rule" } } }
        main { tagged("revoke:rule").performClick() }
        waitFor("rule revoked") { calls.any { it.optString("action") == "revoke_rule" } && main { views().filterIsInstance<TextView>().any { it.text == "没有永久授权规则" } } }
    }

    @Test fun lostResponseRetriesSameIdAndQuestionSurvivesReopening() {
        pending.add(question())
        val fail = AtomicBoolean(true)
        intercept = { if (it.optString("action") == "resolve" && fail.getAndSet(false)) error("offline after send") else null }
        launch()
        main { tagged("option:q:0").performClick(); assertEquals("JSON", (tagged("answer:q") as EditText).text.toString()); (tagged("answer:q") as EditText).setText("JSON，加上说明"); tagged("answer-submit:q").performClick() }
        waitFor("failure retained") { main { views().filterIsInstance<TextView>().any { it.text.contains("提交未确认") } && tagged("answer-submit:q").isEnabled } }
        val first = calls.first { it.optString("action") == "resolve" }
        main { authorization!!.close(); mount() }
        waitFor("restored answer") { main { authorization!!.canSend && (tagged("answer:q") as EditText).text.toString() == "JSON，加上说明" } }
        main { tagged("answer-submit:q").performClick() }
        waitFor("retry resolved") { calls.count { it.optString("action") == "resolve" } == 2 }
        val retry = calls.last { it.optString("action") == "resolve" }
        assertEquals(first.getString("request_id"), retry.getString("request_id"))
        assertEquals("JSON，加上说明", retry.getString("answer"))
        assertFalse(retry.has("decision"))
    }

    @Test fun fullAuthorizationWaitsForDesktopAndConflictReadsActualRevision() {
        launch()
        val started = CountDownLatch(1); val reply = CountDownLatch(1)
        intercept = { command ->
            if (command.optString("action") == "set_permissions") { started.countDown(); check(reply.await(8, TimeUnit.SECONDS)); null } else null
        }
        try {
            main { authorization!!.entry.performClick(); (tagged("full-authorization") as Switch).performClick() }
            assertTrue(started.await(5, TimeUnit.SECONDS))
            main { assertFalse((tagged("full-authorization") as Switch).isChecked); assertFalse(tagged("full-authorization").isEnabled) }
            reply.countDown()
            waitFor("server enabled full") { main { (tagged("full-authorization") as Switch).isChecked } }
            assertEquals(0, calls.first { it.optString("action") == "set_permissions" }.getInt("expected_revision"))
            synchronized(this) { permissions.put("revision", 9).put("full_authorization", false) }
            main { (tagged("full-authorization") as Switch).performClick() }
            waitFor("conflict reread") { main { !(tagged("full-authorization") as Switch).isChecked && views().filterIsInstance<TextView>().any { it.text.contains("permission_revision_conflict") } } }
            main { tagged("mode:read_only").performClick() }
            waitFor("read only") { main { authorization!!.permissionMode == "read_only" && !tagged("full-authorization").isEnabled } }
        } finally { reply.countDown() }
    }

    @Test fun pendingReadFailureRetainsCardsAndReadonlyDeviceCannotAnswer() {
        pending.add(approval("one")); launch()
        intercept = { if (it.optString("action") == "pending") error("pending offline") else null }
        synchronized(this) { permissions.put("can_mutate", false) }
        main { authorization!!.refresh() }
        waitFor("read only cards retained") { main { !tagged("once:one").isEnabled && views().filterIsInstance<TextView>().any { it.text.contains("pending offline") } } }
        main { tagged("once:one").performClick(); authorization!!.entry.performClick(); assertFalse(tagged("full-authorization").isEnabled) }
        assertFalse(calls.any { it.optString("action") in setOf("resolve", "set_permissions") })
    }

    @Test fun oldDesktopCannotSilentlyFallBackToAllowInput() {
        launch()
        intercept = { if (it.optString("action") == "permissions") JSONObject() else null }
        main { authorization!!.refresh() }
        waitFor("unsupported") { main { !authorization!!.canSend && views().filterIsInstance<TextView>().any { it.text.contains("不支持新版授权") } } }
        main { authorization!!.entry.performClick(); assertFalse(tagged("full-authorization").isEnabled) }
    }

    @Test fun invalidatedScopeDropsQueuedMutation() {
        pending.add(approval("one")); launch()
        val started = CountDownLatch(1); val release = CountDownLatch(1)
        intercept = { command -> if (command.optString("action") == "rules") { started.countDown(); check(release.await(8, TimeUnit.SECONDS)); page(emptyList()) } else null }
        try {
            main { authorization!!.entry.performClick(); tagged("authorization-rules").performClick() }
            assertTrue(started.await(5, TimeUnit.SECONDS))
            main { dialog()!!.dismiss(); tagged("once:one").performClick(); connected.set(false) }
            release.countDown()
            waitFor("invalidated message") { main { views().filterIsInstance<TextView>().any { it.text.contains("连接已变化") } } }
            assertFalse(calls.any { it.optString("action") == "resolve" })
        } finally { release.countDown() }
    }

    @Test fun globalDelegatedApprovalAndWaitingRunCanStopAndSendExplicitAsk() {
        pending.add(approval("child")); launch(useChat = true)
        waitFor("waiting stop") { main { views().any { it.contentDescription == "停止" && it.isEnabled } } }
        main { tagged("once:child").performClick() }
        waitFor("delegated response") { calls.any { it.optString("pending_id") == "child" } }
        main { views().first { it.contentDescription == "停止" }.performClick() }
        waitFor("cancel") { calls.any { it.optString("action") == "cancel" } }
        // A later new task uses confirmed ask, never the legacy allow_input or full from drafts.
        main { AgentPanel::class.java.getDeclaredField("runState").apply { isAccessible = true }.set(chat, "idle")
            (views().first { it.contentDescription == "发送任务或追加消息" } as EditText).setText("下一项")
            views().first { it.contentDescription == "发送" }.performClick() }
        waitFor("send") { calls.any { it.optString("action") == "send" } }
        val sent = calls.first { it.optString("action") == "send" }
        assertEquals("ask", sent.getString("permission_mode")); assertFalse(sent.has("allow_input")); assertFalse(sent.has("full_authorization"))
        assertTrue(GlobalConversationStore.running("waiting_for_user"))
        assertEquals("等待回答", GlobalConversationStore.stateLabel("waiting_for_user"))
    }

    @Test fun missingDeviceGrantDisablesControlsAndCachedFullIsNeverRestored() {
        permissions.put("full_authorization", true)
        launch()
        main { authorization!!.entry.performClick(); assertTrue((tagged("full-authorization") as Switch).isChecked); dialog()!!.dismiss(); authorization!!.close() }
        permissions.remove("can_mutate")
        main { mount() }
        main { authorization!!.entry.performClick() }
        waitFor("missing grant is visible") { main { views().filterIsInstance<TextView>().any { it.text.contains("未提供设备操作权限") } } }
        main { assertFalse(authorization!!.canSend); assertFalse(tagged("full-authorization").isEnabled); dialog()!!.dismiss(); authorization!!.close() }
        intercept = { if (it.optString("action") == "permissions") error("offline") else null }
        main { mount(); authorization!!.entry.performClick(); assertFalse((tagged("full-authorization") as Switch).isChecked) }
        waitFor("offline remains unconfirmed") { main { !authorization!!.canSend && !tagged("full-authorization").isEnabled } }
    }

    @Test fun pendingPaginationAndMalformedRefreshKeepPreviousCards() {
        pending.add(approval("first"))
        launch()
        intercept = { command -> if (command.optString("action") == "pending") {
            if (!command.has("cursor")) page(listOf(approval("first"))).put("cursor", "next").put("has_more", true)
            else page(listOf(approval("second")))
        } else null }
        main { authorization!!.refresh() }
        waitFor("both pages shown") { main { views().any { it.tag == "pending:first" } && views().any { it.tag == "pending:second" } } }
        assertTrue(calls.any { it.optString("action") == "pending" && it.optString("cursor") == "next" })
        intercept = { if (it.optString("action") == "pending") page(listOf(JSONObject().put("kind", "approval"))) else null }
        main { authorization!!.refresh() }
        waitFor("malformed page retains cards") { main { views().filterIsInstance<TextView>().any { it.text.contains("格式不正确") } && views().any { it.tag == "pending:second" } } }
    }

    @Test fun endedPendingIsVisibleButCannotBeApprovedOrCalledCompleted() {
        pending.add(approval("expired").put("state", "expired"))
        pending.add(approval("denied").put("state", "resolved").put("response", JSONObject().put("decision", "deny")))
        launch()
        main {
            assertFalse(views().any { it.tag == "once:expired" || it.tag == "once:denied" })
            assertTrue(views().filterIsInstance<TextView>().any { it.text.contains("请求已失效") })
            assertTrue(views().filterIsInstance<TextView>().any { it.text.contains("已拒绝本次操作") })
            assertFalse(views().filterIsInstance<TextView>().any { it.text.contains("已完成") })
        }
    }

    @Test fun pendingAnswerFocusSurvivesOtherRunsHistoryUpdates() {
        pending.add(question()); launch(useChat = true)
        val phase = java.util.concurrent.atomic.AtomicInteger()
        intercept = { command -> if (command.optString("action") == "history") page(listOf(JSONObject().put("id", "status").put("kind", "assistant").put("sequence", 1)
            .put("value", JSONObject().put("text", "其他任务进度 ${phase.get()}")))).put("generation", 0) else null }
        main { val input = tagged("answer:q") as EditText; input.setText("保留我的回答草稿"); input.requestFocus(); input.setSelection(2, 5) }
        repeat(2) { index ->
            phase.set(index + 1)
            waitFor("history update ${index + 1}") { main { views().filterIsInstance<TextView>().any { it.text.toString() == "其他任务进度 ${index + 1}" } } }
            main { val input = tagged("answer:q") as EditText; assertTrue(input.hasFocus()); assertEquals("保留我的回答草稿", input.text.toString()); assertEquals(2, input.selectionStart); assertEquals(5, input.selectionEnd) }
        }
        main { root.layoutParams = FrameLayout.LayoutParams(-1, activity.dp(420)); root.requestLayout() }
        waitFor("answer pair survives viewport resize") { main {
            val viewport = AgentPanel::class.java.getDeclaredField("scroll").apply { isAccessible = true }.get(chat) as ScrollView
            val visible = Rect(); viewport.getGlobalVisibleRect(visible)
            listOf("answer:q", "answer-submit:q").all { tag -> val view = tagged(tag); val xy = IntArray(2); view.getLocationOnScreen(xy)
                visible.contains(Rect(xy[0], xy[1], xy[0] + view.width, xy[1] + view.height)) }
        } }
        pending.add(approval("arrived"))
        main { authorization!!.refresh() }
        waitFor("new delegated approval") { main { views().any { it.tag == "pending:arrived" } } }
        main { val input = tagged("answer:q") as EditText; assertTrue(input.hasFocus()); assertEquals("保留我的回答草稿", input.text.toString()); assertEquals(2, input.selectionStart); assertEquals(5, input.selectionEnd) }
    }

    @Test fun truncatedApprovalNeedsCompleteMatchingDetailsBeforeApproval() {
        pending.add(approval("long").put("requires_details", true).put("arguments_truncated", true).put("fingerprint", "fp-long"))
        val failTail = AtomicBoolean(true)
        intercept = { command -> if (command.optString("action") == "approval_details") {
            if (command.has("cursor") && failTail.getAndSet(false)) error("tail offline")
            JSONObject().put("pending_id", "long").put("fingerprint", "fp-long").put("truncated", false)
                .put("text", if (command.has("cursor")) "dangerous-target.txt" else "{\"command\":\"")
                .put("cursor", if (command.has("cursor")) JSONObject.NULL else "tail").put("has_more", !command.has("cursor"))
        } else null }
        launch()
        main { assertFalse(tagged("once:long").isEnabled); assertFalse(tagged("always:long").isEnabled); assertTrue(tagged("deny:long").isEnabled); tagged("details:long").performClick() }
        waitFor("failed detail keeps approval disabled") { main { views().filterIsInstance<TextView>().any { it.text.contains("tail offline") } && !tagged("once:long").isEnabled && tagged("deny:long").isEnabled } }
        main { tagged("details:long").performClick() }
        waitFor("full detail allows exact approval") { main { views().any { it.tag == "approval-details:long" && (it as TextView).text.contains("dangerous-target.txt") } && tagged("once:long").isEnabled } }
        main { tagged("once:long").performClick() }
        waitFor("detail ack sent") { calls.any { it.optString("action") == "resolve" } }
        val result = calls.last { it.optString("action") == "resolve" }
        assertEquals("fp-long", result.getString("fingerprint")); assertTrue(result.getBoolean("details_ack"))
    }

    @Test fun mismatchedDetailsCannotEnableApprovalButDenialStillWorks() {
        pending.add(approval("long").put("requires_details", true).put("fingerprint", "correct"))
        intercept = { if (it.optString("action") == "approval_details") JSONObject().put("pending_id", "long").put("fingerprint", "wrong").put("text", "not the requested action").put("truncated", false).put("has_more", false).put("cursor", JSONObject.NULL) else null }
        launch()
        main { tagged("details:long").performClick() }
        waitFor("mismatch blocked") { main { views().filterIsInstance<TextView>().any { it.text.contains("详情已变化") } && !tagged("once:long").isEnabled } }
        main { tagged("deny:long").performClick() }
        waitFor("deny without details") { calls.any { it.optString("decision") == "deny" } }
        val result = calls.last { it.optString("action") == "resolve" }
        assertFalse(result.has("details_ack")); assertFalse(result.has("fingerprint"))
    }

    @Test fun revokeRetriesLostAcknowledgementButRegrantUsesNewNonce() {
        rules.add(JSONObject().put("id", "stable-rule").put("rule_preview", "/usr/bin/tee -a report.txt"))
        val responses = mutableMapOf<String, JSONObject>(); val loseAck = AtomicBoolean(true)
        intercept = { command -> if (command.optString("action") == "revoke_rule") {
            val id = command.getString("request_id")
            // AgentPanel adds routing fields to the same JSONObject after the nonce was selected.
            command.put("version", 1).put("agent_id", "originating-scope")
            val result = synchronized(this) { responses.getOrPut(id) {
                rules.removeAll { it.optString("id") == "stable-rule" }
                JSONObject().put("revoked", true).put("rule_id", "stable-rule")
            } }
            if (loseAck.getAndSet(false)) error("ACK lost after successful revoke")
            result
        } else null }
        launch()
        main { authorization!!.entry.performClick(); tagged("authorization-rules").performClick() }
        waitFor("first rule") { main { views().any { it.tag == "revoke:stable-rule" } } }
        main { tagged("revoke:stable-rule").performClick() }
        waitFor("lost revoke ack retained") { main { tagged("revoke:stable-rule").isEnabled && views().filterIsInstance<TextView>().any { it.text.contains("ACK lost") } } }
        main { tagged("revoke:stable-rule").performClick() }
        waitFor("retry confirmed empty") { main { views().filterIsInstance<TextView>().any { it.text == "没有永久授权规则" } } }
        val first = calls.filter { it.optString("action") == "revoke_rule" }
        assertEquals(2, first.size); assertEquals(first[0].getString("request_id"), first[1].getString("request_id"))
        synchronized(this) { rules.add(JSONObject().put("id", "stable-rule").put("rule_preview", "same regranted rule")) }
        main { tagged("authorization-rules").performClick() }
        waitFor("regranted rule") { main { views().any { it.tag == "revoke:stable-rule" } } }
        main { tagged("revoke:stable-rule").performClick() }
        waitFor("second revoke really removed rule") { main { views().filterIsInstance<TextView>().any { it.text == "没有永久授权规则" } } }
        val latest = calls.last { it.optString("action") == "revoke_rule" }
        assertNotEquals(first[0].getString("request_id"), latest.getString("request_id"))
        assertTrue(synchronized(this) { rules.isEmpty() })
    }

    @Test fun detachedOrClosedTerminalDoesNotDisableAgentQueriesAnswersSettingsOrStop() {
        pending.add(question()); permissions.put("permission_mode", "read_only")
        launch(sessionChat = true)
        // These are the production Activity's unavailable terminal states; the Agent device grant stays valid.
        main {
            for (field in listOf("desktopAttached", "controlled")) MainActivity::class.java.getDeclaredField(field).apply { isAccessible = true }.setBoolean(activity, false)
            MainActivity::class.java.getDeclaredField("sessionExited").apply { isAccessible = true }.setBoolean(activity, true)
            (tagged("answer:q") as EditText).setText("仍可回答")
            assertTrue(tagged("answer-submit:q").isEnabled); tagged("answer-submit:q").performClick()
        }
        waitFor("answer with unavailable PTY") { calls.any { it.optString("answer") == "仍可回答" } }
        main { authorization!!.entry.performClick(); assertTrue(tagged("mode:ask").isEnabled); tagged("mode:ask").performClick() }
        waitFor("management with unavailable PTY") { main { authorization!!.permissionMode == "ask" && tagged("mode:read_only").isEnabled } }
        main { tagged("mode:read_only").performClick() }
        waitFor("readonly mode changed") { main { authorization!!.permissionMode == "read_only" } }
        main { dialog()!!.dismiss(); views().first { it.contentDescription == "停止" && it.isEnabled }.performClick() }
        waitFor("cancel unavailable PTY") { calls.any { it.optString("action") == "cancel" } }
        main { AgentPanel::class.java.getDeclaredField("runState").apply { isAccessible = true }.set(chat, "idle")
            (views().first { it.contentDescription == "发送任务或追加消息" } as EditText).setText("读取对话历史")
            views().first { it.contentDescription == "发送" && it.isEnabled }.performClick() }
        waitFor("readonly query sent") { calls.any { it.optString("action") == "send" } }
        assertEquals("read_only", calls.last { it.optString("action") == "send" }.getString("permission_mode"))
    }

    @Test fun compactLargeTextCardsAndImeKeepResponseReachable() {
        pending.add(question()); launch()
        main {
            root.layoutParams = FrameLayout.LayoutParams(activity.dp(300), activity.dp(330))
            views().filterIsInstance<TextView>().forEach { it.textSize = 24f }
            val input = tagged("answer:q") as EditText
            input.setText("可以继续"); input.requestFocus()
            (activity.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager).showSoftInput(input, 0)
            scroll.post { scroll.fullScroll(View.FOCUS_DOWN) }
        }
        waitFor("answer reachable") { main {
            scroll.fullScroll(View.FOCUS_DOWN)
            val button = tagged("answer-submit:q"); val rect = Rect()
            button.getGlobalVisibleRect(rect) && rect.height() >= activity.dp(44)
        } }
        Thread.sleep(300)
        instrumentation.uiAutomation.takeScreenshot()?.let { bitmap ->
            context.filesDir.resolve("authorization-compact-ime.png").outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
        }
        main { tagged("answer-submit:q").performClick() }
        waitFor("answer submitted") { calls.any { it.optString("answer") == "可以继续" } }
    }
}
