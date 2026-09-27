package com.yxf.aterminal

import android.content.ClipboardManager
import android.content.Context
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
import java.util.concurrent.atomic.AtomicInteger

class ToolDetailsUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private var chat: AgentPanel? = null
    private fun <T> main(action: () -> T): T {
        var result: T? = null; var failure: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (e: Throwable) { failure = e } }
        failure?.let { throw it }; @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun all(v: View): List<View> = listOf(v) + if (v is ViewGroup) (0 until v.childCount).flatMap { all(v.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun icon(name: String) = views().first { it.contentDescription == name }
    private fun toggle(id: String) = views().first { it.tag == "record-toggle:$id" }.performClick()
    private fun waitFor(name: String, check: () -> Boolean) { val end = SystemClock.elapsedRealtime() + 12000; while (SystemClock.elapsedRealtime() < end) { if (check()) return; Thread.sleep(50) }; fail("Timed out: $name") }
    private fun shot(name: String) { instrumentation.waitForIdleSync(); Thread.sleep(250); instrumentation.uiAutomation.takeScreenshot()?.let { b -> File(context.filesDir, "tool-details-$name.png").outputStream().use { b.compress(Bitmap.CompressFormat.PNG, 100, it) }; b.recycle() } }
    private fun launch(record: (JSONObject) -> JSONObject) {
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui", true).putExtra("render_fixture", true))
        waitFor("activity") { main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull { it.hasWindowFocus() }?.also { activity = it } != null } }
        val item = JSONObject().put("id", "unit").put("sequence", 2).put("kind", "interaction").put("created_at", System.currentTimeMillis())
            .put("value", JSONObject().put("text", "我先读取终端，再整理结果。")
                .put("tools", JSONArray().put(JSONObject().put("name", "read_terminal")))
                .put("updates", JSONArray()
                    .put(JSONObject().put("call_record_id", "call").put("name", "read_terminal"))
                    .put(JSONObject().put("result_record_id", "result").put("name", "read_terminal"))
                    .put(JSONObject().put("record_id", "observation").put("state", "analyzed").put("summary", "当前终端处于空闲状态。已读取工作目录与最近输出，可展开查看调用参数和完整结果。"))))
        main {
            val body = MainActivity::class.java.getDeclaredMethod("panel", String::class.java, Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(activity, "AI Agent", false) as LinearLayout
            chat = AgentPanel(activity, body, listOf("tool-details-test", UUID.randomUUID().toString(), "desktop"), "session", { true }, { session, raw ->
                assertEquals("session", session)
                val command = JSONObject(raw)
                when (command.getString("action")) {
                    "state" -> JSONObject().put("state", "completed").put("history_generation", 0).toString()
                    "history" -> JSONObject().put("generation", 0).put("has_more", false).put("items", JSONArray().put(item)).toString()
                    "record" -> record(command).toString()
                    else -> error("Unexpected action")
                }
            }, {}, workingPath = { "/Volumes/Code/My/aTerminal" })
            MainActivity::class.java.getDeclaredField("agentPanel").apply { isAccessible = true }.set(activity, chat)
            views().filterIsInstance<EditText>().first().setText("保留这条草稿")
        }
        waitFor("tool entry") { main { views().any { it.contentDescription == "查看工具详情" } } }
        main { icon("查看工具详情").performClick() }
    }
    private fun finish() { main { activity.finish() } }
    @Test fun fullPageRecordsExpandIndependentlyAndCopyCompletePagedText() {
        val calls = AtomicInteger()
        val parameters = JSONObject().put("name", "read_terminal").put("arguments", JSONObject().put("mode", "tail").put("max_lines", 200).put("session_id", "terminal-main")).toString()
        val output = "$ pwd\n/Volumes/Code/My/aTerminal\n\n$ git status --short\n M apps/android/app/src/main/java/com/yxf/aterminal/AgentPanel.kt\n\n" + (1..30).joinToString("\n") { "检查项 $it：终端输出原文" } + "\n完整结果末尾"
        launch { command ->
            calls.incrementAndGet()
            val text = when (command.getString("record_id")) { "call" -> parameters; "result" -> output; else -> "关联记录原文" }
            val offset = command.optString("cursor").toIntOrNull() ?: 0; val end = minOf(offset + 160, text.length)
            JSONObject().put("body", text.substring(offset, end)).put("cursor", if (end < text.length) end.toString() else JSONObject.NULL)
        }
        try {
            main {
                assertTrue(activity.hasWindowFocus()); assertEquals(0, calls.get())
                assertFalse(views().any { it is EditText })
                assertFalse(views().any { it.tag == "record-content:call" })
                assertTrue(views().filterIsInstance<TextView>().any { it.text == "已完成" })
            }
            shot("overview")
            main { toggle("call") }
            waitFor("parameters inline") { main { views().filterIsInstance<TextView>().any { it.text.contains("max_lines") } } }
            shot("parameters")
            main { toggle("result") }
            waitFor("all result pages inline") { main { views().filterIsInstance<TextView>().any { it.text.endsWith("完整结果末尾") } } }
            assertTrue(calls.get() > 3)
            main {
                assertTrue(activity.hasWindowFocus())
                assertTrue(views().filterIsInstance<TextView>().any { it.text.contains("max_lines") })
                val card = views().first { it.tag == "record-toggle:result" }.parent as View
                ((card.parent as View).parent as ScrollView).scrollTo(0, card.top)
            }
            shot("result")
            main {
                icon("复制执行结果").performClick()
                assertEquals(output, (activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).primaryClip!!.getItemAt(0).text.toString())
            }
            val loaded = calls.get()
            main { toggle("result"); assertFalse(views().filterIsInstance<TextView>().any { it.text.endsWith("完整结果末尾") }); toggle("result") }
            assertEquals(loaded, calls.get())
            main {
                activity.onBackPressed()
                assertEquals("保留这条草稿", views().filterIsInstance<EditText>().first().text.toString())
                assertFalse(views().any { it.contentDescription == "返回对话" })
            }
        } finally { finish() }
    }
    @Test fun failedReadRetriesInlineAndLateResultCannotReopenDetails() {
        val attempts = AtomicInteger(); val started = CountDownLatch(1); val release = CountDownLatch(1)
        launch { command ->
            if (command.getString("record_id") == "call") {
                if (attempts.incrementAndGet() == 1) error("temporary record failure")
                JSONObject().put("body", "{\"mode\":\"tail\"}").put("cursor", JSONObject.NULL)
            } else { started.countDown(); check(release.await(8, TimeUnit.SECONDS)); JSONObject().put("body", "迟到的执行结果").put("cursor", JSONObject.NULL) }
        }
        try {
            main { toggle("call") }
            waitFor("inline retry") { main { views().filterIsInstance<Button>().any { it.text == "重新读取" } } }
            main { assertTrue(activity.hasWindowFocus()); views().filterIsInstance<Button>().first { it.text == "重新读取" }.performClick() }
            waitFor("retry result") { main { views().filterIsInstance<TextView>().any { it.text.contains("\"mode\"") } } }
            main { toggle("result") }
            assertTrue(started.await(5, TimeUnit.SECONDS))
            main { icon("返回对话").performClick() }
            release.countDown(); Thread.sleep(300)
            main {
                assertTrue(activity.hasWindowFocus())
                assertFalse(views().filterIsInstance<TextView>().any { it.text.contains("迟到的执行结果") })
                assertEquals("保留这条草稿", views().filterIsInstance<EditText>().first().text.toString())
            }
        } finally { release.countDown(); finish() }
    }
}
