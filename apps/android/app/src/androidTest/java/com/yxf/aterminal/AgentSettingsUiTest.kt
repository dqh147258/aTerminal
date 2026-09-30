package com.yxf.aterminal

import android.content.Intent
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
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** Isolated Activity + controlled config callbacks: no network, accounts or real credentials. */
class AgentSettingsUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private lateinit var activity: MainActivity
    private lateinit var body: LinearLayout
    private lateinit var panel: AgentSettingsPanel
    private val gates = mutableListOf<CountDownLatch>()
    private fun <T> main(action: () -> T): T {
        var result: T? = null; var error: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (e: Throwable) { error = e } }
        error?.let { throw it }; @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun all(v: View): List<View> = listOf(v) + if (v is ViewGroup) (0 until v.childCount).flatMap { all(v.getChildAt(it)) } else emptyList()
    private fun field(name: String) = all(body).filterIsInstance<EditText>().first { it.hint == name }
    private fun button(text: String) = all(body).filterIsInstance<Button>().first { it.text == text }
    private fun row(title: String) = all(body).first { it.contentDescription == title }
    private fun text(value: String) = all(body).filterIsInstance<TextView>().any { it.isShown && it.text.contains(value) }
    private fun save() = all(body).first { it.tag == "settings-save" }.performClick()
    private fun waitFor(name: String, predicate: () -> Boolean) {
        val until = SystemClock.elapsedRealtime() + 8000
        while (SystemClock.elapsedRealtime() < until) { if (main(predicate)) return; Thread.sleep(30) }
        fail("Timed out: $name")
    }
    private fun gate() = CountDownLatch(1).also { gates.add(it) }
    private fun snapshot(revision: Int = 7) = JSONObject("""{"revision":$revision,"config":{
      "providers":{"p":{"id":"p","name":"Provider","connection":{"protocol":"openai_responses","endpoint":"https://example.test/v1"},"credential_revision":2,"enabled":true,"catalog_url":"https://example.test/models","secret_ref":"existing-reference"},"other":{"id":"other","name":"Other","connection":{"protocol":"anthropic","endpoint":"https://example.test"},"credential_revision":0,"enabled":true}},
      "models":{"m":{"id":"m","name":"Model","provider_id":"p","model":"model-one","context_window":128000,"max_tokens":4096,"max_rounds":17,"max_seconds":123,"read_only":true,"capabilities":{"tools":true,"vision":true,"streaming":true,"reasoning_levels":["high"]},"reasoning":{"mode":"level","level":"high"}}},
      "bindings":{"global":{"model_id":"m"},"session-default":{"model_id":"m"},"session/s":{"model_id":"m","reasoning":{"mode":"level","level":"high"}}},
      "mcp":{},"skills":{},"credentials":{"alias":"unchanged"},"skill_sources":[],"terminal_reading":{"head_lines":10,"tail_lines":20}}}""")
    private fun launch(page: String = "llm", callback: (JSONObject) -> JSONObject = { snapshot() }) {
        val context = instrumentation.targetContext
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui", true))
        waitFor("isolated activity") { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull { it.hasWindowFocus() }?.also { activity = it } != null }
        main {
            body = MainActivity::class.java.getDeclaredMethod("panel", String::class.java, Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(activity, "设置测试", false) as LinearLayout
            val header = body.getChildAt(0) as LinearLayout
            panel = AgentSettingsPanel(activity, body, { callback(JSONObject(it)).toString() }, "s", {}, page, { (header.getChildAt(0) as TextView).text = it })
            MainActivity::class.java.getDeclaredField("settingsEditor").apply { isAccessible = true }.set(activity, panel)
        }
        waitFor("loaded") { text(if (page == "llm") "作用域绑定" else if (page == "reading") "终端读取锚点" else "已安装") }
    }
    @After fun finish() { gates.forEach { it.countDown() }; if (::activity.isInitialized) main { if (::panel.isInitialized) panel.close(); activity.finish() } }

    @Test fun providerValidationAzureBusyFailureDraftAndBackClearSecret() {
        val count = AtomicInteger(); val entered = gate(); val release = gate()
        launch { command ->
            if (command.getString("action") == "replace") { count.incrementAndGet(); entered.countDown(); check(release.await(8, TimeUnit.SECONDS)); error("fixture-save-failed") }
            snapshot()
        }
        lateinit var secret: EditText
        main {
            button("添加供应商").performClick(); field("供应商 ID").setText("p"); save()
            assertNotNull(field("供应商 ID").error); assertEquals(0, count.get())
            field("供应商 ID").setText("new-provider")
            val protocol = all(body).filterIsInstance<Spinner>().single(); protocol.setSelection(4)
        }
        waitFor("Azure group") { all(body).filterIsInstance<TextView>().any { it.text == "Azure API version *" && it.isShown } }
        main { save(); assertNotNull(field("Azure API version").error); field("Azure API version").setText("2025-01-01"); secret = field("API 密钥（留空保留）"); secret.setText("fixture-only"); save(); save() }
        assertTrue(entered.await(8, TimeUnit.SECONDS)); assertEquals(1, count.get())
        main { assertFalse(all(body).first { it.tag == "settings-save" }.isEnabled) }
        release.countDown(); waitFor("save failure") { text("fixture-save-failed") }
        main {
            assertEquals("new-provider", field("供应商 ID").text.toString()); assertTrue(all(body).first { it.tag == "settings-save" }.isEnabled)
            all(body).filterIsInstance<Spinner>().single().setSelection(0)
        }
        waitFor("Azure label hidden") { all(body).filterIsInstance<TextView>().none { it.text == "Azure API version *" && it.isShown } }
        main { activity.onBackPressed(); assertEquals("", secret.text.toString()) }
        waitFor("root after back") { text("作用域绑定") }
    }

    @Test fun successfulMutationUsesReturnedSnapshotWithoutDependingOnShow() {
        val shows = AtomicInteger(); val replaces = AtomicInteger(); var payload: JSONObject? = null
        launch { command ->
            when (command.getString("action")) {
                "show" -> { check(shows.incrementAndGet() == 1) { "show unavailable after commit" }; snapshot() }
                "replace" -> { replaces.incrementAndGet(); payload = command; JSONObject().put("revision", 8).put("config", command.getJSONObject("config")) }
                else -> error("unexpected action")
            }
        }
        main { row("Provider").performClick(); assertFalse(field("供应商 ID").isEnabled); field("API 地址").setText("https://edited.test/v1"); save() }
        waitFor("returned list") { text("作用域绑定") }
        main {
            val command = payload!!; assertEquals(7, command.getInt("expected_revision")); assertEquals(0, command.getJSONObject("secrets").length())
            val config = command.getJSONObject("config"); val provider = config.getJSONObject("providers").getJSONObject("p")
            assertEquals("existing-reference", provider.getString("secret_ref")); assertEquals("https://example.test/models", provider.getString("catalog_url"))
            assertEquals("unchanged", config.getJSONObject("credentials").getString("alias")); assertEquals(17, config.getJSONObject("models").getJSONObject("m").getInt("max_rounds"))
        }
        assertEquals(1, shows.get()); assertEquals(1, replaces.get())
    }

    @Test fun revisionConflictRefreshesButKeepsDraftUntilExplicitRetry() {
        val replaces = AtomicInteger(); val shows = AtomicInteger(); var second: JSONObject? = null
        launch { command -> when (command.getString("action")) {
            "show" -> snapshot(if (shows.incrementAndGet() == 1) 7 else 9).also { if (shows.get() > 1) it.getJSONObject("config").getJSONObject("providers").getJSONObject("p").put("catalog_url", "https://new.test/models") }
            "replace" -> { if (replaces.incrementAndGet() == 1) error("config_revision_conflict"); second = command; JSONObject().put("revision", 10).put("config", command.getJSONObject("config")) }
            else -> error("unexpected action")
        } }
        main { row("Provider").performClick(); field("API 地址").setText("https://draft.test"); save() }
        waitFor("conflict") { text("配置已变化") }
        main { assertEquals("https://draft.test", field("API 地址").text.toString()); assertEquals(1, replaces.get()); save() }
        waitFor("retry completed") { text("作用域绑定") }
        assertEquals(9, second!!.getInt("expected_revision"))
        assertEquals("https://new.test/models", second!!.getJSONObject("config").getJSONObject("providers").getJSONObject("p").getString("catalog_url"))
    }

    @Test fun modelValidationResetsCapabilitiesAndPreservesUnexposedFields() {
        var payload: JSONObject? = null; val count = AtomicInteger()
        launch { command -> if (command.getString("action") == "replace") { payload = command; count.incrementAndGet(); JSONObject().put("revision", 8).put("config", command.getJSONObject("config")) } else snapshot() }
        main {
            row("model-one").performClick(); button("高级参数与思考能力").performClick()
            field("上下文窗口").setText("2048"); save(); assertNotNull(field("上下文窗口").error); assertEquals(0, count.get())
            field("上下文窗口").setText("128000"); field("Top P（可留空）").setText("0"); save(); assertNotNull(field("Top P（可留空）").error)
            field("模型 ID / Azure deployment").setText("new-model")
            assertEquals("", field("供应商声明的思考等级（逗号分隔）").text.toString())
            assertFalse(all(body).filterIsInstance<CheckBox>().first { it.text == "已确认模型支持工具" }.isChecked)
            assertEquals("", field("Top P（可留空）").text.toString()); save()
        }
        waitFor("model saved") { text("作用域绑定") }
        val config = payload!!.getJSONObject("config"); val model = config.getJSONObject("models").getJSONObject("m")
        assertEquals(17, model.getInt("max_rounds")); assertEquals(123, model.getInt("max_seconds")); assertTrue(model.getBoolean("read_only"))
        assertEquals("provider_default", model.getJSONObject("reasoning").getString("mode")); assertFalse(model.getJSONObject("capabilities").has("streaming"))
        assertFalse(config.getJSONObject("bindings").getJSONObject("session/s").has("reasoning"))
        main { row("new-model").performClick(); all(body).filterIsInstance<Spinner>().first().setSelection(1) }
        waitFor("provider reset") { field("模型 ID / Azure deployment").text.isEmpty() }
    }

    @Test fun catalogPaginationAndLateResultCannotReplaceParentForm() {
        val late = gate(); val entered = gate(); val searches = mutableListOf<JSONObject>()
        launch { command -> if (command.getString("action") != "discover") snapshot() else {
            synchronized(searches) { searches.add(command) }
            if (command.optString("search") == "late") { entered.countDown(); check(late.await(8, TimeUnit.SECONDS)) }
            JSONObject().put("models", JSONArray().put(JSONObject().put("id", if (command.has("cursor")) "page-two" else "catalog-one"))).put("cursor", if (command.has("cursor")) JSONObject.NULL else "next")
        } }
        main { row("model-one").performClick(); button("搜索供应商模型目录").performClick() }
        waitFor("catalog page one") { text("catalog-one") }
        main { button("下一页").performClick() }; waitFor("catalog page two") { text("page-two") }
        main { assertEquals("next", searches.last().getString("cursor")); field("搜索模型").setText("late"); button("搜索").performClick() }
        assertTrue(entered.await(8, TimeUnit.SECONDS)); main { activity.onBackPressed(); assertEquals("model-one", field("模型 ID / Azure deployment").text.toString()) }
        late.countDown(); instrumentation.waitForIdleSync()
        main { assertFalse(text("catalog-one")); assertEquals("model-one", field("模型 ID / Azure deployment").text.toString()) }
    }

    @Test fun bindingsRemoveOverrides() {
        var payload: JSONObject? = null
        launch { command -> if (command.getString("action") == "replace") { payload = command; JSONObject().put("revision", 8).put("config", command.getJSONObject("config")) } else snapshot() }
        main { row("作用域绑定").performClick(); all(body).filterIsInstance<Spinner>().forEach { it.setSelection(0) }; save() }
        waitFor("bindings saved") { text("作用域绑定") && all(body).none { it.tag == "settings-save" } }
        assertEquals(0, payload!!.getJSONObject("config").getJSONObject("bindings").length())
    }

    @Test fun readingRejectsInvalidLocally() {
        val calls = AtomicInteger()
        launch("reading") { command -> if (command.getString("action") != "show") calls.incrementAndGet(); snapshot() }
        main { field("首部保留行数（1–100）").setText("101"); button("保存读取设置").performClick(); assertNotNull(field("首部保留行数（1–100）").error) }
        assertEquals(0, calls.get())
    }

    @Test fun mcpJsonFailureStaysEditableAndValidImportKeepsMergeSemantics() {
        var payload: JSONObject? = null
        launch("mcp") { command -> if (command.getString("action") == "replace") { payload = command; JSONObject().put("revision", 8).put("config", command.getJSONObject("config")) } else snapshot() }
        main {
            button("导入 MCP JSON").performClick(); val input = field("{\"mcpServers\": {...}}")
            input.setText("{broken"); save(); assertNotNull(input.error); assertNull(payload)
            input.setText("""{"mcpServers":{"sample":{"command":"/usr/bin/example","args":["--flag"],"envSecretRefs":{"TOKEN":"alias"}}}}"""); save()
        }
        waitFor("MCP imported") { text("sample") && all(body).none { it.tag == "settings-save" } }
        assertEquals("alias", payload!!.getJSONObject("config").getJSONObject("mcp").getJSONObject("sample").getJSONObject("envSecretRefs").getString("TOKEN"))
        main { row("sample").performClick(); button("查看 / 编辑").performClick(); assertTrue(field("MCP JSON").text.contains("envSecretRefs")); button("取消").performClick(); assertTrue(text("MCP 详情")) }
    }
    @Test fun skillEditorFailureKeepsMarkdownAndSuccessReturnsToDetails() {
        val edits = AtomicInteger(); var submitted: JSONObject? = null
        val fixture = snapshot().apply { getJSONObject("config").getJSONObject("skills").put("sample", JSONObject().put("id", "sample").put("name", "Sample Skill").put("description", "Fixture skill").put("enabled", true).put("root", "/fixture/sample")) }
        launch("skills") { command -> when (command.getString("action")) {
            "show" -> fixture
            "skill_read" -> JSONObject().put("body", "# Original skill")
            "skill_edit" -> { submitted = command; if (edits.incrementAndGet() == 1) error("fixture-write-failed"); fixture.put("revision", 8) }
            else -> error("unexpected action")
        } }
        main { row("Sample Skill").performClick(); button("查看 / 编辑").performClick() }
        waitFor("skill markdown") { all(body).filterIsInstance<EditText>().any { it.hint == "SKILL.md" } }
        main { field("SKILL.md").setText("# Edited skill"); save() }
        waitFor("skill failure") { text("fixture-write-failed") }
        main { assertEquals("# Edited skill", field("SKILL.md").text.toString()); save() }
        waitFor("skill details") { text("Skill 详情") && all(body).none { it.tag == "settings-save" } }
        assertEquals(7, submitted!!.getInt("expected_revision")); assertEquals("SKILL.md", submitted!!.getString("path")); assertEquals("# Edited skill", submitted!!.getString("body"))
    }

    @Test fun reasoningPreflightMatchesProtocolAndSamplingConstraints() {
        val caps = JSONObject().put("reasoning_levels", JSONArray().put("high")).put("reasoning_budget", JSONArray().put(1024).put(8192)).put("reasoning_adaptive", true).put("reasoning_disabled", true)
        val level = JSONObject().put("mode", "level").put("level", "high")
        assertNull(SettingsValidation.reasoning("openai_responses", level, caps, 4096, false))
        assertNotNull(SettingsValidation.reasoning("anthropic", level, caps, 4096, false))
        val budget = JSONObject().put("mode", "budget").put("tokens", 4096)
        assertNotNull(SettingsValidation.reasoning("anthropic", budget, caps, 4096, false))
        budget.put("tokens", 2048)
        assertNull(SettingsValidation.reasoning("anthropic", budget, caps, 4096, false))
        assertNotNull(SettingsValidation.reasoning("anthropic", budget, caps, 4096, true))
        assertNotNull(SettingsValidation.reasoning("gemini", JSONObject().put("mode", "adaptive"), caps, 4096, false))
        assertFalse(SettingsValidation.endpoint("https://user:password@example.test")); assertTrue(SettingsValidation.endpoint("https://example.test/v1"))
    }

}
