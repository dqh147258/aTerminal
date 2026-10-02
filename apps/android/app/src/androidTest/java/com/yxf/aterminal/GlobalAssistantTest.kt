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
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class GlobalAssistantTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private fun <T> main(action: () -> T): T {
        var result: T? = null; var failure: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (e: Throwable) { failure = e } }
        failure?.let { throw it }; @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun all(v: View): List<View> = listOf(v) + if (v is ViewGroup) (0 until v.childCount).flatMap { all(v.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun icon(name: String) = views().first { it.contentDescription == name }
    private fun input() = views().filterIsInstance<EditText>().first()
    private fun panel() = MainActivity::class.java.getDeclaredMethod("panel", String::class.java, Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(activity, "全局AI助手", false) as LinearLayout
    private fun waitFor(name: String, check: () -> Boolean) { val end = SystemClock.elapsedRealtime() + 12000; while (SystemClock.elapsedRealtime() < end) { if (check()) return; Thread.sleep(50) }; fail("Timed out: $name") }
    private fun launch() {
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui", true).putExtra("render_fixture", true))
        waitFor("activity") { main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull { it.hasWindowFocus() }?.also { activity = it } != null } }
    }
    private fun shot(name: String) { instrumentation.waitForIdleSync(); Thread.sleep(300); instrumentation.uiAutomation.takeScreenshot()?.let { b -> File(context.filesDir, "global-$name.png").outputStream().use { b.compress(Bitmap.CompressFormat.PNG, 100, it) }; b.recycle() } }
    private fun row(id: String, title: String = "新会话", state: String = "idle", sequence: Int = 0) = JSONObject().put("scope", JSONObject().put("agent", id).put("session", JSONObject.NULL)).put("title", title).put("state", state).put("last_reply_sequence", sequence).put("preview", if (sequence > 0) "可以，巡检顺序已整理好：确认设备连接，查看终端状态，汇总未完成任务。" else "").put("updated_at", System.currentTimeMillis())

    @Test fun entryFullPageBackNavigationAndBlankNewConversation() {
        launch()
        try {
            main {
                val entry = icon("全局AI助手"); val terminal = icon("AI 对话")
                assertTrue(entry is ImageButton); assertEquals(terminal.layoutParams.width, entry.layoutParams.width); assertEquals(terminal.layoutParams.height, entry.layoutParams.height)
                entry.performClick(); assertTrue(views().any { it.contentDescription == "新增会话" }); assertFalse(icon("新增会话").isEnabled)
                MainActivity::class.java.getDeclaredMethod("openChat", Conversation::class.java, JSONObject::class.java).apply { isAccessible = true }.invoke(activity, null, row("blank"))
                assertTrue(views().any { it.contentDescription == "返回全局AI助手" })
                assertTrue(views().filterIsInstance<TextView>().any { it.text.toString() == "新会话" })
                assertFalse(views().filterIsInstance<TextView>().any { it.text.contains("从当前工作空间") || it.text.contains("目标终端") || it.text.contains("查看在线终端") })
                val overlay = MainActivity::class.java.getDeclaredField("overlayPanel").apply { isAccessible = true }.get(activity) as View
                overlay.post { assertEquals((overlay.parent as View).width, overlay.width) }
                activity.onBackPressed(); assertTrue(views().any { it.contentDescription == "新增会话" })
                icon("返回首页").performClick(); assertTrue(views().any { it.contentDescription == "AI 对话" })
            }
        } finally { main { activity.finish() } }
    }

    @Test fun listUnreadRunningDraftMigrationAndLateSendStayInTheirConversation() {
        launch()
        val identity = listOf("test-server", UUID.randomUUID().toString(), "test-desktop")
        val store = GlobalConversationStore(context, identity)
        val draftStore = AgentDraftStore(context, identity)
        val started = CountDownLatch(1); val release = CountDownLatch(1)
        var list: GlobalConversationPanel? = null; var chat: AgentPanel? = null
        val rows = listOf(row("one", "工作空间巡检", "running", 8), row("two", "项目改动梳理", "completed", 4), row("three", "构建结果整理", "completed"), row("four", "下一步安排", "completed")).mapIndexed { index, item -> item.put("updated_at", System.currentTimeMillis() - index * 60000) }
        store.merge(rows)
        draftStore.save("global:one", JSONObject().put("scroll", 0))
        draftStore.save("", JSONObject().put("text", "旧全局草稿"))
        draftStore.migrateGlobal("legacy"); draftStore.save("", JSONObject().put("text", "之后的旧草稿")); draftStore.migrateGlobal("legacy")
        assertEquals("旧全局草稿", draftStore.read("global:legacy").getString("text"))
        val calls = java.util.concurrent.CopyOnWriteArrayList<JSONObject>()
        val rpc: (String, String) -> String = { session, json ->
            assertEquals("", session)
            val command = JSONObject(json); calls.add(command)
            val id = command.getString("agent_id")
            when (command.getString("action")) {
                "permissions" -> """{"permission_mode":"ask","full_authorization":false,"revision":0,"can_mutate":true}"""
                "pending" -> """{"items":[],"cursor":null,"has_more":false}"""
                "state" -> JSONObject().put("state", "idle").put("history_generation", 0).toString()
                "history" -> JSONObject().put("generation", 0).put("has_more", false).put("items", JSONArray().apply {
                    if (id == "one") for (i in 8 downTo 1) put(JSONObject().put("id", "$id-$i").put("sequence", i).put("kind", if (i % 2 == 0) "assistant" else "user").put("value", JSONObject().put(if (i % 2 == 0) "text" else "message", if (i % 2 == 0) "先确认 Desktop 连接，再查看在线终端，最后逐个确认是否有未完成任务。\n本轮没有执行终端命令。" else "帮我整理一下工作空间巡检的顺序。")))
                }).toString()
                "send" -> { started.countDown(); check(release.await(10, TimeUnit.SECONDS)); "{\"state\":\"running\"}" }
                else -> error("Unexpected command $command")
            }
        }
        fun open(item: JSONObject) { list?.close(); list = null; chat?.close(); chat = AgentPanel(activity, panel(), identity, "", { true }, rpc, {}, workingPath = { "Mac Studio" }, globalConversation = item, back = { }) }
        try {
            main {
                list = GlobalConversationPanel(activity, panel(), identity, "Mac Studio", { true }, { command ->
                    if (JSONObject(command).getString("action") == "global_create") JSONObject().put("scope", row("new").getJSONObject("scope")).toString()
                    else JSONObject().put("conversations", JSONArray(rows)).put("cursor", JSONObject.NULL).toString()
                }, {}, { open(it) })
            }
            waitFor("running and unread") { main { views().filterIsInstance<ProgressBar>().isNotEmpty() && views().filterIsInstance<TextView>().count { it.text.toString().trim() == "未读" } == 2 } }
            shot("list")
            main { icon("新增会话").performClick() }
            waitFor("new chat") { main { views().any { it.contentDescription == "返回全局AI助手" } } }
            shot("new")
            main { assertTrue(input().text.isEmpty()); open(rows[0]) }
            waitFor("history") { main { views().filterIsInstance<TextView>().any { it.text.toString().startsWith("先确认 Desktop") } } }
            shot("history")
            main {
                // Opening an old reading position must not mark unseen replies as read.
                assertTrue(store.unread(rows[0])); assertTrue(store.unread(rows[1]))
                val scroll = AgentPanel::class.java.getDeclaredField("scroll").apply { isAccessible = true }.get(chat) as ScrollView
                scroll.fullScroll(View.FOCUS_DOWN)
            }
            waitFor("actually read") { !store.unread(rows[0]) }
            assertTrue(store.unread(rows[1]))
            main { input().setText("任务 A"); icon("发送").performClick() }
            assertTrue(started.await(5, TimeUnit.SECONDS))
            main { open(rows[1]); input().setText("会话 B 草稿") }
            release.countDown()
            waitFor("late send persisted to A") { draftStore.read("global:one").optString("text") == "" }
            main { assertEquals("会话 B 草稿", input().text.toString()); assertEquals("会话 B 草稿", draftStore.read("global:two").optString("text")); assertTrue(store.unread(rows[1])) }
            assertEquals("one", calls.first { it.optString("action") == "send" }.getString("agent_id"))
            main { open(rows[0]); assertTrue(input().text.isEmpty()); open(rows[1]); assertEquals("会话 B 草稿", input().text.toString()) }
            assertTrue(GlobalConversationStore(context, identity).unread(rows[1]))
            assertTrue(GlobalConversationStore(context, identity + "other").rows().isEmpty())
        } finally { release.countDown(); main { list?.close(); chat?.close(); activity.finish() } }
    }
}
