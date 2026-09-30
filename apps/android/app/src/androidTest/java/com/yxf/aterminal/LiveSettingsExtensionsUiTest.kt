package com.yxf.aterminal

import android.content.Intent
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import uniffi.ai_terminal_mobile.RemoteTerminal
import java.io.File
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.TimeUnit

/**
 * Opt-in live config round trip; no login/logout, model calls, service startup, or fake RPC.
 * Target filesDir/live-settings-extensions-fixture.json (absence => Assume skip):
 * {"desktop_id":"connected Desktop id", "skill_path":"/absolute/disposable/skill",
 *  "resource_path":"assets/probe.txt", "resource_sha256":"64 lowercase hex characters"}
 * Coordinator supplies a valid SKILL.md and a non-Markdown UTF-8 resource <=512 KiB.
 * The fixture contains no credentials. Use the normal logged-in, connected MainActivity.
 * Installation and edit must preserve the resource in the registered version package.
 * SAF picker/full-folder upload is deliberately outside this test's coverage.
 *
 * All normal mutations use production forms, including the deletion confirmation. Finally
 * drains queued UI work and removes only this run's UUID entries from a fresh revision;
 * it never restores a stale config or writes provider/model/binding/credential changes.
 */
class LiveSettingsExtensionsUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private lateinit var remote: RemoteTerminal
    private lateinit var owner: String
    private lateinit var desktop: String
    private var generation = 0

    private fun <T> main(action: () -> T): T {
        var result: T? = null; var failure: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (error: Throwable) { failure = error } }
        failure?.let { throw it }; @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun member(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun field(hint: String) = views().filterIsInstance<EditText>().first { it.hint?.toString() == hint }
    private fun button(title: String) = views().filterIsInstance<Button>().first { it.text.toString() == title && it.isEnabled }
    private fun waitFor(label: String, condition: () -> Boolean) {
        val end = SystemClock.elapsedRealtime() + 30000
        while (SystemClock.elapsedRealtime() < end) { if (condition()) return; Thread.sleep(50) }
        fail("Timed out: $label") // Do not dump the normal account UI or configuration.
    }
    private fun identity() = main {
        check(member("accountName") == owner && member("deviceId") == desktop && member("generation") == generation && member("connected") == true) { "Live fixture connection changed" }
    }
    private fun rpc(command: JSONObject): JSONObject {
        identity(); val result = JSONObject(remote.configuration(command.toString())); identity(); return result
    }
    private fun show() = rpc(JSONObject().put("action", "show"))
    private fun entry(kind: String, id: String) = show().getJSONObject("config").getJSONObject(kind).optJSONObject(id)
    private fun panel() = member("settingsEditor") as? AgentSettingsPanel
    private fun drainEditor() {
        val executor = main { panel()?.let { AgentSettingsPanel::class.java.getDeclaredField("worker").apply { isAccessible = true }.get(it) as ExecutorService } } ?: return
        val done = CountDownLatch(1)
        executor.execute { activity.runOnUiThread { done.countDown() } }
        check(done.await(30, TimeUnit.SECONDS)) { "Settings RPC/UI did not drain; refusing cleanup while a mutation may still be queued" }
        identity()
    }
    private fun click(title: String) { main { assertTrue(button(title).performClick()) }; drainEditor() }
    private fun save() {
        main { val action = views().first { it.tag == "settings-save" }; assertTrue(action.isEnabled); assertTrue(action.performClick()) }
        drainEditor()
    }
    private fun openPage(title: String) {
        drainEditor()
        main {
            MainActivity::class.java.getDeclaredMethod("settingsPanel").apply { isAccessible = true }.invoke(activity)
            assertTrue(views().first { it.contentDescription == title }.performClick())
        }
        drainEditor()
        assertTrue("Production settings panel missing", main { panel() != null })
    }
    private fun openEntry(id: String) {
        main {
            val subtitle = views().filterIsInstance<TextView>().first { it.text.toString().startsWith("$id · ") }
            val row = subtitle.parent.parent as View
            assertTrue("Expected an installed extension row", row.isClickable); assertTrue(row.performClick())
        }
    }
    private fun nodes(node: AccessibilityNodeInfo?): List<AccessibilityNodeInfo> = if (node == null) emptyList() else listOf(node) + (0 until node.childCount).flatMap { nodes(node.getChild(it)) }
    private fun deleteEntry(kind: String, id: String) {
        main { assertTrue(button("删除").performClick()) }
        waitFor("delete confirmation") { nodes(instrumentation.uiAutomation.rootInActiveWindow).any { it.text?.toString() == "删除 $id？" } }
        val confirm = nodes(instrumentation.uiAutomation.rootInActiveWindow).first { it.text?.toString() == "删除" && it.className?.toString()?.endsWith("Button") == true }
        assertTrue(confirm.performAction(AccessibilityNodeInfo.ACTION_CLICK))
        waitFor("confirmed extension deletion") { entry(kind, id) == null }
        drainEditor()
    }
    private fun hash(value: String) = MessageDigest.getInstance("SHA-256").digest(value.toByteArray(Charsets.UTF_8)).joinToString("") { "%02x".format(it.toInt() and 255) }
    private fun canonical(value: Any?): String = when (value) {
        is JSONObject -> value.keys().asSequence().toList().sorted().joinToString(",", "{", "}") { JSONObject.quote(it) + ":" + canonical(value.get(it)) }
        is JSONArray -> (0 until value.length()).joinToString(",", "[", "]") { canonical(value.get(it)) }
        null, JSONObject.NULL -> "null"
        is String -> JSONObject.quote(value)
        else -> value.toString()
    }
    private fun unrelatedHash(config: JSONObject, mcpId: String, skillId: String): String {
        val other = JSONObject(config.toString())
        other.getJSONObject("mcp").remove(mcpId); other.getJSONObject("skills").remove(skillId)
        return hash(canonical(other))
    }
    private fun verifyResource(skillId: String, path: String, expected: String, version: String) {
        val resource = rpc(JSONObject().put("action", "skill_read").put("id", skillId).put("path", path))
        assertEquals("Resource must belong to the current version package", version, resource.getString("version"))
        assertEquals("Non-Markdown resource changed or was lost", expected, hash(resource.getString("body")))
    }
    private fun cleanup(mcpId: String, skillId: String) {
        // Fresh candidates preserve every concurrent unrelated field. Retry only revision conflicts.
        repeat(3) { attempt ->
            val current = show(); val config = current.getJSONObject("config")
            if (!config.getJSONObject("mcp").has(mcpId) && !config.getJSONObject("skills").has(skillId)) return
            config.getJSONObject("mcp").remove(mcpId); config.getJSONObject("skills").remove(skillId)
            try {
                rpc(JSONObject().put("action", "replace").put("expected_revision", current.getLong("revision")).put("config", config).put("secrets", JSONObject()))
                return
            } catch (error: Exception) { if (attempt == 2 || !error.message.orEmpty().contains("revision", true)) throw error }
        }
    }

    @Test fun normalAccountMcpAndSkillFormsRoundTripWithoutChangingOtherConfig() {
        val file = File(context.filesDir, "live-settings-extensions-fixture.json")
        assumeTrue("Opt-in live-settings-extensions-fixture.json is absent", file.isFile)
        val fixture = JSONObject(file.readText())
        val expectedDesktop = fixture.getString("desktop_id")
        val path = fixture.getString("skill_path")
        val resourcePath = fixture.getString("resource_path")
        val expectedHash = fixture.getString("resource_sha256")
        require(expectedDesktop.isNotBlank() && path.startsWith('/'))
        require(!resourcePath.startsWith('/') && resourcePath.split('/').none { it.isEmpty() || it == "." || it == ".." } && !resourcePath.endsWith(".md", true))
        require(expectedHash.matches(Regex("[0-9a-f]{64}")))
        val mcpId = "live-settings-mcp-${UUID.randomUUID()}"
        val skillId = "live-settings-skill-${UUID.randomUUID()}"
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT))
        waitFor("normal connected MainActivity") { main {
            ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>()
                .firstOrNull { it.hasWindowFocus() }?.also { activity = it } != null && member("connected") == true && member("accountName").toString().isNotBlank()
        } }
        main {
            assertFalse(activity.intent.getBooleanExtra("isolated_ui", false))
            assertFalse(activity.intent.getBooleanExtra("terminal_input_test", false))
            assertFalse(activity.intent.getBooleanExtra("acceptance_test", false))
            assertFalse(activity.intent.getBooleanExtra("render_fixture", false))
            owner = member("accountName") as String; desktop = member("deviceId") as String
            assertEquals("Fixture targets a different Desktop", expectedDesktop, desktop)
            generation = member("generation") as Int; remote = member("remote") as RemoteTerminal
        }
        drainEditor()
        val before = show().getJSONObject("config")
        assertFalse(before.getJSONObject("mcp").has(mcpId)); assertFalse(before.getJSONObject("skills").has(skillId))
        val original = unrelatedHash(before, mcpId, skillId)
        var failure: Throwable? = null
        try {
            openPage("MCP"); click("导入 MCP JSON")
            val server = JSONObject().put("transport", "streamable_http").put("url", "http://127.0.0.1:9/live-settings-fixture").put("enabled", false)
            main { field("{\"mcpServers\": {...}}").setText(JSONObject().put("mcpServers", JSONObject().put(mcpId, server)).toString()) }; save()
            assertFalse("MCP import must remain disabled", entry("mcp", mcpId)!!.getBoolean("enabled"))
            openEntry(mcpId); click("查看 / 编辑")
            main {
                val editor = field("MCP JSON"); val edited = JSONObject(editor.text.toString())
                assertFalse(edited.getBoolean("enabled")); edited.put("call_timeout_ms", 12345); editor.setText(edited.toString(2))
            }; save()
            assertEquals(12345, entry("mcp", mcpId)!!.getInt("call_timeout_ms"))
            click("启用"); assertTrue(entry("mcp", mcpId)!!.getBoolean("enabled"))
            click("停用"); assertFalse(entry("mcp", mcpId)!!.getBoolean("enabled"))
            deleteEntry("mcp", mcpId)
            assertEquals("MCP flow changed unrelated configuration", original, unrelatedHash(show().getJSONObject("config"), mcpId, skillId))

            openPage("Skills"); click("安装 Desktop 上的 Skill")
            main { field("Skill ID").setText(skillId); field("Desktop 上的绝对目录").setText(path) }; save()
            val installed = entry("skills", skillId) ?: error("Skill was not installed")
            val version = installed.getString("version")
            assertTrue(version.matches(Regex("[0-9a-f]{64}")))
            assertTrue("Expected an immutable version package", installed.getString("root").replace('\\', '/').endsWith("/$version"))
            assertNotEquals("Installation must copy the complete source package", path, installed.getString("root"))
            verifyResource(skillId, resourcePath, expectedHash, version)
            val markdown = rpc(JSONObject().put("action", "skill_read").put("id", skillId).put("path", "SKILL.md")).getString("body")
            openEntry(skillId); click("查看 / 编辑")
            val updated = markdown + "\n\n<!-- live-settings-edit:$skillId -->\n"
            main { assertEquals(markdown, field("SKILL.md").text.toString()); field("SKILL.md").setText(updated) }; save()
            val edited = entry("skills", skillId) ?: error("Edited Skill missing")
            val editedVersion = edited.getString("version")
            assertNotEquals("Editing must create a new package version", version, editedVersion)
            assertTrue(edited.getString("root").replace('\\', '/').endsWith("/$editedVersion"))
            assertEquals(updated, rpc(JSONObject().put("action", "skill_read").put("id", skillId).put("path", "SKILL.md")).getString("body"))
            verifyResource(skillId, resourcePath, expectedHash, editedVersion)
            click("停用"); assertFalse(entry("skills", skillId)!!.getBoolean("enabled"))
            click("启用"); assertTrue(entry("skills", skillId)!!.getBoolean("enabled"))
            click("停用"); assertFalse(entry("skills", skillId)!!.getBoolean("enabled"))
            deleteEntry("skills", skillId)
            assertEquals("Extension forms changed unrelated configuration", original, unrelatedHash(show().getJSONObject("config"), mcpId, skillId))
        } catch (error: Throwable) { failure = error; throw error }
        finally {
            try {
                drainEditor() // Never race cleanup against a pending form submission.
                cleanup(mcpId, skillId)
                val finalConfig = show().getJSONObject("config")
                assertFalse(finalConfig.getJSONObject("mcp").has(mcpId)); assertFalse(finalConfig.getJSONObject("skills").has(skillId))
                assertEquals("Unrelated config must not be restored or modified", original, unrelatedHash(finalConfig, mcpId, skillId))
                identity() // Leave the normal logged-in account and connection intact.
            } catch (cleanupError: Throwable) {
                val detail = AssertionError("Fixture cleanup/verification failed; inspect only $mcpId and $skillId", cleanupError)
                if (failure != null) failure.addSuppressed(detail) else throw detail
            }
        }
    }
}
