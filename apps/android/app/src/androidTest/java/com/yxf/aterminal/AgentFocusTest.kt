package com.yxf.aterminal

import android.content.Intent
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.LinearLayout
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.util.UUID
import java.util.concurrent.atomic.AtomicInteger

class AgentFocusTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun <T> main(action: () -> T): T {
        var result: T? = null; var error: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (e: Throwable) { error = e } }
        error?.let { throw it }; @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    @Test fun repeatedStatesCollapseAcrossMessagesAndPagesButTransitionsRemain() {
        fun item(id: String, kind: String, state: String, session: String = "one") = JSONObject().put("id", id).put("kind", kind).put("value", JSONObject().put("session_id", session).put("session_process", JSONObject().put("state", state)))
        val chronological = listOf(item("1", "pty_status", "running"), item("2", "user", ""), item("3", "pty_status_snapshot", "running"), item("4", "pty_status", "exited"), item("5", "pty_status", "running"), item("6", "pty_status", "running", "two"), item("7", "assistant", ""))
        val newest = chronological.reversed()
        val pages = newest.take(3) + newest.drop(3) + listOf(chronological.first())
        assertEquals(listOf("1", "2", "4", "5", "6", "7"), AgentTimeline.visible(pages, false).map { it.getString("id") })
        assertEquals(listOf("7", "6", "5", "4", "2", "1"), AgentTimeline.visible(pages, true).map { it.getString("id") })
    }
    @Test fun draftFocusAndSelectionSurviveRepeatedHistoryRefresh() {
        var activity: MainActivity? = null
        var panel: AgentPanel? = null
        val cache = File(context.cacheDir, "focus-${UUID.randomUUID()}.sqlite3")
        val loads = AtomicInteger()
        try {
            context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui", true).putExtra("render_fixture", true))
            val deadline = SystemClock.elapsedRealtime() + 10000
            while (activity == null && SystemClock.elapsedRealtime() < deadline) {
                activity = main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull() }
                Thread.sleep(40)
            }
            val screen = activity!!
            main {
                val body = MainActivity::class.java.getDeclaredMethod("panel", String::class.java, Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(screen, "AI Agent", false) as LinearLayout
                panel = AgentPanel(screen, body, listOf("https://focus.invalid", "focus", "desktop"), "session", { true }, { _, text ->
                    when (JSONObject(text).getString("action")) {
                        "permissions" -> """{"permission_mode":"ask","full_authorization":false,"revision":0,"can_mutate":true}"""
                        "pending" -> """{"items":[],"cursor":null,"has_more":false}"""
                        "state" -> JSONObject().put("history_generation", 0).put("state", "running").toString()
                        "history" -> {
                            val count = loads.incrementAndGet()
                            val items = JSONArray()
                            items.put(JSONObject().put("id", "reply").put("kind", "assistant").put("value", JSONObject().put("text", "streaming reply $count")))
                            repeat(12) { i -> items.put(JSONObject().put("id", "status-$i").put("kind", "pty_status").put("value", JSONObject().put("session_id", "session").put("session_process", JSONObject().put("state", "running")))) }
                            JSONObject().put("generation", 0).put("items", items).put("cursor", JSONObject.NULL).put("has_more", false).toString()
                        }
                        else -> error("Unexpected request")
                    }
                }, {}, cachePath = cache.path)
            }
            while (loads.get() < 1) Thread.sleep(40)
            instrumentation.waitForIdleSync()
            val draft = main { all(screen.window.decorView).filterIsInstance<EditText>().first { it.contentDescription == "发送任务或追加消息" } }
            main {
                draft.setText("检查终端，不要发送这段测试草稿")
                draft.requestFocus(); draft.setSelection(2, 6)
                (screen.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager).showSoftInput(draft, 0)
            }
            val initial = loads.get()
            val until = SystemClock.elapsedRealtime() + 8000
            while (SystemClock.elapsedRealtime() < until) {
                Thread.sleep(50)
                main {
                    assertTrue("History polling stole draft focus: ${screen.currentFocus?.javaClass?.simpleName}", draft.hasFocus())
                    assertEquals("检查终端，不要发送这段测试草稿", draft.text.toString())
                    assertEquals(2, draft.selectionStart); assertEquals(6, draft.selectionEnd)
                }
                if (loads.get() >= initial + 3) break
            }
            assertTrue("Need multiple actual refreshes", loads.get() >= initial + 3)
            main {
                assertEquals("Repeated running history must collapse", 1, all(screen.window.decorView).filterIsInstance<android.widget.TextView>().count { it.text.toString() == "终端状态：running" })
                draft.onCreateInputConnection(android.view.inputmethod.EditorInfo()).commitText("继续", 1)
                assertTrue(draft.hasFocus())
                assertTrue(draft.text.toString().contains("继续"))
            }
        } finally {
            main { panel?.close(); activity?.finish() }
            listOf(cache, File(cache.path + "-wal"), File(cache.path + "-shm")).forEach { it.delete() }
        }
    }
}
