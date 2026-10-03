package com.yxf.aterminal

import android.content.Intent
import android.graphics.Bitmap
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.*
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.ai_terminal_mobile.Account
import uniffi.ai_terminal_mobile.RemoteTerminal
import java.io.File
import java.util.UUID

/** Actual native controls + encrypted RPC + deterministic model + disposable shell/marker files.
 * The runner independently checks every expected marker; the model never writes them itself.
 */
class AgentAuthorizationRpcUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private lateinit var remote: RemoteTerminal
    private lateinit var session: String
    private var agentId: String? = null
    private var chat: AgentPanel? = null
    private lateinit var root: LinearLayout
    private var authorization: AgentAuthorizationPanel? = null
    private val sent = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()
    private val report = JSONObject().put("real_encrypted_rpc", true).put("model_source", "local deterministic fixture")
    private fun <T> main(action: () -> T): T { var result: T? = null; var error: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (e: Throwable) { error = e } }
        error?.let { throw it }; @Suppress("UNCHECKED_CAST") return result as T }
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun dialog() = authorization?.let { AgentAuthorizationPanel::class.java.getDeclaredField("dialog").apply { isAccessible = true }.get(it) as? android.app.AlertDialog }
    private fun views() = all(root) + listOfNotNull(dialog()?.window?.decorView).flatMap { all(it) }
    private fun tagged(tag: String) = views().first { it.tag == tag }
    private fun request(action: String, fields: JSONObject = JSONObject()): JSONObject {
        agentId?.let { fields.put("agent_id", it) }
        return JSONObject(remote.agent(session, fields.put("version", 1).put("action", action).toString()))
    }
    private fun waitFor(name: String, condition: () -> Boolean) {
        val until = SystemClock.elapsedRealtime() + 35000
        while (SystemClock.elapsedRealtime() < until) { if (condition()) return; Thread.sleep(60) }
        report.put("timeout", name).put("last_state", runCatching { request("state") }.getOrNull())
        screenshot("authorization-timeout")
        fail("Timed out: $name")
    }
    private fun screenshot(name: String) {
        instrumentation.waitForIdleSync()
        instrumentation.uiAutomation.takeScreenshot()?.let { bitmap ->
            context.filesDir.resolve("agent-ui-$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
        }
    }
    private fun task(id: String, command: String? = null, steps: JSONArray? = null, waitCommand: Boolean = true) {
        val tools = steps ?: JSONArray().put(JSONObject().put("tool", "run_command").put("arguments", JSONObject().put("command", command)))
        if (command != null && waitCommand) tools.put(JSONObject().put("tool", "wait_command").put("arguments", JSONObject()
            .put("command_id", JSONObject().put("\$fixture_ref", JSONObject().put("step", 0).put("pointer", "/command_id")))
            .put("timeout_ms", 30000)))
        val message = "AUTH_REVIEW:" + JSONObject().put("id", id).put("steps", tools)
        waitFor("composer ready $id") { main { authorization?.canSend == true && views().any { it.contentDescription == "发送" } &&
            views().filterIsInstance<EditText>().any { it.contentDescription == "发送任务或追加消息" && it.isEnabled } } }
        main {
            (views().first { it.contentDescription == "发送任务或追加消息" } as EditText).setText(message)
            val send = views().first { it.contentDescription == "发送" }
            assertTrue("Draft should enable send", send.isEnabled); send.performClick()
        }
        waitFor("task acknowledged $id") { main { (views().first { it.contentDescription == "发送任务或追加消息" } as EditText).text.isEmpty() } }
    }
    private fun nativeTask(id: String, file: String = "auth-review-always.log", input: String = "always\n") = task(id, steps = JSONArray().put(JSONObject()
        .put("tool", "run_program").put("arguments", JSONObject().put("program", "/usr/bin/tee").put("args", JSONArray(listOf("-a", file))).put("stdin", input))))
    private fun revoke(ruleId: String) {
        permissionsDialog()
        main { tagged("authorization-rules").performClick() }
        waitFor("rule rendered") { main { views().any { it.tag == "revoke:$ruleId" && it.isEnabled } } }
        screenshot("authorization-rules"); main { tagged("revoke:$ruleId").performClick() }
        waitFor("rule revoked") { request("rules").getJSONArray("items").length() == 0 }
        waitFor("UI rule removal") { main { views().filterIsInstance<TextView>().any { it.text == "没有永久授权规则" } } }
        main { dialog()!!.dismiss() }
    }

    private fun activePending(kind: String = "approval"): JSONObject? {
        val page = request("pending").getJSONArray("items")
        return (0 until page.length()).map { page.getJSONObject(it) }.firstOrNull { it.optString("kind") == kind && it.optString("state") == "pending" }
    }
    private fun pending(kind: String = "approval"): JSONObject {
        var value: JSONObject? = null
        waitFor("pending $kind") { activePending(kind)?.also { value = it } != null }
        val item = value!!
        waitFor("render pending ${item.getString("id")}") { main { views().any { it.tag == "pending:${item.getString("id")}" } } }
        return item
    }
    private fun decision(item: JSONObject, decision: String) {
        val tag = "$decision:${item.getString("id")}"
        waitFor("enabled $tag") { main { views().any { it.tag == tag && it.isEnabled } } }
        main {
            val button = tagged(tag)
            button.requestRectangleOnScreen(android.graphics.Rect(0, 0, button.width, button.height), true)
            assertTrue(button.isEnabled); button.performClick()
        }
    }
    private fun completed(id: String) {
        waitFor("completion $id") {
            val state = request("state")
            assertFalse("Run failed: $state", state.optString("state") in setOf("failed", "paused", "orphaned"))
            state.optString("state") == "completed"
        }
        waitFor("UI completed $id") { main { views().filterIsInstance<TextView>().any { it.text.toString() == "AUTH_REVIEW_DONE:$id" } } }
    }
    private fun permissionsDialog() {
        main { if (dialog() == null) authorization!!.entry.performClick() }
        waitFor("Desktop permissions confirmed") { main { views().any { it.tag == "full-authorization" && it.isEnabled } } }
    }
    private fun full(enabled: Boolean) {
        permissionsDialog()
        main { val toggle = tagged("full-authorization") as Switch; assertNotEquals(enabled, toggle.isChecked); toggle.performClick() }
        waitFor("Desktop full=$enabled") { request("permissions").getBoolean("full_authorization") == enabled }
        waitFor("UI full=$enabled") { main { (tagged("full-authorization") as Switch).isChecked == enabled && tagged("full-authorization").isEnabled } }
        screenshot("authorization-full")
        main { dialog()!!.dismiss() }
    }

    @Test fun nativeApprovalsQuestionsRulesFullAndCwdRoundTrip() {
        val fixture = JSONObject(context.filesDir.resolve("agent-ui-fixture.json").readText())
        val account = Account(); remote = RemoteTerminal(); session = fixture.getString("session")
        val primaryAccount = context.getSharedPreferences("account", 0).all.toMap()
        val primaryConnection = context.getSharedPreferences("connection", 0).all.toMap()
        val cache = context.cacheDir.resolve("authorization-wire-${UUID.randomUUID()}.sqlite3")
        val expected = JSONObject().put("auth-review-once.log", JSONArray(listOf("once")))
            .put("auth-review-always.log", JSONArray(listOf("always", "always", "always")))
            .put("auth-review-long.log", JSONArray(listOf(" ".repeat(6000) + "long")))
            .put("auth-review-global-full.log", JSONArray(listOf("global-full")))
            .put("auth-review-global-full-off.log", JSONObject.NULL)
            .put("auth-review-closed-write.log", JSONObject.NULL)
            .put("auth-review-cancelled.log", JSONObject.NULL)
            .put("auth-review-denied.log", JSONObject.NULL).put("auth-review-full.log", JSONArray(listOf("full", "full")))
            .put("auth-review-full-off.log", JSONArray(listOf("after-off")))
            .put("auth-review-cwd.log", JSONArray(listOf("cwd")))
            .put("auth-review-subdir/auth-review-always.log", JSONArray(listOf("always")))
        report.put("expected_markers", expected)
        try {
            account.login(fixture.getString("server"), fixture.getString("username"), fixture.getString("password"), "Authorization UI fixture", "android", "")
            var desktop = ""
            waitFor("fixture Desktop") { desktop = account.devices().firstOrNull { it.platform == "desktop" && it.online }?.id.orEmpty(); desktop.isNotBlank() }
            account.connect(desktop, remote); remote.select(session, false)
            val initialCwd = request("context").getString("cwd")
            assertTrue(initialCwd.startsWith("/")); report.put("cwd", initialCwd)
            context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui", true))
            waitFor("activity") { main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull { it.hasWindowFocus() }?.also { activity = it } != null } }
            main {
                root = activity.column(); root.addView(activity.row().apply { addView(activity.heading("Agent")); addView(activity.label("关闭")) })
                activity.setContentView(root)
                chat = AgentPanel(activity, root, listOf(fixture.getString("server"), fixture.getString("username"), desktop), session, { true }, { target, raw ->
                    sent.add(target to raw); remote.agent(target, raw)
                }, {}, cachePath = cache.path, workingPath = { "Fixture Desktop" })
                authorization = AgentPanel::class.java.getDeclaredField("authorization").apply { isAccessible = true }.get(chat) as AgentAuthorizationPanel
            }
            waitFor("default ask") { main { authorization!!.canSend } }
            assertEquals("ask", request("permissions").getString("permission_mode")); assertFalse(request("permissions").getBoolean("full_authorization"))

            task("once", "printf 'once\\n' >> auth-review-once.log")
            val once = pending(); assertEquals(session, once.getString("session_id")); assertEquals(initialCwd, once.getString("cwd"))
            assertTrue(once.get("arguments_preview").toString().contains("auth-review-once.log"))
            screenshot("authorization-once"); decision(once, "once"); completed("once")
            val original = sent.last { JSONObject(it.second).optString("action") == "resolve" }
            val duplicate = JSONObject(remote.agent(original.first, original.second)); assertTrue(duplicate.getBoolean("duplicate"))
            report.put("once_and_idempotent_replay", true)

            nativeTask("long-details", "auth-review-long.log", " ".repeat(6000) + "long\n")
            val long = pending(); val longId = long.getString("id")
            assertTrue(long.getBoolean("requires_details"))
            main { assertFalse(tagged("once:$longId").isEnabled); assertFalse(tagged("always:$longId").isEnabled); assertTrue(tagged("deny:$longId").isEnabled); tagged("details:$longId").performClick() }
            waitFor("complete approval detail rendered") { main { views().filterIsInstance<TextView>().any { it.tag == "approval-details:$longId" && it.text.contains("auth-review-long.log") } && tagged("once:$longId").isEnabled } }
            decision(long, "once"); completed("long-details"); report.put("long_details_before_approval", true)

            nativeTask("always-first"); val always = pending(); assertTrue("Fixture should have stable cwd/version: $always", always.getBoolean("can_always"))
            decision(always, "always"); completed("always-first")
            nativeTask("always-second"); completed("always-second"); assertNull(activePending())
            report.put("exact_rule_across_runs", true)

            task("denied", "printf 'denied\\n' >> auth-review-denied.log", waitCommand = false)
            val denied = pending(); screenshot("authorization-deny"); decision(denied, "deny"); completed("denied")
            report.put("denied_without_side_effect", true)

            val questionSteps = JSONArray().put(JSONObject().put("tool", "ask_user").put("arguments", JSONObject().put("question", "输出格式？").put("options", JSONArray(listOf("JSON", "文字")))))
            task("question-option", steps = questionSteps)
            val option = pending("question"); val optionId = option.getString("id")
            main { tagged("option:$optionId:0").performClick(); assertEquals("JSON", (tagged("answer:$optionId") as EditText).text.toString()); tagged("answer-submit:$optionId").performClick() }
            completed("question-option")
            task("question-text", steps = JSONArray().put(JSONObject().put("tool", "ask_user").put("arguments", JSONObject().put("question", "补充说明？"))))
            val question = pending("question"); val questionId = question.getString("id")
            main { (tagged("answer:$questionId") as EditText).setText("保留完整说明"); tagged("answer-submit:$questionId").performClick() }
            screenshot("authorization-question"); completed("question-text"); report.put("options_and_free_text", true)

            full(true)
            task("full-first", "printf 'full\\n' >> auth-review-full.log"); completed("full-first"); assertNull(activePending())
            task("full-second", "printf 'full\\n' >> auth-review-full.log"); completed("full-second"); assertNull(activePending())
            full(false)
            task("full-off", "printf 'after-off\\n' >> auth-review-full-off.log"); decision(pending(), "once"); completed("full-off")
            report.put("full_persists_across_runs_until_closed", true)

            val terminalSession = session
            val scope = JSONObject(remote.agent("", JSONObject().put("version", 1).put("action", "global_create").put("request_id", "authorization-global-scope").toString())).getJSONObject("scope")
            session = ""; agentId = scope.getString("agent")
            main {
                chat?.close()
                root.removeAllViews(); root.addView(activity.row().apply { addView(activity.heading("Global Agent")); addView(activity.label("关闭")) })
                chat = AgentPanel(activity, root, listOf(fixture.getString("server"), fixture.getString("username"), desktop), "", { true }, { target, raw -> sent.add(target to raw); remote.agent(target, raw) }, {}, cachePath = cache.path,
                    globalConversation = JSONObject().put("scope", scope).put("title", "授权隔离对话"))
                authorization = AgentPanel::class.java.getDeclaredField("authorization").apply { isAccessible = true }.get(chat) as AgentAuthorizationPanel
            }
            waitFor("global mode confirmed") { main { authorization!!.canSend } }
            assertFalse(request("permissions").getBoolean("full_authorization"))
            full(true)
            fun globalNative(file: String, input: String) = JSONArray().put(JSONObject().put("tool", "run_program").put("arguments", JSONObject()
                .put("session_id", terminalSession).put("program", "/usr/bin/tee").put("args", JSONArray(listOf("-a", file))).put("stdin", input)))
            task("global-full", steps = globalNative("auth-review-global-full.log", "global-full\n")); completed("global-full"); assertNull(activePending())
            full(false)
            task("global-full-off", steps = globalNative("auth-review-global-full-off.log", "should-not-run\n")); decision(pending(), "deny"); completed("global-full-off")
            assertFalse(request("permissions").getBoolean("full_authorization"))
            session = terminalSession; agentId = null
            main {
                chat?.close()
                root.removeAllViews(); root.addView(activity.row().apply { addView(activity.heading("Session Agent")); addView(activity.label("关闭")) })
                chat = AgentPanel(activity, root, listOf(fixture.getString("server"), fixture.getString("username"), desktop), session, { true }, { target, raw -> sent.add(target to raw); remote.agent(target, raw) }, {}, cachePath = cache.path)
                authorization = AgentPanel::class.java.getDeclaredField("authorization").apply { isAccessible = true }.get(chat) as AgentAuthorizationPanel
            }
            waitFor("session mode restored") { main { authorization!!.canSend } }
            assertFalse(request("permissions").getBoolean("full_authorization"))
            report.put("global_full_closed_and_scope_isolated", true)

            task("cwd-create", "/bin/mkdir auth-review-subdir"); decision(pending(), "once"); completed("cwd-create")
            task("cwd-change", "cd auth-review-subdir"); decision(pending(), "once"); completed("cwd-change")
            waitFor("observed cwd changed") { request("context").optString("cwd") == "$initialCwd/auth-review-subdir" }
            nativeTask("always-different-cwd")
            val changedCwd = pending(); assertEquals("$initialCwd/auth-review-subdir", changedCwd.getString("cwd")); assertNotEquals(always.optString("fingerprint"), changedCwd.optString("fingerprint"))
            decision(changedCwd, "once"); completed("always-different-cwd")
            task("cwd-back", "cd .."); decision(pending(), "once"); completed("cwd-back")
            waitFor("cwd restored") { request("context").optString("cwd") == initialCwd }
            task("cwd-proof", "printf 'cwd\\n' >> auth-review-cwd.log"); decision(pending(), "once"); completed("cwd-proof")
            report.put("cwd_reassessed_exact_rule", true)

            val rulePage = request("rules").getJSONArray("items")
            val ruleId = (0 until rulePage.length()).map { rulePage.getJSONObject(it) }.first { it.opt("arguments_preview")?.toString().orEmpty().contains("auth-review-always.log") }.getString("id")
            revoke(ruleId)
            nativeTask("rule-regrant"); decision(pending(), "always"); completed("rule-regrant")
            val regranted = request("rules").getJSONArray("items")
            assertEquals(1, regranted.length()); assertEquals(ruleId, regranted.getJSONObject(0).getString("id"))
            revoke(ruleId)
            val revokes = sent.filter { JSONObject(it.second).optString("action") == "revoke_rule" }.map { JSONObject(it.second).getString("request_id") }
            assertEquals(2, revokes.size); assertNotEquals(revokes[0], revokes[1])
            nativeTask("rule-revoked"); decision(pending(), "deny"); completed("rule-revoked")
            assertEquals(0, request("rules").getJSONArray("items").length())
            report.put("regrant_second_revoke_new_nonce", true).put("revocation_requires_new_approval", true)

            task("cancel-wait", "printf 'cancelled\\n' >> auth-review-cancelled.log", waitCommand = false)
            pending()
            waitFor("waiting can stop") { main { views().any { it.contentDescription == "停止" && it.isEnabled } } }
            main { views().first { it.contentDescription == "停止" && it.isEnabled }.performClick() }
            waitFor("waiting run cancelled") { request("state").optString("state") == "cancelled" }
            report.put("waiting_run_can_stop", true)

            permissionsDialog()
            main { tagged("mode:read_only").performClick() }
            waitFor("Desktop read only") { request("permissions").optString("permission_mode") == "read_only" }
            waitFor("full disabled for readonly") { main { !tagged("full-authorization").isEnabled && tagged("mode:ask").isEnabled } }
            screenshot("authorization-readonly")
            main { tagged("mode:ask").performClick() }
            waitFor("Desktop restored ask") { request("permissions").optString("permission_mode") == "ask" }
            main { dialog()!!.dismiss() }
            report.put("read_only_uses_desktop_mode", true)

            // Close only the disposable fixture PTY. Scope history, device grant and management remain usable.
            remote.closeSelected()
            waitFor("fixture terminal closed") { remote.sessions().none { it.id == session } }
            permissionsDialog()
            main { tagged("mode:read_only").performClick() }
            waitFor("closed scope readonly setting") { request("permissions").optString("permission_mode") == "read_only" }
            main { dialog()!!.dismiss() }
            task("closed-query", steps = JSONArray().put(JSONObject().put("tool", "get_capabilities").put("arguments", JSONObject())))
            completed("closed-query")
            task("closed-question", steps = JSONArray().put(JSONObject().put("tool", "ask_user").put("arguments", JSONObject().put("question", "终端已关闭，仍需补充说明？"))))
            val closedQuestion = pending("question").getString("id")
            main { (tagged("answer:$closedQuestion") as EditText).setText("可以回答"); tagged("answer-submit:$closedQuestion").performClick() }
            completed("closed-question")
            main { authorization!!.entry.performClick() }
            waitFor("closed scope can change setting") { main { tagged("mode:ask").isEnabled } }
            main { tagged("mode:ask").performClick() }
            waitFor("closed scope ask restored") { request("permissions").optString("permission_mode") == "ask" }
            main { dialog()!!.dismiss() }
            full(true)
            task("closed-write", "printf 'unexpected\\n' >> auth-review-closed-write.log", waitCommand = false)
            completed("closed-write")
            full(false)
            assertFalse(request("permissions").getBoolean("full_authorization"))
            report.put("closed_scope_queries_questions_settings", true).put("closed_write_denied_by_host", true)

            assertEquals(primaryAccount, context.getSharedPreferences("account", 0).all)
            assertEquals(primaryConnection, context.getSharedPreferences("connection", 0).all)
            assertTrue(sent.filter { JSONObject(it.second).optString("action") == "send" }.all { JSONObject(it.second).optString("permission_mode") in setOf("ask", "read_only") && !JSONObject(it.second).has("allow_input") })
            report.put("primary_preferences_unchanged", true).put("passed", true)
        } catch (error: Throwable) { report.put("passed", false).put("error", error.toString()); throw error }
        finally {
            main { chat?.close(); if (::activity.isInitialized) activity.finish() }
            runCatching { remote.disconnect() }; runCatching { account.logout() }; remote.close(); account.close()
            context.filesDir.resolve("authorization-ui-results.json").writeText(report.toString(2))
            listOf("", "-wal", "-shm").forEach { File(cache.path + it).delete() }
        }
    }
}
