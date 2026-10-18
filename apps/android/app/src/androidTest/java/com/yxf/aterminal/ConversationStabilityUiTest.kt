package com.yxf.aterminal

import android.content.Intent
import android.graphics.Bitmap
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.IOException
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** Disposable native UI/cache fixtures. No account login, Desktop, terminal input or model calls. */
class ConversationStabilityUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun page(generation: Long, vararg items: JSONObject) = JSONObject().put("generation", generation)
        .put("items", JSONArray(items.toList())).put("has_more", false).put("cursor", JSONObject.NULL).toString()
    private fun message(id: String, text: String, kind: String = "assistant", sequence: Long = 1) = JSONObject()
        .put("id", id).put("kind", kind).put("sequence", sequence).put("created_at", 1_700_000_000_000L + sequence)
        .put("value", JSONObject().put(if (kind == "user") "message" else "text", text))
    private fun waitFor(name: String, condition: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + 12_000
        while (SystemClock.elapsedRealtime() < deadline) { if (condition()) return; Thread.sleep(20) }
        fail("Timed out: $name")
    }

    private inner class Fixture(initialPage: String = page(0), seed: (AgentDraftStore) -> Unit = {},
        private val reply: (JSONObject) -> String? = { null }) : AutoCloseable {
        val online = AtomicBoolean(true)
        val history = AtomicReference(initialPage)
        val identity = listOf("https://conversation-fixture.invalid", UUID.randomUUID().toString(), "desktop")
        val scenario = ActivityScenario.launch<MainActivity>(Intent(context, MainActivity::class.java).putExtra("isolated_ui", true))
        lateinit var activity: MainActivity
        lateinit var panel: AgentPanel
        lateinit var store: AgentDraftStore
        init {
            scenario.onActivity { host ->
                activity = host
                MainActivity::class.java.getDeclaredField("active").apply { isAccessible = true }.setBoolean(host, false)
                store = AgentDraftStore(host, identity); seed(store)
                val body = MainActivity::class.java.getDeclaredMethod("panel", String::class.java, Boolean::class.javaPrimitiveType)
                    .apply { isAccessible = true }.invoke(host, "AI Agent", false) as LinearLayout
                panel = AgentPanel(host, body, identity, "session", { online.get() }, { _, raw ->
                    val command = JSONObject(raw)
                    reply(command) ?: when (command.getString("action")) {
                        "permissions" -> """{"permission_mode":"ask","full_authorization":false,"revision":1,"can_mutate":true}"""
                        "pending" -> """{"items":[],"cursor":null,"has_more":false}"""
                        "state" -> JSONObject().put("state", "idle").put("history_generation", JSONObject(history.get()).getLong("generation")).toString()
                        "history" -> history.get()
                        "send" -> """{"state":"running"}"""
                        else -> error("Unexpected fixture request: ${command.getString("action")}")
                    }
                }, {})
                MainActivity::class.java.getDeclaredField("agentPanel").apply { isAccessible = true }.set(host, panel)
            }
        }
        fun <T> main(action: () -> T): T {
            var result: Result<T>? = null
            instrumentation.runOnMainSync { result = runCatching(action) }
            return result!!.getOrThrow()
        }
        fun views() = all(activity.window.decorView).filter { it.isShown }
        fun draft() = all(activity.window.decorView).filterIsInstance<EditText>().first { it.contentDescription == "发送任务或追加消息" }
        fun send() = all(activity.window.decorView).first { it.contentDescription == "发送" }
        fun text(value: String) = views().filterIsInstance<TextView>().any { it.text.toString() == value }
        fun call(name: String) = AgentPanel::class.java.getDeclaredMethod(name).apply { isAccessible = true }.invoke(panel)
        fun generation() = AgentPanel::class.java.getDeclaredField("generation").apply { isAccessible = true }.get(panel)
        override fun close() { scenario.onActivity { panel.close() }; scenario.close() }
    }

    @Test fun emptyGenerationClearsVisibleMessagesAndRejectsAnOlderPage() {
        Fixture(page(1, message("old", "需要清理的旧消息"))).use { f ->
            waitFor("old message rendered") { f.main { f.text("需要清理的旧消息") } }
            f.history.set(page(2)); f.main { f.call("refresh") }
            waitFor("empty generation rendered") { f.main { f.generation() == 2L && !f.text("需要清理的旧消息") } }
            f.history.set(page(1, message("old", "需要清理的旧消息")))
            f.main { AgentPanel::class.java.getDeclaredMethod("load", Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(f.panel, true) }
            waitFor("old page finished") { f.main { !AgentPanel::class.java.getDeclaredField("loading").apply { isAccessible = true }.getBoolean(f.panel) } }
            f.main { assertEquals(2L, f.generation()); assertFalse(f.text("需要清理的旧消息")) }
        }
    }

    @Test fun firstServerPageReplacesCachedHistoryFromAnotherGeneration() {
        Fixture(page(6, message("fresh", "新一代消息")), seed = { store ->
            store.save("session", JSONObject().put("generation", 5).put("items", JSONArray().put(message("cached", "已经删除的缓存消息")))
                .put("text", "保留本地草稿").put("request_id", "local-draft"))
        }).use { f ->
            waitFor("new history rendered") { f.main { f.text("新一代消息") } }
            f.main { assertFalse(f.text("已经删除的缓存消息")); assertEquals("保留本地草稿", f.draft().text.toString()) }
        }
    }

    @Test fun uncertainSendKeepsOriginalSnapshotAndRetryNeverSendsTheNewDraft() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1); val calls = AtomicInteger()
        val sent = java.util.Collections.synchronizedList(mutableListOf<JSONObject>())
        Fixture(reply = { command -> if (command.optString("action") == "send") {
            sent.add(JSONObject(command.toString()))
            if (calls.incrementAndGet() == 1) { entered.countDown(); check(release.await(8, TimeUnit.SECONDS)); throw IOException("request timeout") }
            """{"state":"running"}"""
        } else null }).use { f -> try {
            f.main { f.draft().setText("原消息 A") }
            waitFor("send permission") { f.main { f.send().isEnabled } }
            f.main { f.send().performClick() }; assertTrue(entered.await(5, TimeUnit.SECONDS))
            val originalId = f.store.pending("session")!!.getString("request_id")
            f.main {
                f.online.set(false); f.panel.connectionInterrupted()
                assertTrue(f.draft().isEnabled); f.draft().setText("新草稿 B")
            }
            release.countDown()
            waitFor("uncertain send surfaced") { f.main { f.views().any { it.tag == "pending-send" } } }
            assertEquals("原消息 A", f.store.pending("session")!!.getString("text"))
            f.main { f.online.set(true); f.panel.connectionRestored() }
            waitFor("permissions restored") { f.main {
                val authorization = AgentPanel::class.java.getDeclaredField("authorization").apply { isAccessible = true }.get(f.panel) as AgentAuthorizationPanel
                authorization.canSend
            } }
            f.main {
                AgentPanel::class.java.getDeclaredMethod("submit", JSONObject::class.java).apply { isAccessible = true }.invoke(f.panel, f.store.pending("session"))
            }
            waitFor("retry acknowledged") { f.store.pending("session") == null && calls.get() == 2 }
            f.main { assertEquals("新草稿 B", f.draft().text.toString()) }
            assertEquals(listOf("原消息 A", "原消息 A"), sent.map { it.getString("message") })
            assertEquals(listOf(originalId, originalId), sent.map { it.getString("request_id") })
        } finally { release.countDown() } }
    }

    @Test fun pendingAttachmentSurvivesLocalRemovalAndScopedAcknowledgementKeepsNewDraft() {
        Fixture().use { f -> f.main {
            val picture = JSONObject().put("file", "pending-picture").put("mime", "image/png")
            val file = f.store.file("pending-picture").apply { writeBytes(byteArrayOf(1, 2, 3)) }
            f.store.savePending("session", JSONObject().put("request_id", "send-A").put("text", "A").put("images", JSONArray().put(picture)).put("permission_mode", "ask"))
            f.store.save("session", JSONObject().put("request_id", "draft-B").put("text", "B").put("images", JSONArray()))
            f.store.deleteIfUnreferenced(file.name)
            assertTrue("Pending snapshot still owns the attachment", file.isFile)
            assertEquals("send-A", AgentDraftStore(f.activity, f.identity).pending("session")!!.getString("request_id"))
            assertFalse(f.store.acknowledge("session", "send-A", "running"))
            f.store.clearPending("session", "unrelated-request"); assertNotNull(f.store.pending("session"))
            f.store.clearPending("session", "send-A")
            assertEquals("B", f.store.read("session").getString("text")); assertFalse(file.exists())
        } }
    }

    @Test fun reconnectKeepsAnswerDraftEditableButDisablesItsSubmission() {
        Fixture(reply = { command -> if (command.optString("action") == "pending")
            """{"items":[{"id":"question","kind":"question","state":"pending","question":"下一步做什么？","session_id":"session","run_id":"run"}],"cursor":null,"has_more":false}"""
            else null }).use { f ->
            waitFor("question input") { f.main { f.views().any { it.tag == "answer:question" } } }
            f.main {
                val input = f.views().first { it.tag == "answer:question" } as EditText
                input.setText("保留正在输入的回答"); input.requestFocus(); input.setSelection(2, 5)
                f.online.set(false); f.panel.connectionInterrupted()
                val preserved = f.views().first { it.tag == "answer:question" } as EditText
                assertTrue(preserved.isEnabled); assertEquals("保留正在输入的回答", preserved.text.toString())
                assertEquals(2, preserved.selectionStart); assertEquals(5, preserved.selectionEnd)
                preserved.append("，离线继续编辑")
                assertFalse(f.views().first { it.tag == "answer-submit:question" }.isEnabled)
            }
        }
    }

    @Test fun modelRetryUsesTheExistingStatusAndLeavesTheComposerUsable() {
        Fixture(reply = { command -> if (command.optString("action") == "state")
            """{"state":"running","history_generation":0,"model_retry":{"attempt":2,"max_attempts":3,"delay_ms":500,"reason":"timeout"}}"""
            else null }).use { f ->
            waitFor("model retry status") { f.main { f.text("模型重试 2/3") } }
            f.main { assertTrue(f.draft().isEnabled); f.draft().setText("等待回复时继续编辑") }
            assertEquals("等待回复时继续编辑", f.store.read("session").getString("text"))
        }
    }

    @Test fun typingNeverRewritesTheRetainedHistorySnapshot() {
        Fixture(page(0, message("long-message", "保留的长回答。".repeat(2000)))).use { f ->
            waitFor("history retained") { f.store.read("session").optJSONArray("items")?.length() == 1 }
            val prefs = context.getSharedPreferences("agent-drafts-${ChatStore.digest(JSONArray(f.identity).toString())}", 0)
            val key = ChatStore.digest("session"); val before = prefs.getString(key, null)
            val writes = AtomicInteger()
            val listener = android.content.SharedPreferences.OnSharedPreferenceChangeListener { _, _ -> writes.incrementAndGet() }
            f.main {
                prefs.registerOnSharedPreferenceChangeListener(listener)
                try {
                    repeat(40) { f.draft().append("字") }
                    assertEquals(before, prefs.getString(key, null)); assertEquals(0, writes.get())
                    assertEquals("字".repeat(40), f.store.readComposer("session").getString("text"))
                    val local = context.getSharedPreferences("agent-composer-${ChatStore.digest(JSONArray(f.identity).toString())}", 0)
                    assertFalse(local.getString("composer:$key", "")!!.contains("保留的长回答"))
                    assertTrue(local.contains("composer:$key"))
                } finally { prefs.unregisterOnSharedPreferenceChangeListener(listener) }
            }
        }
    }

    @Test fun legacyCombinedSnapshotKeepsItsHistoryWhileComposerAndViewMigrate() {
        val identity = listOf("https://legacy-fixture.invalid", UUID.randomUUID().toString(), "desktop")
        val prefs = context.getSharedPreferences("agent-drafts-${ChatStore.digest(JSONArray(identity).toString())}", 0)
        val original = JSONObject().put("text", "旧版本草稿").put("request_id", "legacy-draft").put("images", JSONArray())
            .put("generation", 3).put("scroll", 72).put("items", JSONArray().put(message("legacy-message", "旧版本历史"))).put("search", "旧版本历史")
        prefs.edit().putString(ChatStore.digest("legacy"), original.toString()).commit()
        val store = AgentDraftStore(context, identity)
        assertEquals("旧版本草稿", store.readComposer("legacy").getString("text"))
        store.saveComposer("legacy", store.readComposer("legacy").put("text", "继续编辑").put("request_id", "new-draft"))
        store.saveView("legacy", JSONObject().put("scroll", 100))
        assertEquals(original.toString(), prefs.getString(ChatStore.digest("legacy"), null))
        store.saveHistory("legacy", original.getJSONArray("items"), "旧版本历史")
        val restored = AgentDraftStore(context, identity).read("legacy")
        assertEquals("继续编辑", restored.getString("text")); assertEquals("new-draft", restored.getString("request_id"))
        assertEquals(100, restored.getInt("scroll")); assertEquals(3, restored.getInt("generation"))
        assertEquals("legacy-message", restored.getJSONArray("items").getJSONObject(0).getString("id"))
    }

    @Test fun survivingToolDetailsBecomeReadableAgainAfterHistoryGenerationChanges() {
        val calls = AtomicInteger()
        val item = message("retained-tool", "仍然保留的工具记录", "interaction").apply { getJSONObject("value")
            .put("tools", JSONArray().put(JSONObject().put("name", "read_terminal"))).put("call_record_id", "retained-record") }
        Fixture(page(0, item), reply = { command -> if (command.optString("action") == "record") {
            calls.incrementAndGet(); """{"body":"保留的完整记录","cursor":null}"""
        } else null }).use { f ->
            waitFor("tool card") { f.main { f.views().any { it.contentDescription == "查看工具详情" } } }
            f.main { f.views().first { it.contentDescription == "查看工具详情" }.performClick() }
            f.main { f.views().first { it.tag == "record-toggle:retained-record" }.performClick() }
            waitFor("original tool record") { f.main { f.text("保留的完整记录") } }
            f.history.set(page(1, item)); f.main { f.call("refresh") }
            waitFor("surviving record confirmed in new generation") { f.main { f.generation() == 1L && calls.get() == 2 && f.text("保留的完整记录") } }
            f.main { assertFalse(f.views().filterIsInstance<TextView>().any { it.text.contains("已从对话历史中清理") }) }
        }
    }

    @Test fun reconnectPreservesDetailsAndRetriesExpandedRecordsAfterAnOldCallback() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1); val calls = AtomicInteger()
        val item = message("tool", "正在读取终端", "interaction").apply { getJSONObject("value")
            .put("tools", JSONArray().put(JSONObject().put("name", "read_terminal"))).put("call_record_id", "detail-record") }
        Fixture(page(0, item), reply = { command -> if (command.optString("action") == "record") {
            if (calls.incrementAndGet() == 1) { entered.countDown(); check(release.await(8, TimeUnit.SECONDS)); throw IOException("connection reset") }
            """{"body":"恢复后的完整记录","cursor":null}"""
        } else null }).use { f -> try {
            waitFor("tool card") { f.main { f.views().any { it.contentDescription == "查看工具详情" } } }
            f.main { f.views().first { it.contentDescription == "查看工具详情" }.performClick() }
            f.main { f.views().first { it.tag == "record-toggle:detail-record" }.performClick() }
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            val details = f.main { AgentPanel::class.java.getDeclaredField("details").apply { isAccessible = true }.get(f.panel) }
            f.main { f.online.set(false); f.panel.connectionInterrupted(); f.online.set(true); f.panel.connectionRestored() }
            release.countDown()
            waitFor("expanded record recovered") { f.main { f.text("恢复后的完整记录") } }
            f.main {
                assertSame(details, AgentPanel::class.java.getDeclaredField("details").apply { isAccessible = true }.get(f.panel))
                assertFalse(f.text("正在读取…"))
            }
            assertEquals(2, calls.get())
        } finally { release.countDown() } }
    }

    @Test fun repeatedImageRendersAreDeduplicatedAndDoNotBlockSending() {
        val entered = CountDownLatch(1); val release = CountDownLatch(1); val sent = CountDownLatch(1); val calls = AtomicInteger()
        val bytes = java.io.ByteArrayOutputStream()
        Bitmap.createBitmap(1, 1, Bitmap.Config.ARGB_8888).let { bitmap -> bitmap.compress(Bitmap.CompressFormat.PNG, 100, bytes); bitmap.recycle() }
        val imageBody = android.util.Base64.encodeToString(bytes.toByteArray(), android.util.Base64.URL_SAFE or android.util.Base64.NO_PADDING or android.util.Base64.NO_WRAP)
        val item = message("image-message", "参考图片", "user").apply { getJSONObject("value")
            .put("images", JSONArray().put(JSONObject().put("record_id", "image-record"))) }
        Fixture(page(0, item), reply = { command -> when (command.optString("action")) {
            "record" -> {
                calls.incrementAndGet(); entered.countDown(); check(release.await(8, TimeUnit.SECONDS))
                JSONObject().put("encoding", "base64url").put("body", imageBody).put("cursor", JSONObject.NULL).toString()
            }
            "send" -> { sent.countDown(); """{"state":"running"}""" }
            else -> null
        } }).use { f -> try {
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            f.main {
                repeat(6) { AgentPanel::class.java.getDeclaredMethod("render", Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(f.panel, false) }
                f.draft().setText("图片读取期间仍能发送")
            }
            waitFor("send ready") { f.main { f.send().isEnabled } }
            f.main { f.send().performClick() }
            assertTrue("A blocked image reader must not own the send queue", sent.await(3, TimeUnit.SECONDS))
            assertEquals(1, calls.get()); release.countDown()
            waitFor("image cache written") { f.store.file("record-image-record").exists() }
            f.main { AgentPanel::class.java.getDeclaredMethod("render", Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(f.panel, false) }
            assertEquals(1, calls.get())
        } finally { release.countDown() } }
    }

    @Test fun interruptedReplyHasItsOwnStatusAndNativeConversationPreview() {
        val interrupted = message("interrupted", "已完成项目结构检查，接下来", "interaction", 3).apply {
            getJSONObject("value").put("source", "model_interrupted").put("interrupted", true).put("state", "interrupted")
        }
        Fixture(page(0, message("user", "帮我检查这次修改，并说明验证结果。", "user", 1),
            message("assistant", "### 修改已整理\n\n- 保留对话和本地草稿\n- 重连时继续阅读\n\n`request_id` 用于核对原消息。", sequence = 2), interrupted)).use { f ->
            waitFor("interrupted message rendered") { f.main { f.views().any { it.tag == "interrupted-reply" } } }
            f.main {
                val block = f.views().first { it.tag == "interrupted" }
                assertFalse(all(block).any { it.contentDescription == "查看工具详情" })
                assertTrue(f.draft().isEnabled)
            }
            instrumentation.waitForIdleSync()
            val bitmap = instrumentation.uiAutomation.takeScreenshot()
            assertNotNull("Native screenshot unavailable", bitmap)
            val directory = context.filesDir.resolve("reconnect-evidence").apply { mkdirs() }
            directory.resolve("conversation-stability.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }
}
