package com.yxf.aterminal

import android.content.Intent
import android.graphics.Bitmap
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import uniffi.ai_terminal_mobile.Account
import uniffi.ai_terminal_mobile.AccountDevice
import uniffi.ai_terminal_mobile.RemoteSession
import uniffi.ai_terminal_mobile.RemoteTerminal
import java.io.File
import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Opt-in live workflow; the coordinator supplies files/terminal-agent-workflow-fixture.json:
 * {session, message, expected_model, timeout_seconds}. Use an attached, idle test session.
 * Starts the normal signed-in App, sends only through its Agent composer, and leaves it signed in.
 * The fixture contains no credentials. Reports/screenshots stay in the App's private files directory.
 */
class TerminalAgentWorkflowUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val reader = Executors.newSingleThreadExecutor()
    private lateinit var activity: MainActivity
    private lateinit var remote: RemoteTerminal
    private var deadline = 0L
    private var session = ""
    private val report = JSONObject().put("passed", false)
    private val screenshots = JSONArray()

    private fun <T> main(action: () -> T): T {
        var result: T? = null
        var failure: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (error: Throwable) { failure = error } }
        failure?.let { throw it }
        @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun get(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun click(description: String) = main {
        val button = views().single { it.contentDescription?.toString() == description && it.isEnabled }
        assertTrue("Could not click $description", button.performClick())
    }
    private fun waitFor(label: String, until: Long = deadline, predicate: () -> Boolean) {
        while (SystemClock.elapsedRealtime() < minOf(until, deadline)) {
            if (predicate()) return
            Thread.sleep(250)
        }
        throw AssertionError("Timed out: $label")
    }
    private fun <T> read(label: String, action: () -> T): T {
        val remaining = deadline - SystemClock.elapsedRealtime()
        assertTrue("Deadline exhausted before $label", remaining > 0)
        val pending = reader.submit(Callable { action() })
        try { return pending.get(minOf(remaining, 20000L), TimeUnit.MILLISECONDS) }
        catch (error: ExecutionException) { throw error.cause ?: error }
        catch (error: Exception) { pending.cancel(true); throw AssertionError("Read failed: $label", error) }
    }
    private fun agent(action: String, fields: JSONObject = JSONObject()) = read("Agent $action") {
        JSONObject(remote.agent(session, fields.put("version", 1).put("action", action).toString()))
    }
    private fun run(state: JSONObject) = state.optJSONObject("last_run") ?: state
    private fun identity(): List<String> {
        val account = main { get("account") as Account }
        val current = read("current mobile identity") { account.devices().single { it.current }.id }
        return main { listOf(get("serverUrl") as String, get("accountName") as String, current) }
    }
    private fun screenshot(name: String) {
        assertTrue("App lost foreground", main { activity.hasWindowFocus() })
        val bitmap = instrumentation.uiAutomation.takeScreenshot() ?: throw AssertionError("Screenshot unavailable: $name")
        val file = File(context.filesDir, "terminal-agent-workflow-$name.png")
        try { file.outputStream().use { assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) } }
        finally { bitmap.recycle() }
        screenshots.put(file.name)
    }
    private fun record(id: String): String {
        val body = StringBuilder()
        var cursor: String? = null
        val seen = mutableSetOf<String>()
        repeat(32) {
            val request = JSONObject().put("record_id", id).put("part", "body")
            cursor?.let { request.put("cursor", it) }
            val page = agent("record", request)
            assertEquals("utf8", page.getString("encoding"))
            body.append(page.getString("body"))
            assertTrue("Record exceeds 256 KiB: $id", body.length <= 256 * 1024)
            cursor = page.optString("cursor").takeUnless { it.isEmpty() || it == "null" }
            if (cursor == null) return body.toString()
            assertTrue("Repeated record cursor", seen.add(cursor!!))
        }
        throw AssertionError("Record exceeds 32 pages: $id")
    }
    private fun history(root: String): JSONArray {
        val items = JSONArray()
        report.put("history", items)
        var cursor: String? = null
        val seen = mutableSetOf<String>()
        repeat(24) {
            val request = JSONObject()
            cursor?.let { request.put("cursor", it) }
            val page = agent("history", request)
            val rows = page.getJSONArray("items")
            for (index in 0 until rows.length()) {
                val item = rows.getJSONObject(index)
                if (item.optString("root_user_message_id") == root) items.put(item)
                if (item.getString("id") == root) return items
            }
            if (!page.optBoolean("has_more")) return items
            cursor = page.getString("cursor")
            assertTrue("Repeated history cursor", seen.add(cursor!!))
        }
        throw AssertionError("Run history exceeds 24 pages")
    }
    private fun verifyCalls(items: JSONArray, message: String) {
        val calls = JSONArray()
        report.put("tool_calls", calls)
        var foundMessage = false
        var foundElapsedWait = false
        val seen = mutableSetOf<String>()
        for (index in 0 until items.length()) {
            val item = items.getJSONObject(index)
            var value = item.getJSONObject("value")
            if (value.optBoolean("partial")) value = JSONObject(record(value.getString("record_id")))
            if (item.getString("kind") == "user") foundMessage = value.optString("message") == message
            if (item.getString("kind") != "interaction") continue
            val updates = value.optJSONArray("updates") ?: continue
            var waitCall: JSONObject? = null
            for (update in 0 until updates.length()) {
                val entry = updates.getJSONObject(update)
                if (entry.optString("name") !in setOf("wait", "input_text")) continue
                val id = entry.optString("call_record_id")
                if (id.isNotEmpty() && id != "null" && seen.add(id)) {
                    val call = JSONObject(record(id))
                    val evidence = JSONObject().put("record_id", id).put("interaction_id", item.getString("id")).put("call", call)
                    calls.put(evidence)
                    if (call.optString("name") == "wait") waitCall = evidence
                }
                val resultId = entry.optString("result_record_id")
                if (entry.optString("name") == "wait" && resultId.isNotEmpty() && resultId != "null" && seen.add(resultId)) {
                    val result = JSONObject(record(resultId))
                    waitCall?.put("result_record_id", resultId)?.put("result", result)
                    if (result.length() == 1 && result.opt("elapsed_ms") is Number && result.getLong("elapsed_ms") > 0) foundElapsedWait = true
                }
            }
        }
        assertTrue("This Run did not contain the message submitted from the UI", foundMessage)
        val recorded = (0 until calls.length()).map { calls.getJSONObject(it).getJSONObject("call") }
        assertTrue("No actual wait tool-call record in this Run", recorded.any { it.optString("name") == "wait" })
        assertTrue("No wait result containing only a positive elapsed_ms in this Run", foundElapsedWait)
        // Match an executable at a shell command boundary, not 'codex' mentioned in a prompt/echo.
        val executable = Regex("(?:^|[\\n;&|])\\s*(?:(?:command|exec)\\s+)?(?:/[^\\s\\\"';&|]+/)?codex(?=\\s|$)")
        assertTrue("No input_text starting Codex in this Run", recorded.any {
            it.optString("name") == "input_text" && executable.containsMatchIn(it.getJSONObject("arguments").optString("text"))
        })
    }

    @Test fun terminalAgentWorkflowUsesConfiguredModelAndRealTools() {
        val file = File(context.filesDir, "terminal-agent-workflow-fixture.json")
        assumeTrue("Explicit private workflow fixture required", file.isFile)
        val started = SystemClock.elapsedRealtime()
        var originalIdentity: List<String>? = null
        var submitted = false
        var root = ""
        var failure: Throwable? = null
        try {
            assertTrue("Fixture exceeds 64 KiB", file.length() <= 65536)
            val fixture = JSONObject(file.readText())
            session = fixture.getString("session")
            val message = fixture.getString("message").trim()
            val expectedModel = fixture.getString("expected_model")
            val seconds = fixture.optLong("timeout_seconds", 1800)
            assertTrue("Invalid fixture fields", session.isNotBlank() && message.isNotBlank() && expectedModel.isNotBlank() && seconds in 1L..3600L)
            deadline = started + seconds * 1000
            report.put("session", session).put("expected_model", expectedModel).put("timeout_seconds", seconds).put("diagnostic_grace_seconds", 30)
            context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK))
            waitFor("normal MainActivity", minOf(deadline, started + 60000)) {
                main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>()
                    .firstOrNull { it.hasWindowFocus() }?.also { activity = it } != null }
            }
            waitFor("saved account restoration") { main { get("loginBusy") == false && get("entryPending") == false && get("connecting") == false } }
            assertTrue("A normal saved login is required; this test never logs in", main { (get("accountName") as String).isNotBlank() })
            originalIdentity = identity()
            if (!main { get("connected") as Boolean }) {
                val desktop = main {
                    @Suppress("UNCHECKED_CAST") val devices = get("devices") as List<AccountDevice>
                    devices.filter { it.online && !it.current && it.platform == "desktop" }.singleOrNull()
                } ?: throw AssertionError("Connect the intended Desktop first, or expose exactly one online Desktop")
                click("账号与设备"); click("连接 ${desktop.name}")
            }
            waitFor("Desktop connection") { main { get("connected") == true && get("entryPending") == false && get("connecting") == false } }
            waitFor("fixture Session in connected Desktop") { main {
                @Suppress("UNCHECKED_CAST") val sessions = get("sessions") as List<RemoteSession>
                sessions.any { it.id == session && !it.exited && it.desktopAttached }
            } }
            if (main { get("selected") != session }) {
                click("打开工作空间")
                waitFor("fixture Session row") { main { views().any { it.tag == session && it.isClickable } } }
                main { assertTrue(views().first { it.tag == session && it.isClickable }.performClick()) }
            }
            waitFor("fixture Session selected") { main { get("selected") == session && get("entryPending") == false && get("desktopAttached") == true } }
            remote = main { get("remote") as RemoteTerminal }
            val configuration = read("configuration") { JSONObject(remote.configuration("{\"action\":\"show\"}")) }
            val config = configuration.getJSONObject("config")
            val bindings = config.getJSONObject("bindings")
            val binding = bindings.optJSONObject("session/$session") ?: bindings.getJSONObject("session-default")
            val profile = config.getJSONObject("models").getJSONObject(binding.getString("model_id"))
            assertEquals("Bound provider model ID", expectedModel, profile.getString("model"))
            assertFalse("Model profile must allow terminal operations", profile.optBoolean("read_only"))
            report.put("configuration_revision", configuration.getLong("revision")).put("model_profile_id", binding.getString("model_id")).put("actual_model", profile.getString("model"))
            val before = agent("state")
            report.put("state_before", before)
            assertFalse("Do not append to an existing active Run", before.optString("state") in setOf("running", "monitoring", "stopping", "finishing"))
            val previousRun = run(before).optString("run_id")
            click("AI 对话")
            waitFor("production Agent composer") { main { get("agentPanel") != null && views().any { it.contentDescription == "发送任务或追加消息" } } }
            main {
                val composer = views().single { it.contentDescription == "发送任务或追加消息" } as EditText
                assertTrue("Use a Session without an existing draft", composer.text.isBlank())
                assertFalse("Do not send existing image drafts", views().any { it.contentDescription == "移除图片" })
                composer.setText(message)
            }
            waitFor("enabled Send button") { main { views().any { it.contentDescription == "发送" && it.isEnabled } } }
            screenshot("prepared")
            submitted = true
            click("发送")
            waitFor("a new Run accepted from the UI") {
                val state = agent("state")
                report.put("state", state)
                val accepted = run(state)
                val id = accepted.optString("run_id")
                if (id.isNotEmpty() && id != "null" && id != previousRun) {
                    root = accepted.getString("root_user_message_id")
                    report.put("run_id", id).put("root_user_message_id", root)
                    true
                } else false
            }
            screenshot("submitted")
            waitFor("Run completion") {
                val state = agent("state")
                report.put("state", state)
                assertEquals("Another Run replaced this workflow", report.getString("run_id"), run(state).getString("run_id"))
                val status = state.getString("state")
                if (status in setOf("paused", "cancelled", "orphaned", "failed")) throw AssertionError("Run $status: ${state.optString("error")}")
                status == "completed"
            }
            waitFor("completed Agent UI") { main {
                val panel = get("agentPanel") as AgentPanel
                AgentPanel::class.java.getDeclaredField("runState").apply { isAccessible = true }.get(panel) == "completed" &&
                    AgentPanel::class.java.getDeclaredField("loading").apply { isAccessible = true }.get(panel) == false
            } }
            screenshot("completed")
            verifyCalls(history(root), message)
        } catch (error: Throwable) { failure = error; report.put("error", error.toString()) }
        finally {
            // A timeout does not cancel the Desktop Run or send any terminal input.
            deadline = maxOf(deadline, started) + 30000
            if (submitted && ::remote.isInitialized && failure != null) {
                try {
                    report.put("state", agent("state"))
                    if (root.isNotEmpty()) history(root)
                } catch (error: Throwable) { report.put("diagnostic_error", error.toString()) }
            }
            if (originalIdentity != null) try {
                assertEquals("Normal account/device identity changed", originalIdentity, identity())
                report.put("account_identity_preserved", true)
            } catch (error: Throwable) {
                report.put("identity_error", error.toString())
                if (failure == null) failure = error
            }
            if (failure != null && ::activity.isInitialized) try { screenshot("failed") }
                catch (error: Throwable) { report.put("screenshot_error", error.toString()) }
            reader.shutdownNow()
            failure?.let { report.put("error", it.toString()) }
            report.put("passed", failure == null).put("elapsed_ms", SystemClock.elapsedRealtime() - started).put("screenshots", screenshots)
            File(context.filesDir, "terminal-agent-workflow-results.json").writeText(report.toString(2))
        }
        failure?.let { throw it }
    }
}
