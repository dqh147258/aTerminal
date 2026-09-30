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
 * {session, message, expected_model, timeout_seconds, previous_root_user_message_id?}.
 * Use an attached, idle test session; previous_root links a documented paused workflow recovery.
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
    private fun record(id: String, terminalInteraction: String? = null): String {
        val body = StringBuilder()
        var cursor: String? = null
        val seen = mutableSetOf<String>()
        repeat(32) {
            val request = JSONObject().put("record_id", id).put("part", "body")
            cursor?.let { request.put("cursor", it) }
            val page = agent("record", request)
            assertEquals("utf8", page.getString("encoding"))
            if (terminalInteraction != null) {
                assertEquals("text", page.getString("kind"))
                assertEquals(session, page.getJSONObject("metadata").getString("session_id"))
                assertEquals(terminalInteraction, page.getJSONObject("metadata").getString("history_unit_id"))
            }
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

    private data class TerminalEvidence(val recordId: String, val sequence: Long, val body: String)
    // Split simple shell commands outside quoted arguments, comments and substitutions.
    // Here-documents are deliberately unsupported: their body is not executable shell input.
    private fun shellCommands(text: String): List<String> {
        val commands = mutableListOf<String>()
        val command = StringBuilder()
        var quote: Char? = null
        var escaped = false
        var comment = false
        var depth = 0
        for (index in text.indices) {
            val char = text[index]
            if (comment) { if (char != '\n') continue; comment = false }
            if (escaped) { command.append(char); escaped = false; continue }
            if (char == '\\' && quote != '\'') { command.append(char); escaped = true; continue }
            if (quote != null) { command.append(char); if (char == quote) quote = null; continue }
            if (char in "\"'`") { command.append(char); quote = char; continue }
            if (char == '#' && (command.isEmpty() || command.last().isWhitespace())) { comment = true; continue }
            if (char == '<' && text.getOrNull(index + 1) == '<') return emptyList()
            if (char == '(') depth++
            if (char == ')') depth--
            if (depth == 0 && char in "\n;&|") { commands.add(command.toString()); command.clear() }
            else command.append(char)
        }
        if (quote != null || escaped || depth != 0) return emptyList()
        commands.add(command.toString())
        return commands.filter { it.isNotBlank() }
    }
    private val codexExec = Regex("^\\s*(?:(?:command|exec)\\s+)?(?:/[^\\s\\\"';&|]+/)?codex\\s+exec(?=\\s|$)")
    private val exitPrint = Regex("""^\s*printf\s+['"]([A-Z][A-Z0-9_]*_EXIT)=%[sd]\\n['"]\s+["']?\$\?["']?\s*$""")
    private val codexBanner = Regex("""(?m)^OpenAI Codex \(v[0-9][^\r\n)]*\)\s*$""")
    private fun accepted(evidence: JSONObject): Boolean = evidence.optJSONObject("result")?.let {
        it.optBoolean("accepted") && !it.has("error") && it.opt("executed") != false
    } == true
    private fun verifiedLaunch(calls: List<JSONObject>, observations: List<TerminalEvidence>): JSONObject? {
        val ordered = calls.sortedWith(compareBy({ it.getLong("sequence") }, { it.getInt("position") }))
        for ((index, evidence) in ordered.withIndex()) {
            val call = evidence.getJSONObject("call")
            if (call.optString("name") != "input_text" || !accepted(evidence)) continue
            val args = call.getJSONObject("arguments")
            val commands = shellCommands(args.optString("text"))
            val launches = commands.indices.filter { codexExec.containsMatchIn(commands[it]) }
            if (launches.isEmpty()) continue
            val submission = if (args.optBoolean("submit")) evidence else {
                // A draft qualifies only when the next terminal write is an accepted, plain Enter.
                val next = ordered.drop(index + 1).firstOrNull {
                    it.getJSONObject("call").optString("name") in setOf("input_text", "send_keys")
                } ?: continue
                val nextCall = next.getJSONObject("call")
                val keys = nextCall.getJSONObject("arguments")
                if (nextCall.optString("name") != "send_keys" || !accepted(next) ||
                    keys.optString("key").lowercase() != "enter" || (keys.optJSONArray("modifiers")?.length() ?: 0) != 0 ||
                    keys.optInt("repeat", 1) != 1) continue
                next
            }
            val markers = launches.mapNotNull { commands.getOrNull(it + 1)?.let { command -> exitPrint.matchEntire(command)?.groupValues?.get(1) } }
            val observed = observations.firstOrNull { observation ->
                observation.sequence >= submission.getLong("sequence") &&
                    (codexBanner.containsMatchIn(observation.body) || observation.body.lineSequence().any { line -> markers.any { line.trimEnd('\r') == "$it=0" } })
            } ?: continue
            return JSONObject().put("call_record_id", evidence.getString("record_id"))
                .put("submission_record_id", submission.getString("record_id")).put("terminal_record_id", observed.recordId)
        }
        return null
    }

    private fun verifyCalls(items: JSONArray, message: String) {
        val calls = JSONArray()
        report.put("tool_calls", calls)
        var foundMessage = false
        var foundElapsedWait = false
        val seen = mutableSetOf<String>()
        val observations = mutableListOf<TerminalEvidence>()
        for (index in 0 until items.length()) {
            val item = items.getJSONObject(index)
            var value = item.getJSONObject("value")
            if (value.optBoolean("partial")) value = JSONObject(record(value.getString("record_id")))
            if (item.getString("kind") == "user") foundMessage = foundMessage || value.optString("message") == message
            if (item.getString("kind") != "interaction") continue
            // The complete record index preserves call/result order; updates retain only a bounded tail.
            val records = value.getJSONArray("records")
            var precedingCall: JSONObject? = null
            for (position in 0 until records.length()) {
                val entry = records.getJSONObject(position)
                val id = entry.getString("record_id")
                if (!seen.add(id)) continue
                when (entry.optString("source")) {
                    "tool_call" -> {
                        val call = JSONObject(record(id))
                        precedingCall = JSONObject().put("record_id", id).put("interaction_id", item.getString("id")).put("call", call)
                            .put("sequence", item.getLong("sequence")).put("position", position)
                        if (call.optString("name") in setOf("wait", "input_text", "send_keys")) calls.put(precedingCall)
                    }
                    "tool_result" -> {
                        val call = precedingCall ?: continue
                        val name = call.getJSONObject("call").optString("name")
                        if (name !in setOf("wait", "input_text", "send_keys")) continue
                        val result = JSONObject(record(id))
                        call.put("result_record_id", id).put("result", result)
                        if (name == "wait" && result.length() == 1 && result.opt("elapsed_ms") is Number && result.getLong("elapsed_ms") > 0) foundElapsedWait = true
                    }
                    else -> if (entry.optString("kind") == "text" && precedingCall?.getJSONObject("call")?.optString("name") == "read_terminal") {
                        observations.add(TerminalEvidence(id, item.getLong("sequence"), record(id, item.getString("id"))))
                    }
                }
            }
        }
        assertTrue("This Run did not contain the message submitted from the UI", foundMessage)
        val recorded = (0 until calls.length()).map { calls.getJSONObject(it) }
        assertTrue("No actual wait tool-call record in this Run", recorded.any { it.getJSONObject("call").optString("name") == "wait" })
        assertTrue("No wait result containing only a positive elapsed_ms in this Run", foundElapsedWait)
        val launch = verifiedLaunch(recorded, observations)
        assertNotNull("No accepted, submitted codex exec followed by archived Codex launch/completion output", launch)
        report.put("verified_codex_launch", launch)
    }

    private fun fixtureCall(name: String, args: JSONObject, result: JSONObject? = JSONObject().put("accepted", true), sequence: Long = 1): JSONObject =
        JSONObject().put("record_id", "call-$sequence").put("sequence", sequence).put("position", 0)
            .put("call", JSONObject().put("name", name).put("arguments", args)).also { evidence -> result?.let { evidence.put("result", it) } }

    @Test fun codexWorkflowEvidenceRejectsProbesDraftsAndRejectedInput() {
        val output = listOf(TerminalEvidence("terminal", 2, "OpenAI Codex (v0.159.2)\n"))
        fun input(text: String, submit: Boolean = true, result: JSONObject? = JSONObject().put("accepted", true)) =
            fixtureCall("input_text", JSONObject().put("text", text).put("submit", submit), result)
        for (probe in listOf("codex --version", "command -v codex; codex --version", "echo 'sample; codex exec task'",
            "echo sample # codex exec task", "cat <<EOF\ncodex exec task\nEOF")) {
            assertNull("A probe or quoted prompt must not prove launch: $probe", verifiedLaunch(listOf(input(probe)), output))
        }
        assertNull("Typed input has not started a task", verifiedLaunch(listOf(input("codex exec task", false)), output))
        assertNull("A request without a result is not execution evidence", verifiedLaunch(listOf(input("codex exec task", result = null)), output))
        assertNull("Rejected input must not qualify", verifiedLaunch(listOf(input("codex exec task", result = JSONObject().put("error", "lease_lost").put("executed", false))), output))
        assertNull("An explicitly unexecuted result must not qualify", verifiedLaunch(listOf(input("codex exec task", result = JSONObject().put("accepted", true).put("executed", false))), output))
        assertNull("Accepted input alone does not prove launch", verifiedLaunch(listOf(input("codex exec task")), emptyList()))
        assertNull("Assistant summaries do not prove launch", verifiedLaunch(listOf(input("codex exec task")), listOf(TerminalEvidence("terminal", 2, "Codex completed the requested task."))))
        assertNull("Earlier terminal output does not prove this launch", verifiedLaunch(listOf(input("codex exec task")), listOf(output.single().copy(sequence = 0))))
    }

    @Test fun codexWorkflowEvidenceAcceptsSubmittedExecWithArchivedOutput() {
        val banner = listOf(TerminalEvidence("terminal", 2, "OpenAI Codex (v0.159.2)\n"))
        for (text in listOf("codex exec 'draw a pelican'", "cd /fixture && command /usr/local/bin/codex exec 'draw a pelican'")) {
            val submitted = fixtureCall("input_text", JSONObject().put("text", text).put("submit", true))
            assertNotNull(verifiedLaunch(listOf(submitted), banner))
        }
        val completion = fixtureCall("input_text", JSONObject().put("text", "codex exec task; printf 'CODEX_FIXTURE_EXIT=%s\\n' \"\$?\"").put("submit", true))
        assertNotNull("The command's actual successful exit marker proves execution", verifiedLaunch(listOf(completion), listOf(TerminalEvidence("terminal", 2, "CODEX_FIXTURE_EXIT=0\n"))))
        assertNull("An unrelated marker must not qualify", verifiedLaunch(listOf(completion), listOf(TerminalEvidence("terminal", 2, "CODEX_OTHER_EXIT=0\n"))))
        assertNull("A failed task must not qualify through completion alone", verifiedLaunch(listOf(completion), listOf(TerminalEvidence("terminal", 2, "CODEX_FIXTURE_EXIT=1\n"))))
        val draft = fixtureCall("input_text", JSONObject().put("text", "codex exec task"))
        val enter = fixtureCall("send_keys", JSONObject().put("key", "enter"), sequence = 2)
        val afterEnter = listOf(banner.single().copy(sequence = 3))
        assertNotNull("An accepted separate Enter submits the accepted draft", verifiedLaunch(listOf(draft, enter), afterEnter))
        val rejectedEnter = fixtureCall("send_keys", JSONObject().put("key", "enter"), JSONObject().put("error", "lease_lost").put("executed", false), 2)
        assertNull(verifiedLaunch(listOf(draft, rejectedEnter), afterEnter))
        val changedDraft = fixtureCall("input_text", JSONObject().put("text", " --version"), sequence = 2)
        assertNull("Enter must submit the same unchanged draft", verifiedLaunch(listOf(draft, changedDraft, enter), afterEnter))
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
            val previousRoot = fixture.optString("previous_root_user_message_id").takeUnless { it.isBlank() || it == "null" }
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
            val currentHistory = history(root)
            if (previousRoot != null) {
                assertNotEquals("Recovery must reference an earlier user message", root, previousRoot)
                val previousHistory = history(previousRoot)
                assertTrue("Previous workflow must exist in this Session", (0 until previousHistory.length()).any {
                    previousHistory.getJSONObject(it).getString("id") == previousRoot
                })
                report.put("previous_root_user_message_id", previousRoot).put("previous_history", previousHistory)
                for (index in 0 until previousHistory.length()) currentHistory.put(previousHistory.getJSONObject(index))
                report.put("history", currentHistory)
            }
            verifyCalls(currentHistory, message)
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
