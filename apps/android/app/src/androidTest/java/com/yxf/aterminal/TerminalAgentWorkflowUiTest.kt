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
import org.json.JSONTokener
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
 * {session, message, expected_model, timeout_seconds, previous_root_user_message_id?,
 *  codex_mode?: "exec" | "interactive"} (defaults to the original exec workflow).
 * Interactive evidence requires a submitted launch, its TUI banner, and a submitted prompt echoed
 * by a later read_terminal archive. Reports include those archives' bodies for coordinator review;
 * launch/prompt evidence alone does not verify the generated animation or its completion.
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
        val epoch = main { get("accountEpoch") as Int }
        val current = read("current mobile identity") {
            try { account.devices().single { it.current }.id }
            finally {
                MainActivity::class.java.getDeclaredMethod("persist", Int::class.javaPrimitiveType)
                    .apply { isAccessible = true }.invoke(activity, epoch)
            }
        }
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

    private enum class CodexMode { EXEC, INTERACTIVE }
    private data class TerminalEvidence(val recordId: String, val sequence: Long, val body: String, val position: Int = Int.MAX_VALUE)
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
    private val codexTuiBanner = Regex("""(?m)^[ \t]*(?:│[ \t]*)?>_ OpenAI Codex \(v[0-9][^\r\n)]*\)[ \t]*(?:│)?[ \t]*\r?$""")
    private fun accepted(evidence: JSONObject): Boolean = evidence.optJSONObject("result")?.let {
        it.optBoolean("accepted") && !it.has("error") && it.opt("executed") != false
    } == true
    private fun sameSession(call: JSONObject): Boolean = call.getJSONObject("arguments").optString("session_id").let { it.isBlank() || it == session }
    private fun submission(ordered: List<JSONObject>, index: Int): JSONObject? {
        val evidence = ordered[index]
        val call = evidence.getJSONObject("call")
        if (call.optString("name") != "input_text" || !sameSession(call) || !accepted(evidence)) return null
        if (call.getJSONObject("arguments").optBoolean("submit")) return evidence
        // An explicitly unexecuted write cannot change the draft. Any other write must be the
        // accepted, plain Enter that submits it; an uncertain receipt cannot be skipped.
        val next = ordered.drop(index + 1).firstOrNull {
            val candidate = it.getJSONObject("call")
            candidate.optString("name") in setOf("input_text", "send_keys") && sameSession(candidate) &&
                it.optJSONObject("result")?.opt("executed") != false
        } ?: return null
        val nextCall = next.getJSONObject("call")
        val keys = nextCall.getJSONObject("arguments")
        if (!accepted(next)) return null
        if (nextCall.optString("name") == "input_text")
            return next.takeIf { keys.has("text") && keys.getString("text").isEmpty() && keys.optBoolean("submit") }
        return next.takeIf { nextCall.optString("name") == "send_keys" &&
            keys.optString("key").lowercase() == "enter" && (keys.optJSONArray("modifiers")?.length() ?: 0) == 0 &&
            keys.optInt("repeat", 1) == 1 }
    }
    private fun after(observation: TerminalEvidence, evidence: JSONObject): Boolean =
        observation.sequence > evidence.getLong("sequence") || (observation.sequence == evidence.getLong("sequence") &&
            observation.position > evidence.optInt("result_position", evidence.getInt("position")))
    private fun after(evidence: JSONObject, observation: TerminalEvidence): Boolean =
        evidence.getLong("sequence") > observation.sequence || (evidence.getLong("sequence") == observation.sequence &&
            evidence.getInt("position") > observation.position)

    // Only literal shell words are supported. Fail closed on substitutions, redirection or shell
    // syntax rather than treating text quoted for echo, a script, or a here-document as a launch.
    private fun shellWords(command: String): List<String>? {
        val words = mutableListOf<String>()
        val word = StringBuilder()
        var quote: Char? = null
        var escaped = false
        var started = false
        for (char in command) {
            if (escaped) { word.append(char); escaped = false; started = true; continue }
            if (char == '\\' && quote != '\'') { escaped = true; started = true; continue }
            if (quote == '\'') { if (char == quote) quote = null else word.append(char); continue }
            if (char in "\u0024`" || (quote == null && char in "<>(){}")) return null
            if (quote == '"') { if (char == quote) quote = null else word.append(char); continue }
            if (char in "\"'") { quote = char; started = true; continue }
            if (char.isWhitespace()) { if (started) { words.add(word.toString()); word.clear(); started = false } }
            else { word.append(char); started = true }
        }
        if (quote != null || escaped) return null
        if (started) words.add(word.toString())
        return words
    }
    private data class CodexInvocation(val mode: CodexMode, val prompt: String?)
    private fun codexInvocation(command: String): CodexInvocation? {
        val words = shellWords(command) ?: return null
        var index = if (words.firstOrNull() in setOf("command", "exec")) 1 else 0
        val executable = words.getOrNull(index++) ?: return null
        if (executable != "codex" && !(executable.startsWith('/') && executable.substringAfterLast('/') == "codex")) return null
        val valueOptions = setOf("-c", "--config", "-m", "--model", "-p", "--profile", "-s", "--sandbox",
            "-a", "--ask-for-approval", "-C", "--cd", "--add-dir", "--enable", "--disable", "-i", "--image")
        val flags = setOf("--no-alt-screen", "--full-auto", "--search", "--oss", "--dangerously-bypass-approvals-and-sandbox")
        var promptOnly = false
        while (index < words.size) {
            val word = words[index]
            if (word == "--") { promptOnly = true; index++; break }
            if (!word.startsWith('-')) break
            if (word in valueOptions) { if (index + 1 >= words.size) return null; index += 2 }
            else if (word.substringBefore('=') in valueOptions && '=' in word) index++
            else if (word in flags) index++
            else return null // Includes --version/-V and --help/-h probes.
        }
        val arguments = words.drop(index)
        if (!promptOnly && arguments.firstOrNull() in setOf("exec", "e")) return CodexInvocation(CodexMode.EXEC, null)
        // This fixture supports a new interactive session, not the CLI's administrative subcommands.
        if (arguments.size > 1 || (!promptOnly && arguments.firstOrNull() in setOf("resume", "r", "fork", "review", "login", "logout",
                "mcp", "mcp-server", "app", "app-server", "completion", "sandbox", "debug", "apply", "a", "cloud", "features", "help"))) return null
        return CodexInvocation(CodexMode.INTERACTIVE, arguments.singleOrNull()?.takeIf { it.isNotBlank() })
    }
    private fun promptEchoed(body: String, prompt: String): Boolean {
        fun compact(text: String) = text.filterNot { it.isWhitespace() }
        val expected = compact(prompt)
        if (expected.isEmpty()) return false
        val lines = body.lines()
        for ((index, line) in lines.withIndex()) {
            val trimmed = line.trimStart()
            if (!trimmed.startsWith("› ")) continue
            val echo = StringBuilder(trimmed.removePrefix("› "))
            if (compact(echo.toString()) == expected) return true
            for (continuation in lines.drop(index + 1)) {
                if (continuation.isNotEmpty() && !continuation.first().isWhitespace()) break
                echo.append(continuation.trim())
                val actual = compact(echo.toString())
                if (actual == expected) return true
                if (!expected.startsWith(actual)) break
            }
        }
        return false
    }
    private fun verifiedInteractiveLaunch(ordered: List<JSONObject>, observations: List<TerminalEvidence>): JSONObject? {
        val submitted = ordered.indices.mapNotNull { index -> submission(ordered, index)?.let { index to it } }
        // A successful interactive launch cannot excuse an actual non-interactive launch in this Run.
        if (submitted.any { (index, _) -> shellCommands(ordered[index].getJSONObject("call").getJSONObject("arguments").optString("text"))
                .any { codexInvocation(it)?.mode == CodexMode.EXEC } }) return null
        for ((index, launchSubmission) in submitted) {
            val evidence = ordered[index]
            val commands = shellCommands(evidence.getJSONObject("call").getJSONObject("arguments").optString("text"))
            for (command in commands) {
                val invocation = codexInvocation(command)?.takeIf { it.mode == CodexMode.INTERACTIVE } ?: continue
                val banner = observations.firstOrNull { after(it, launchSubmission) && codexTuiBanner.containsMatchIn(it.body) } ?: continue
                for ((promptIndex, promptSubmission) in submitted) {
                    val promptCall = ordered[promptIndex]
                    val prompt = if (promptIndex == index) invocation.prompt?.takeUnless { it.trimStart().startsWith('/') } ?: continue else {
                        if (!after(promptCall, banner)) continue
                        promptCall.getJSONObject("call").getJSONObject("arguments").optString("text").takeIf { it.isNotBlank() && !it.trimStart().startsWith('/') } ?: continue
                    }
                    val echoed = observations.firstOrNull { after(it, promptSubmission) && promptEchoed(it.body, prompt) } ?: continue
                    return JSONObject().put("mode", "interactive").put("launch_command", command.trim())
                        .put("call_record_id", evidence.getString("record_id")).put("submission_record_id", launchSubmission.getString("record_id"))
                        .put("terminal_record_id", banner.recordId).put("prompt_call_record_id", promptCall.getString("record_id"))
                        .put("prompt_submission_record_id", promptSubmission.getString("record_id")).put("prompt_terminal_record_id", echoed.recordId)
                }
            }
        }
        return null
    }
    private fun verifiedLaunch(calls: List<JSONObject>, observations: List<TerminalEvidence>, mode: CodexMode = CodexMode.EXEC): JSONObject? {
        val ordered = calls.sortedWith(compareBy({ it.getLong("sequence") }, { it.getInt("position") }))
        if (mode == CodexMode.INTERACTIVE) return verifiedInteractiveLaunch(ordered, observations)
        for ((index, evidence) in ordered.withIndex()) {
            val call = evidence.getJSONObject("call")
            val submission = submission(ordered, index) ?: continue
            val args = call.getJSONObject("arguments")
            val commands = shellCommands(args.optString("text"))
            val launches = commands.indices.filter { codexExec.containsMatchIn(commands[it]) }
            if (launches.isEmpty()) continue
            val markers = launches.mapNotNull { commands.getOrNull(it + 1)?.let { command -> exitPrint.matchEntire(command)?.groupValues?.get(1) } }
            val observed = observations.firstOrNull { observation ->
                after(observation, submission) &&
                    (codexBanner.containsMatchIn(observation.body) || observation.body.lineSequence().any { line -> markers.any { line.trimEnd('\r') == "$it=0" } })
            } ?: continue
            return JSONObject().put("mode", "exec").put("call_record_id", evidence.getString("record_id"))
                .put("submission_record_id", submission.getString("record_id")).put("terminal_record_id", observed.recordId)
        }
        return null
    }

    private fun verifyCalls(items: JSONArray, message: String, mode: CodexMode = CodexMode.EXEC, validate: Boolean = true,
                            loadRecord: (String, String?) -> String = ::record) {
        report.put("evidence_collection_complete", false)
        val calls = JSONArray()
        report.put("tool_calls", calls)
        val terminalReads = JSONArray()
        report.put("terminal_observations", terminalReads)
        var foundMessage = false
        var foundElapsedWait = false
        val seen = mutableSetOf<String>()
        val observations = mutableListOf<TerminalEvidence>()
        for (index in 0 until items.length()) {
            val item = items.getJSONObject(index)
            var value = item.getJSONObject("value")
            if (value.optBoolean("partial")) value = JSONObject(loadRecord(value.getString("record_id"), null))
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
                        val body = loadRecord(id, null)
                        val call = JSONObject(body)
                        precedingCall = JSONObject().put("record_id", id).put("interaction_id", item.getString("id")).put("call", call)
                            .put("call_body", body).put("sequence", item.getLong("sequence")).put("position", position)
                        calls.put(precedingCall)
                    }
                    "tool_result" -> {
                        val call = precedingCall ?: continue
                        val name = call.getJSONObject("call").optString("name")
                        val body = loadRecord(id, null)
                        val result = JSONTokener(body).nextValue()
                        call.put("result_record_id", id).put("result", result).put("result_body", body).put("result_position", position)
                        if (name == "wait" && result is JSONObject && result.length() == 1 && result.opt("elapsed_ms") is Number && result.getLong("elapsed_ms") > 0) foundElapsedWait = true
                    }
                    else -> if (entry.optString("kind") == "text" && precedingCall?.getJSONObject("call")?.optString("name") == "read_terminal") {
                        val body = loadRecord(id, item.getString("id"))
                        observations.add(TerminalEvidence(id, item.getLong("sequence"), body, position))
                        terminalReads.put(JSONObject().put("record_id", id).put("interaction_id", item.getString("id"))
                            .put("call_record_id", precedingCall.getString("record_id")).put("sequence", item.getLong("sequence"))
                            .put("position", position).put("body", body))
                    }
                }
            }
        }
        report.put("submitted_message_found", foundMessage).put("pure_wait_verified", foundElapsedWait)
        val recorded = (0 until calls.length()).map { calls.getJSONObject(it) }
        val launch = verifiedLaunch(recorded, observations, mode)
        report.put("verified_codex_launch", launch ?: JSONObject.NULL).put("evidence_collection_complete", true)
        if (!validate) return
        assertTrue("This Run did not contain the message submitted from the UI", foundMessage)
        assertTrue("No actual wait tool-call record in this Run", recorded.any { it.getJSONObject("call").optString("name") == "wait" })
        assertTrue("No wait result containing only a positive elapsed_ms in this Run", foundElapsedWait)
        assertNotNull(if (mode == CodexMode.INTERACTIVE)
            "No accepted, submitted interactive Codex launch with archived TUI banner and submitted prompt readback (or a non-interactive exec was submitted)"
            else "No accepted, submitted codex exec followed by archived Codex launch/completion output", launch)
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

    @Test fun interactiveCodexEvidenceRequiresLaunchSubmissionAndPromptReadback() {
        val prompt = "Create and run a pelican riding a bicycle character animation."
        val banner = TerminalEvidence("tui", 2, "╭──────────────────────────────────────╮\n│ >_ OpenAI Codex (v0.159.2)            │\n╰──────────────────────────────────────╯\n")
        val echo = TerminalEvidence("prompt-echo", 4, "› Create and run a pelican riding a\n  bicycle character animation.\n\n• Creating the animation.\n")
        val task = fixtureCall("input_text", JSONObject().put("text", prompt).put("submit", true), sequence = 3)
        for (command in listOf("codex", "cd /fixture && command /usr/local/bin/codex --no-alt-screen --sandbox workspace-write",
                "exec '/usr/local/bin/codex' -C '/fixture path' -c 'model_reasoning_effort=\"high\"'")) {
            val launch = fixtureCall("input_text", JSONObject().put("text", command).put("submit", true))
            val verified = verifiedLaunch(listOf(task, launch), listOf(echo, banner), CodexMode.INTERACTIVE)
            assertNotNull("Submitted interactive launch and task must qualify: $command", verified)
            assertEquals("interactive", verified!!.getString("mode"))
            assertEquals("tui", verified.getString("terminal_record_id"))
            assertEquals("prompt-echo", verified.getString("prompt_terminal_record_id"))
        }
        val inline = fixtureCall("input_text", JSONObject().put("text", "codex --no-alt-screen '$prompt'").put("submit", true))
        assertNotNull("An initial positional prompt is interactive", verifiedLaunch(listOf(inline), listOf(banner, echo), CodexMode.INTERACTIVE))
        val literalExecPrompt = fixtureCall("input_text", JSONObject().put("text", "codex -- 'exec'").put("submit", true))
        assertNotNull("After --, exec is a prompt rather than a subcommand", verifiedLaunch(listOf(literalExecPrompt), listOf(banner, echo.copy(body = "› exec\n")), CodexMode.INTERACTIVE))
        val launch = fixtureCall("input_text", JSONObject().put("text", "codex").put("submit", true))
        assertNull("Launching a TUI without a task is insufficient", verifiedLaunch(listOf(launch), listOf(banner), CodexMode.INTERACTIVE))
        assertNull("Accepted task input without terminal readback is insufficient", verifiedLaunch(listOf(launch, task), listOf(banner), CodexMode.INTERACTIVE))
        assertNull("An unrelated task echo is insufficient", verifiedLaunch(listOf(launch, task), listOf(banner, echo.copy(body = "› Some other task\n")), CodexMode.INTERACTIVE))
        assertNull("Task readback before its submission is insufficient", verifiedLaunch(listOf(launch, task), listOf(banner, echo.copy(sequence = 2)), CodexMode.INTERACTIVE))
        assertNull("An exec banner is not a TUI banner", verifiedLaunch(listOf(launch, task), listOf(banner.copy(body = "OpenAI Codex (v0.159.2)\n"), echo), CodexMode.INTERACTIVE))
        assertNull("Assistant descriptions are not TUI output", verifiedLaunch(listOf(launch, task), listOf(banner.copy(body = "Codex is running interactively."), echo), CodexMode.INTERACTIVE))
    }

    @Test fun interactiveCodexEvidenceRejectsExecProbesAndUnexecutedInput() {
        val prompt = "Draw and run the character animation"
        val banner = TerminalEvidence("tui", 2, ">_ OpenAI Codex (v0.159.2)\n")
        val echo = TerminalEvidence("echo", 4, "› $prompt\n")
        val task = fixtureCall("input_text", JSONObject().put("text", prompt).put("submit", true), sequence = 3)
        fun input(text: String, submit: Boolean = true, result: JSONObject? = JSONObject().put("accepted", true)) =
            fixtureCall("input_text", JSONObject().put("text", text).put("submit", submit), result)
        for (command in listOf("codex exec '$prompt'", "codex --no-alt-screen exec '$prompt'", "codex -C /fixture e '$prompt'",
                "codex --version", "command -v codex; codex --version", "codex --help", "codex login", "codex completion zsh",
                "echo codex", "echo 'sample; codex'", "echo sample # codex", "cat <<EOF\ncodex\nEOF", "echo \$(codex)")) {
            assertNull("Non-interactive commands and probes must fail: $command", verifiedLaunch(listOf(input(command), task), listOf(banner, echo), CodexMode.INTERACTIVE))
        }
        assertNull("An unsubmitted launch is a draft", verifiedLaunch(listOf(input("codex", false), task), listOf(banner, echo), CodexMode.INTERACTIVE))
        for (result in listOf(null, JSONObject().put("error", "lease_lost").put("executed", false),
                JSONObject().put("accepted", true).put("executed", false))) {
            assertNull("A launch without an accepted execution receipt must fail", verifiedLaunch(listOf(input("codex", result = result), task), listOf(banner, echo), CodexMode.INTERACTIVE))
        }
        val launch = input("codex")
        for (result in listOf(null, JSONObject().put("error", "lease_lost"), JSONObject().put("accepted", true).put("executed", false))) {
            val rejectedTask = fixtureCall("input_text", JSONObject().put("text", prompt).put("submit", true), result, 3)
            assertNull("A task without an accepted execution receipt must fail", verifiedLaunch(listOf(launch, rejectedTask), listOf(banner, echo), CodexMode.INTERACTIVE))
        }
        val exec = fixtureCall("input_text", JSONObject().put("text", "codex --sandbox workspace-write exec task").put("submit", true), sequence = 5)
        assertNull("A valid interactive launch must not excuse another submitted exec", verifiedLaunch(listOf(launch, task, exec), listOf(banner, echo), CodexMode.INTERACTIVE))
        assertNull("Exit markers cannot replace an interactive banner", verifiedLaunch(listOf(launch, task), listOf(banner.copy(body = "CODEX_FIXTURE_EXIT=0\n"), echo), CodexMode.INTERACTIVE))
    }

    @Test fun interactiveCodexEvidenceRequiresUnchangedDraftAndOrderedReceipts() {
        val prompt = "Create and run the animation"
        val launchDraft = fixtureCall("input_text", JSONObject().put("text", "codex"))
        val launchEnter = fixtureCall("send_keys", JSONObject().put("key", "enter"), sequence = 2)
        val banner = TerminalEvidence("tui", 3, ">_ OpenAI Codex (v0.159.2)\n")
        val taskDraft = fixtureCall("input_text", JSONObject().put("text", prompt), sequence = 4)
        val taskEnter = fixtureCall("send_keys", JSONObject().put("key", "enter"), sequence = 5)
        val echo = TerminalEvidence("echo", 6, "› $prompt\n")
        val calls = listOf(launchDraft, launchEnter, taskDraft, taskEnter)
        assertNotNull(verifiedLaunch(calls, listOf(banner, echo), CodexMode.INTERACTIVE))
        for (args in listOf(JSONObject().put("key", "enter").put("modifiers", JSONArray().put("ctrl")),
                JSONObject().put("key", "enter").put("repeat", 2), JSONObject().put("key", "tab"))) {
            val otherKey = fixtureCall("send_keys", args, sequence = 5)
            assertNull("Only a plain single Enter submits a draft", verifiedLaunch(listOf(launchDraft, launchEnter, taskDraft, otherKey), listOf(banner, echo), CodexMode.INTERACTIVE))
        }
        val changed = fixtureCall("input_text", JSONObject().put("text", " changed"), sequence = 5)
        assertNull("The launch draft must be unchanged", verifiedLaunch(listOf(launchDraft, changed, launchEnter.copyJsonSequence(6), taskDraft.copyJsonSequence(8), taskEnter.copyJsonSequence(9)),
            listOf(banner.copy(sequence = 7), echo.copy(sequence = 10)), CodexMode.INTERACTIVE))
        assertNull("The task draft must be unchanged", verifiedLaunch(listOf(launchDraft, launchEnter, taskDraft, changed, taskEnter.copyJsonSequence(6)),
            listOf(banner, echo.copy(sequence = 7)), CodexMode.INTERACTIVE))
        val launch = fixtureCall("input_text", JSONObject().put("text", "codex").put("submit", true)).put("result_position", 2)
        val task = fixtureCall("input_text", JSONObject().put("text", prompt).put("submit", true), sequence = 2).put("result_position", 2)
        assertNull("Same-unit output before the launch receipt must fail", verifiedLaunch(listOf(launch, task),
            listOf(banner.copy(sequence = 1, position = 1), echo.copy(sequence = 2, position = 3)), CodexMode.INTERACTIVE))
        assertNull("Same-unit output before the task receipt must fail", verifiedLaunch(listOf(launch, task),
            listOf(banner.copy(sequence = 1, position = 3), echo.copy(sequence = 2, position = 1)), CodexMode.INTERACTIVE))
        assertNotNull("Same-unit output after both receipts must qualify", verifiedLaunch(listOf(launch, task),
            listOf(banner.copy(sequence = 1, position = 3), echo.copy(sequence = 2, position = 3)), CodexMode.INTERACTIVE))
    }

    private fun JSONObject.copyJsonSequence(sequence: Long) = JSONObject(toString()).put("sequence", sequence)

    @Test fun codexDraftSubmissionAcceptsEmptyInputEnterAfterExplicitlyUnexecutedWrites() {
        // Mirrors initial-attempt: bracketed-pasted codex\n remains a draft; return is unsupported;
        // the accepted empty input_text with submit=true sends the Enter that actually launches it.
        val draft = fixtureCall("input_text", JSONObject().put("text", "codex\n"))
        val wait = fixtureCall("wait", JSONObject().put("duration_ms", 4000), JSONObject().put("elapsed_ms", 4002), 2)
        val read = fixtureCall("read_terminal", JSONObject().put("mode", "screen"), result = null, sequence = 3)
        val rejected = fixtureCall("send_keys", JSONObject().put("key", "return"),
            JSONObject().put("error", "unsupported_key").put("executed", false), 4)
        val enter = fixtureCall("input_text", JSONObject().put("text", "").put("submit", true), sequence = 5)
        val ordered = listOf(draft, wait, read, rejected, enter)
        assertEquals("The empty-input receipt submits the unchanged launch draft", "call-5", submission(ordered, 0)!!.getString("record_id"))
        val taskDraft = fixtureCall("input_text", JSONObject().put("text", "Create and run the animation"), sequence = 7)
        val taskRejected = fixtureCall("input_text", JSONObject().put("text", " unwanted change"),
            JSONObject().put("error", "lease_lost").put("executed", false), 8)
        val taskEnter = fixtureCall("input_text", JSONObject().put("text", "").put("submit", true), sequence = 9)
        val output = listOf(TerminalEvidence("tui", 6, ">_ OpenAI Codex (v0.159.2)\n"),
            TerminalEvidence("echo", 10, "› Create and run the animation\n"))
        val verified = verifiedLaunch(ordered + listOf(taskDraft, taskRejected, taskEnter), output, CodexMode.INTERACTIVE)
        assertNotNull("Both interactive launch and prompt support empty-input Enter", verified)
        assertEquals("call-5", verified!!.getString("submission_record_id"))
        assertEquals("call-9", verified.getString("prompt_submission_record_id"))
        val execDraft = fixtureCall("input_text", JSONObject().put("text", "codex exec task\n"))
        assertNotNull("Exec compatibility includes the same Enter encoding", verifiedLaunch(listOf(execDraft, rejected, enter),
            listOf(TerminalEvidence("exec-output", 6, "OpenAI Codex (v0.159.2)\n"))))
        val keyEnter = fixtureCall("send_keys", JSONObject().put("key", "enter"), sequence = 5)
        assertEquals("Plain send_keys Enter still works after an unexecuted rejection", "call-5",
            submission(listOf(draft, rejected, keyEnter), 0)!!.getString("record_id"))
    }

    @Test fun codexDraftSubmissionDoesNotSkipChangedDraftsOtherKeysOrUncertainWrites() {
        val draft = fixtureCall("input_text", JSONObject().put("text", "codex"))
        val enter = fixtureCall("input_text", JSONObject().put("text", "").put("submit", true), sequence = 3)
        val interfering = listOf(
            fixtureCall("input_text", JSONObject().put("text", " --version"), sequence = 2),
            fixtureCall("input_text", JSONObject().put("text", " ").put("submit", true), sequence = 2),
            fixtureCall("input_text", JSONObject().put("text", "").put("submit", false), sequence = 2),
            fixtureCall("send_keys", JSONObject().put("key", "escape"), sequence = 2),
            fixtureCall("send_keys", JSONObject().put("key", "return"), JSONObject().put("error", "unsupported_key"), 2),
            fixtureCall("input_text", JSONObject().put("text", " changed"), result = null, sequence = 2),
            fixtureCall("input_text", JSONObject().put("text", " changed"), JSONObject().put("accepted", false), 2))
        for (write in interfering) assertNull("Only explicitly unexecuted writes may be skipped: $write",
            submission(listOf(draft, write, enter), 0))
        val rejectedEnter = fixtureCall("input_text", JSONObject().put("text", "").put("submit", true),
            JSONObject().put("error", "lease_lost").put("executed", false), 3)
        assertNull("A rejected empty-input Enter alone does not submit the draft", submission(listOf(draft, rejectedEnter), 0))
    }

    @Test fun pausedWorkflowDiagnosticsKeepRawCallsResultsAndTerminalBodies() {
        val pause = "Run paused: tool_timeout_outcome_unknown"
        report.put("error", pause)
        val records = mapOf(
            "state-call" to "{\"name\":\"get_terminal_state\",\"arguments\":{}}",
            "state-result" to "{\"state\":\"running\"}",
            "list-call" to "{\"name\":\"list_sessions\",\"arguments\":{}}",
            "list-result" to "[]",
            "input-call" to "{\"name\":\"input_text\",\"arguments\":{\"text\":\"codex\",\"submit\":true}}",
            "input-result" to "{\"error\":\"lease_lost\",\"executed\":false}",
            "read-call" to "{\"name\":\"read_terminal\",\"arguments\":{\"mode\":\"screen\"}}",
            "terminal" to ">_ OpenAI Codex (v0.159.2)\nwaiting for approval\n")
        val entries = JSONArray()
        for ((id, _) in records) entries.put(JSONObject().put("record_id", id).put("kind", if (id == "terminal") "text" else "associated_text")
            .put("source", if (id.endsWith("-call")) "tool_call" else if (id.endsWith("-result")) "tool_result" else JSONObject.NULL))
        val history = JSONArray().put(JSONObject().put("id", "unit").put("kind", "interaction").put("sequence", 1)
            .put("value", JSONObject().put("records", entries)))
        verifyCalls(history, "missing while paused", CodexMode.INTERACTIVE, validate = false) { id, unit ->
            if (id == "terminal") assertEquals("unit", unit) else assertNull(unit)
            records.getValue(id)
        }
        assertEquals("Diagnostics must preserve the real pause error", pause, report.getString("error"))
        assertTrue(report.getBoolean("evidence_collection_complete"))
        assertFalse(report.getBoolean("submitted_message_found"))
        assertFalse(report.getBoolean("pure_wait_verified"))
        assertTrue(report.isNull("verified_codex_launch"))
        val calls = report.getJSONArray("tool_calls")
        assertEquals("Include all archived tools, including rejected writes", 4, calls.length())
        assertEquals(records.getValue("state-call"), calls.getJSONObject(0).getString("call_body"))
        assertEquals(records.getValue("list-result"), calls.getJSONObject(1).getString("result_body"))
        assertEquals("lease_lost", calls.getJSONObject(2).getJSONObject("result").getString("error"))
        val observations = report.getJSONArray("terminal_observations")
        assertEquals(1, observations.length())
        assertEquals(records.getValue("terminal"), observations.getJSONObject(0).getString("body"))
        assertEquals("read-call", observations.getJSONObject(0).getString("call_record_id"))
    }

    @Test fun terminalAgentWorkflowUsesConfiguredModelAndRealTools() {
        val file = File(context.filesDir, "terminal-agent-workflow-fixture.json")
        assumeTrue("Explicit private workflow fixture required", file.isFile)
        val started = SystemClock.elapsedRealtime()
        var originalIdentity: List<String>? = null
        var submitted = false
        var root = ""
        var message = ""
        var codexMode = CodexMode.EXEC
        var failure: Throwable? = null
        try {
            assertTrue("Fixture exceeds 64 KiB", file.length() <= 65536)
            val fixture = JSONObject(file.readText())
            session = fixture.getString("session")
            message = fixture.getString("message").trim()
            val mode = fixture.optString("codex_mode", "exec")
            assertTrue("codex_mode must be exec or interactive", mode in setOf("exec", "interactive"))
            codexMode = if (mode == "interactive") CodexMode.INTERACTIVE else CodexMode.EXEC
            val expectedModel = fixture.getString("expected_model")
            val previousRoot = fixture.optString("previous_root_user_message_id").takeUnless { it.isBlank() || it == "null" }
            val seconds = fixture.optLong("timeout_seconds", 1800)
            assertTrue("Invalid fixture fields", session.isNotBlank() && message.isNotBlank() && expectedModel.isNotBlank() && seconds in 1L..3600L)
            deadline = started + seconds * 1000
            report.put("session", session).put("expected_model", expectedModel).put("timeout_seconds", seconds)
                .put("codex_mode", mode).put("diagnostic_grace_seconds", 30)
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
            verifyCalls(currentHistory, message, codexMode)
        } catch (error: Throwable) { failure = error; report.put("error", error.toString()) }
        finally {
            // A timeout does not cancel the Desktop Run or send any terminal input.
            deadline = maxOf(deadline, started) + 30000
            if (submitted && ::remote.isInitialized && failure != null) {
                try {
                    report.put("state", agent("state"))
                    if (root.isNotEmpty() && !report.optBoolean("evidence_collection_complete"))
                        verifyCalls(history(root), message, codexMode, validate = false)
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
