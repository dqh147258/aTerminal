package com.yxf.aterminal

import android.app.AlertDialog
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.ai_terminal_mobile.*
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** Isolated UI/native-cache fixture: never signs in or sends commands to a Desktop. */
class ReconnectUiTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private fun field(name: String) = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }
    private fun get(activity: MainActivity, name: String) = field(name).get(activity)
    private fun set(activity: MainActivity, name: String, value: Any?) = field(name).set(activity, value)
    private fun call(activity: MainActivity, name: String) = MainActivity::class.java.getDeclaredMethod(name).apply { isAccessible = true }.invoke(activity)
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun frame(value: String = "x", revision: ULong = 5uL) = RenderFrame(2u, 4u, revision,
        List(8) { RenderCell(value, 1u, 0xeeeeeeu, 0x101014u, 0u) }, 0u, 0u, true, 1u)
    private fun scenario() = ActivityScenario.launch<MainActivity>(Intent(context, MainActivity::class.java).putExtra("isolated_ui", true))
    private fun screenshot(name: String) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.waitForIdleSync()
        val bitmap = instrumentation.uiAutomation.takeScreenshot()
        assertNotNull("Native screen capture unavailable", bitmap)
        val directory = File(context.filesDir, "reconnect-evidence").apply { mkdirs() }
        File(directory, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        bitmap.recycle()
    }

    @Test fun transientOutageKeepsAgentDraftTerminalAndNavigationAndRejectsOfflineInput() {
        scenario().use { scenario -> scenario.onActivity { activity ->
            // Keep the fixture offline so the retry scheduler cannot contact any account.
            set(activity, "active", false)
            set(activity, "accountName", "reconnect-ui-isolated")
            set(activity, "deviceId", "reconnect-desktop")
            set(activity, "selected", "reconnect-session")
            set(activity, "lastHeartbeatAt", Long.MAX_VALUE)
            (get(activity, "workspace") as View).visibility = View.VISIBLE
            ((get(activity, "loginBox") as View).parent as View).visibility = View.GONE
            MainActivity::class.java.getDeclaredMethod("show", RenderFrame::class.java).apply { isAccessible = true }.invoke(activity, frame())
            MainActivity::class.java.getDeclaredMethod("openChat", Conversation::class.java, JSONObject::class.java).apply { isAccessible = true }.invoke(activity, null, null)
            val panel = get(activity, "agentPanel")
            val overlay = get(activity, "overlay")
            val terminal = get(activity, "terminal") as TerminalView
            val generation = get(activity, "generation")
            val draft = all(overlay as View).filterIsInstance<EditText>().first { it.hint?.toString() == "发送任务或追加消息" || it.contentDescription?.toString() == "发送任务或追加消息" }
            draft.setText("保留的草稿，不可自动发送")
            terminal.showHistory(frame("h"))
            set(activity, "connected", true); set(activity, "controlled", true)
            call(activity, "beginReconnect")
            val reconnect = get(activity, "reconnect") as WorkspaceReconnect
            val epoch = reconnect.epoch
            repeat(3) { call(activity, "beginReconnect") }
            assertEquals(epoch, reconnect.epoch)
            assertSame(panel, get(activity, "agentPanel"))
            assertSame(overlay, get(activity, "overlay"))
            assertSame(terminal, get(activity, "terminal"))
            assertEquals(generation, get(activity, "generation"))
            assertEquals("reconnect-session", get(activity, "selected"))
            assertEquals("保留的草稿，不可自动发送", draft.text.toString())
            assertEquals(View.VISIBLE, (get(activity, "workspace") as View).visibility)
            assertEquals(View.GONE, (get(activity, "entryScreen") as View).visibility)
            assertEquals(View.VISIBLE, (get(activity, "reconnectBanner") as View).visibility)
            assertFalse(terminal.canType())
            var sent = 0
            val enqueue = MainActivity::class.java.getDeclaredMethod("enqueue", kotlin.jvm.functions.Function0::class.java).apply { isAccessible = true }
            assertEquals(false, enqueue.invoke(activity, { sent++; Unit }))
            assertEquals(0, sent)
        }
        screenshot("transient-agent-draft")
        scenario.onActivity { activity ->
            val reconnect = get(activity, "reconnect") as WorkspaceReconnect
            val panel = get(activity, "agentPanel")
            val draft = all(get(activity, "overlay") as View).filterIsInstance<EditText>().first { it.contentDescription?.toString() == "发送任务或追加消息" }
            assertTrue(reconnect.fail(reconnect.attempt()!!, WorkspaceReconnect.Blocked.AUTHENTICATION))
            call(activity, "updateReconnectBanner")
            assertTrue((get(activity, "reconnectBanner") as TextView).text.contains("登录已失效"))
            assertEquals("reconnect-ui-isolated", get(activity, "accountName"))
            assertSame(panel, get(activity, "agentPanel"))
            assertEquals("保留的草稿，不可自动发送", draft.text.toString())
        }
        screenshot("authentication-preserves-chat")
        }
    }

    @Test fun firstEntryRetainsItsDedicatedLoadingScreen() {
        scenario().use { scenario -> scenario.onActivity { activity ->
            MainActivity::class.java.getDeclaredMethod("beginEntry", String::class.java).apply { isAccessible = true }.invoke(activity, "正在恢复工作空间…")
            assertEquals(true, get(activity, "entryPending"))
            assertEquals(View.VISIBLE, (get(activity, "entryScreen") as View).visibility)
            assertEquals(View.GONE, (get(activity, "workspace") as View).visibility)
            assertFalse((get(activity, "reconnect") as WorkspaceReconnect).pending)
        } }
    }

    @Test fun newTransportBaselineKeepsHistoryPixelsAndAcceptsItsNewEpoch() {
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            val first = frame()
            val view = TerminalView(context, first)
            fun snapshot(value: RenderFrame, epoch: ULong) = RenderUpdate(1uL, epoch, value.revision, value.rows, value.cols, true,
                value.cells.mapIndexed { i, cell -> RenderPatch(i.toUInt(), cell) }, value.cursorRow, value.cursorCol, value.cursorVisible, value.cursorShape)
            fun pixels(): Bitmap {
                view.measure(View.MeasureSpec.UNSPECIFIED, View.MeasureSpec.UNSPECIFIED)
                view.layout(0, 0, view.measuredWidth, view.measuredHeight)
                return Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888).also { view.draw(Canvas(it)) }
            }
            view.apply(snapshot(first, 1uL)); view.showHistory(frame("h"))
            val history = pixels()
            val recovered = frame("r", 1uL)
            view.update(recovered, preserveViewport = true)
            view.apply(snapshot(recovered, 2uL))
            val after = pixels()
            assertTrue("Recovery jumped out of the local history viewport", history.sameAs(after))
            view.showHistory(null)
            val live = pixels()
            assertFalse(history.sameAs(live))
            history.recycle(); after.recycle(); live.recycle()
        }
    }

    @Test fun closeConfirmationOpenedBeforeOutageCannotClearSelectionOrCancelRecovery() {
        scenario().use { scenario ->
            var generation: Any? = null
            var epoch = 0
            scenario.onActivity { activity ->
                set(activity, "active", false)
                set(activity, "accountName", "reconnect-ui-isolated")
                set(activity, "deviceId", "reconnect-desktop")
                set(activity, "selected", "reconnect-session")
                set(activity, "connected", true)
                val session = RemoteSession("reconnect-session", "/fixture", false, 0u, true)
                MainActivity::class.java.getDeclaredMethod("closeSession", RemoteSession::class.java).apply { isAccessible = true }.invoke(activity, session)
                call(activity, "beginReconnect")
                generation = get(activity, "generation")
                epoch = (get(activity, "reconnect") as WorkspaceReconnect).epoch
            }
            val instrumentation = InstrumentationRegistry.getInstrumentation()
            instrumentation.waitForIdleSync()
            val root = instrumentation.uiAutomation.rootInActiveWindow
            assertEquals(context.packageName, root.packageName.toString())
            val confirm = root.findAccessibilityNodeInfosByText("关闭会话").first { it.isClickable && it.text?.toString() == "关闭会话" }
            assertTrue(confirm.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK))
            instrumentation.waitForIdleSync()
            scenario.onActivity { activity ->
                assertEquals(generation, get(activity, "generation"))
                assertEquals("reconnect-session", get(activity, "selected"))
                assertEquals(false, get(activity, "sessionBusy"))
                val state = get(activity, "reconnect") as WorkspaceReconnect
                assertEquals(epoch, state.epoch)
                assertTrue(state.pending)
                assertNotNull("Old confirmation must not wedge the recovery state", state.attempt())
            }
        }
    }

    @Test fun queuedSettingsSaveDoesNotReplayAfterRecoveryAndDraftSurvives() {
        val gate = CountDownLatch(1)
        val entered = CountDownLatch(1)
        val mutations = AtomicInteger()
        scenario().use { scenario ->
            lateinit var activity: MainActivity
            lateinit var body: LinearLayout
            lateinit var panel: AgentSettingsPanel
            lateinit var worker: ExecutorService
            fun drain() {
                val done = CountDownLatch(1)
                worker.execute { activity.runOnUiThread { done.countDown() } }
                assertTrue("Settings worker/UI did not drain", done.await(8, TimeUnit.SECONDS))
            }
            try {
                scenario.onActivity { host ->
                    activity = host
                    body = MainActivity::class.java.getDeclaredMethod("panel", String::class.java, Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(host, "设置测试", false) as LinearLayout
                    panel = AgentSettingsPanel(host, body, { json ->
                        if (JSONObject(json).getString("action") != "show") mutations.incrementAndGet()
                        """{"revision":7,"config":{"terminal_reading":{"head_lines":10,"tail_lines":20}}}"""
                    }, "reconnect-session", {}, "reading")
                    set(host, "settingsEditor", panel)
                    worker = AgentSettingsPanel::class.java.getDeclaredField("worker").apply { isAccessible = true }.get(panel) as ExecutorService
                }
                drain()
                worker.execute { entered.countDown(); check(gate.await(8, TimeUnit.SECONDS)) }
                assertTrue(entered.await(8, TimeUnit.SECONDS))
                lateinit var draft: EditText
                scenario.onActivity {
                    draft = all(body).filterIsInstance<EditText>().first { it.hint?.toString() == "首部保留行数（1–100）" }
                    draft.setText("7")
                    all(body).filterIsInstance<Button>().first { it.text.toString() == "保存读取设置" }.performClick()
                    panel.connectionInterrupted()
                    panel.connectionRestored()
                }
                gate.countDown(); drain()
                scenario.onActivity {
                    assertEquals("Queued save must not be replayed on the new transport", 0, mutations.get())
                    assertTrue(draft.isAttachedToWindow)
                    assertEquals("7", draft.text.toString())
                    assertTrue(draft.isEnabled)
                }
            } finally { gate.countDown() }
        }
    }

    @Test fun postDispatchCreationFailureIsUncertainEvenBeforeUiDetectsDisconnect() {
        scenario().use { scenario ->
            val instrumentation = InstrumentationRegistry.getInstrumentation()
            lateinit var activity: MainActivity
            lateinit var dialog: AlertDialog
            lateinit var worker: ExecutorService
            var epoch = 0
            scenario.onActivity { host ->
                activity = host
                // Native transport is deliberately empty; no Desktop receives this fixture call.
                // The UI remains marked connected when the dispatched native request throws.
                set(activity, "active", true)
                set(activity, "connected", true)
                set(activity, "accountName", "reconnect-ui-isolated")
                set(activity, "deviceId", "reconnect-desktop")
                set(activity, "selected", "reconnect-session")
                set(activity, "lastHeartbeatAt", Long.MAX_VALUE)
                set(activity, "lastSessionRefreshAt", Long.MAX_VALUE)
                epoch = (get(activity, "reconnect") as WorkspaceReconnect).epoch
                call(activity, "createSession")
                dialog = get(activity, "createDialog") as AlertDialog
                worker = get(activity, "worker") as ExecutorService
            }
            // Dialog.show() posts OnShowListener to the main queue. The production creation
            // listener is installed there; a same-turn performClick hits Android's default
            // dismiss handler instead and never exercises the dispatched-request failure.
            instrumentation.waitForIdleSync()
            scenario.onActivity {
                assertTrue("Creation dialog must finish showing before input", dialog.isShowing)
                assertTrue(dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled)
                assertTrue(dialog.getButton(AlertDialog.BUTTON_POSITIVE).performClick())
                assertFalse("The creation handler must enter submitting state", dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled)
                assertFalse("Submitting must disable cancellation until the result is known", dialog.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled)
                assertTrue("Submitting must not use the default dismiss handler", dialog.isShowing)
            }
            // FIFO marker follows the native create call and its posted UI callback. No network
            // timing or polling is needed: the deliberately empty native transport fails locally.
            val drained = CountDownLatch(1)
            worker.execute { activity.runOnUiThread { drained.countDown() } }
            assertTrue("Creation worker and its UI result did not drain", drained.await(8, TimeUnit.SECONDS))
            scenario.onActivity {
                assertTrue("An uncertain creation must remain visible for review", dialog.isShowing)
                assertTrue("Dispatched creation failure must remain uncertain, even with a healthy UI connection flag",
                    all(dialog.window!!.decorView).filterIsInstance<EditText>().any { it.error?.toString()?.contains("创建结果待确认") == true })
                assertEquals(true, get(activity, "connected"))
                assertEquals(epoch, (get(activity, "reconnect") as WorkspaceReconnect).epoch)
                assertFalse(dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled)
                assertTrue(dialog.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled)
                assertEquals("reconnect-session", get(activity, "selected"))
            }
        }
    }
}
