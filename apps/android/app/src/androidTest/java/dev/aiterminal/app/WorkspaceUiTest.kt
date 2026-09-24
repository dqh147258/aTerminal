package dev.aiterminal.app

import android.content.Intent
import android.content.pm.ActivityInfo
import android.graphics.Bitmap
import android.os.SystemClock
import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup
import android.widget.*
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.Before
import org.junit.After
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class WorkspaceUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private var savedFont = 16
    private var savedOpacity = 88
    @Before fun preserveDisplay() { val prefs = DisplayPreferences(context, "acceptance-display"); savedFont = prefs.fontSize; savedOpacity = prefs.opacity }
    @After fun restoreDisplay() { DisplayPreferences(context, "acceptance-display").apply { fontSize = savedFont; opacity = savedOpacity } }
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun views(activity: MainActivity): List<View> {
        assertEquals("dev.aiterminal.app", activity.packageName)
        assertTrue("Target lost foreground focus; stop UI actions", activity.hasWindowFocus())
        return all(activity.window.decorView)
    }
    private class Scenario(private val activity: MainActivity) : java.io.Closeable {
        fun onActivity(action: (MainActivity) -> Unit) { InstrumentationRegistry.getInstrumentation().runOnMainSync { action(activity) } }
        override fun close() { onActivity { it.finish() }; InstrumentationRegistry.getInstrumentation().waitForIdleSync() }
    }
    private fun launch(fixture: Boolean = true): Scenario {
        instrumentation.uiAutomation.executeShellCommand("am start -W -n dev.aiterminal.app/.MainActivity --ez isolated_ui true --ez render_fixture $fixture")
            .use { fd -> java.io.FileInputStream(fd.fileDescriptor).use { it.readBytes() } }
        var activity: MainActivity? = null
        waitFor {
            instrumentation.runOnMainSync {
                activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED)
                    .filterIsInstance<MainActivity>().firstOrNull { it.hasWindowFocus() }
            }
            activity != null
        }
        return Scenario(activity!!)
    }
    private fun chatBody(activity: MainActivity) = MainActivity::class.java.getDeclaredMethod("panel", String::class.java, Boolean::class.javaPrimitiveType)
        .apply { isAccessible = true }.invoke(activity, "AI Agent", false) as LinearLayout
    private fun waitFor(condition: () -> Boolean) {
        val until = SystemClock.elapsedRealtime() + 10000
        while (SystemClock.elapsedRealtime() < until) { if (condition()) return; Thread.sleep(30) }
        fail("UI condition timed out")
    }
    private fun screenshot(name: String) {
        instrumentation.waitForIdleSync()
        // Window rotation is composed outside the application's main-thread idle queue.
        Thread.sleep(400)
        instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
            File(context.filesDir, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
        }
    }

    @Test fun loginValidationPasswordVisibilityAndSmallScreenLayout() {
        launch(false).use { scenario ->
            screenshot("workspace-login-initial")
            scenario.onActivity { activity ->
                val fields = views(activity).filterIsInstance<EditText>().filter { it.isShown }
                assertEquals(3, fields.size)
                assertTrue(fields.all { it.text.isEmpty() })
                views(activity).filterIsInstance<Button>().first { it.text == "登录" }.performClick()
                assertTrue(views(activity).filterIsInstance<TextView>().any { it.text.toString().contains("服务地址") })
                val password = fields.first { it.hint == "密码" }; password.setText("private-test-draft")
                assertTrue(password.transformationMethod is android.text.method.PasswordTransformationMethod)
                val toggle = views(activity).first { it.contentDescription == "显示密码" }; assertTrue(toggle.performClick())
                assertEquals("隐藏密码", toggle.contentDescription); assertNull(password.transformationMethod)
                toggle.performClick(); assertNotNull(password.transformationMethod); password.setText("")
            }
            screenshot("workspace-login")
        }
    }

    @Test fun displayPanelPersistsResetsAndSurvivesRotation() {
        launch().use { scenario ->
            screenshot("workspace-terminal")
            scenario.onActivity { activity ->
                views(activity).first { it.contentDescription == "终端设置" }.performClick()
                val sliders = views(activity).filterIsInstance<SeekBar>()
                assertEquals(2, sliders.size)
                sliders[0].progress = 12; sliders[1].progress = 36
                assertEquals(24, DisplayPreferences(activity, "acceptance-display").fontSize); assertEquals(96, DisplayPreferences(activity, "acceptance-display").opacity)
                views(activity).filterIsInstance<Button>().first { it.text == "恢复默认" }.performClick()
                assertEquals(16, DisplayPreferences(activity, "acceptance-display").fontSize); assertEquals(88, DisplayPreferences(activity, "acceptance-display").opacity)
            }
            screenshot("workspace-settings")
            scenario.onActivity { it.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE }
            waitFor { var landscape = false; scenario.onActivity { landscape = it.window.decorView.width > it.window.decorView.height }; landscape }
            screenshot("workspace-settings-landscape")
            scenario.onActivity { activity ->
                views(activity).first { it.contentDescription == "关闭终端设置" }.performClick()
                views(activity).first { it.contentDescription == "打开工作空间" }.performClick()
            }
            screenshot("workspace-drawer-landscape")
            scenario.onActivity { it.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT }
            waitFor { var portrait = false; scenario.onActivity { portrait = it.window.decorView.width < it.window.decorView.height }; portrait }
            screenshot("workspace-drawer")
            scenario.onActivity { activity ->
                views(activity).first { it.contentDescription == "关闭工作空间" }.performClick()
                views(activity).first { it.contentDescription == "AI 对话" }.performClick()
            }
            screenshot("workspace-chat-unavailable")
        }
    }

    @Test fun closedPanelCannotOverwriteReopenedCompletedRequest() {
        val username = "ui-test-" + UUID.randomUUID()
        val storage = ChatStore(context, "https://ui.invalid", username)
        val sendStarted = CountDownLatch(1); val releaseSend = CountDownLatch(1)
        var oldPanel: AssistantPanel? = null; var newPanel: AssistantPanel? = null
        try {
            launch().use { scenario ->
                scenario.onActivity { activity ->
                    val body = chatBody(activity)
                    oldPanel = AssistantPanel(activity, body, storage.get("device", "session", "test-session"), storage, true, { true }, { payload ->
                        val request = JSONObject(payload)
                        if (request.getString("action") == "send") {
                            sendStarted.countDown(); releaseSend.await(10, TimeUnit.SECONDS)
                            JSONObject().put("available", true).put("state", "running").put("request_id", request.getString("request_id")).toString()
                        } else "{\"available\":true,\"state\":\"idle\"}"
                    }, {}, {})
                }
                waitFor { var enabled = false; scenario.onActivity { activity -> enabled = views(activity).filterIsInstance<ImageButton>().any { it.contentDescription == "发送给 AI" && it.isEnabled } }; enabled }
                scenario.onActivity { activity ->
                    views(activity).filterIsInstance<EditText>().first { it.hint == "发送给 AI" }.setText("question")
                    views(activity).filterIsInstance<ImageButton>().first { it.contentDescription == "发送给 AI" }.performClick()
                }
                assertTrue(sendStarted.await(5, TimeUnit.SECONDS))
                scenario.onActivity { activity ->
                    oldPanel!!.close()
                    val body = chatBody(activity)
                    val chat = storage.get("device", "session", "")
                    assertEquals("running", chat.state); assertTrue(chat.requestId.isNotBlank())
                    newPanel = AssistantPanel(activity, body, chat, storage, true, { true }, { payload ->
                        val request = JSONObject(payload)
                        assertEquals("poll", request.getString("action"))
                        JSONObject().put("available", true).put("state", "completed").put("request_id", request.getString("request_id")).put("reply", "verified reply").toString()
                    }, {}, {})
                }
                waitFor { storage.get("device", "session", "").state == "completed" }
                releaseSend.countDown(); instrumentation.waitForIdleSync(); Thread.sleep(100)
                val result = storage.get("device", "session", "")
                assertEquals("completed", result.state)
                assertEquals(1, result.messages.count { it.role == "assistant" })
                scenario.onActivity { activity ->
                    assertTrue(views(activity).filterIsInstance<CheckBox>().first { it.text == "监控当前终端" }.isChecked)
                    assertTrue(views(activity).filterIsInstance<CheckBox>().first { it.text == "允许操作" }.isChecked)
                }
                screenshot("workspace-chat")
                scenario.onActivity { newPanel!!.close() }
            }
        } finally { releaseSend.countDown(); instrumentation.runOnMainSync { oldPanel?.close(); newPanel?.close() }; storage.clear() }
    }

    @Test fun terminalReceivesCommittedImeTextAndSpecialKeysWithoutAnInputBox() {
        launch().use { scenario ->
            scenario.onActivity { activity ->
                val terminal = views(activity).filterIsInstance<TerminalView>().first()
                assertFalse(terminal.focusKeyboard())
                val text = mutableListOf<String>(); val keys = mutableListOf<String>()
                terminal.canType = { true }
                terminal.sendText = { value -> text.add(value); true }
                terminal.sendKey = { value -> keys.add(value); true }
                assertTrue(terminal.focusKeyboard())
                val connection = terminal.onCreateInputConnection(android.view.inputmethod.EditorInfo())
                connection.setComposingText("zhongwen", 1)
                assertTrue(text.isEmpty())
                connection.commitText("中文🙂", 1)
                connection.sendKeyEvent(android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_TAB))
                connection.sendKeyEvent(android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_ENTER))
                connection.deleteSurroundingText(1, 0)
                val pasted = "x".repeat(512)
                connection.commitText(pasted, 1)
                assertEquals(listOf("中文🙂", pasted), text)
                assertEquals(listOf("tab", "enter", "backspace"), keys)
                assertFalse(views(activity).filterIsInstance<EditText>().any { it.hint == "输入文字" })
                assertEquals(9, views(activity).filterIsInstance<Button>().count { it.text in listOf("回车", "Tab", "退格", "Ctrl-C", "Esc", "↑", "↓", "←", "→") })
            }
            screenshot("workspace-terminal-keyboard")
            scenario.onActivity { activity ->
                views(activity).first { it.contentDescription == "隐藏终端键盘" }.performClick()
                assertFalse(views(activity).filterIsInstance<TerminalView>().first().hasFocus())
            }
        }
    }

    @Test fun hardwareReturnAndTabReachTerminalWhenHistoryButtonHasFocus() {
        launch().use { scenario ->
            scenario.onActivity { activity ->
                val terminal = views(activity).filterIsInstance<TerminalView>().first()
                val keys = mutableListOf<String>()
                val text = mutableListOf<String>()
                terminal.canType = { true }
                terminal.sendKey = { keys.add(it); true }
                terminal.sendText = { text.add(it); true }
                assertTrue(terminal.focusKeyboard())
                val selected = MainActivity::class.java.getDeclaredField("selected").apply { isAccessible = true }
                selected.set(activity, "fixture-session")
                val history = views(activity).filterIsInstance<Button>().first { it.text == "历史" }
                assertTrue(history.requestFocus())
                assertTrue(history.hasFocus())
                for (code in listOf(KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_TAB, KeyEvent.KEYCODE_A)) {
                    assertTrue(activity.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, code)))
                    assertTrue(activity.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_UP, code)))
                }
                assertEquals(listOf("enter", "tab"), keys)
                assertEquals(listOf("a"), text)
                assertTrue(history.hasFocus())
                assertNull(MainActivity::class.java.getDeclaredField("overlay").apply { isAccessible = true }.get(activity))
            }
        }
    }

    @Test fun chatComposerResizesForNativeKeyboard() {
        val storage = ChatStore(context, "https://keyboard.invalid", UUID.randomUUID().toString())
        var panel: AssistantPanel? = null
        try {
            launch().use { scenario ->
                scenario.onActivity { activity ->
                    panel = AssistantPanel(activity, chatBody(activity), storage.get("device", "session", "keyboard-test"), storage, true, { true },
                        { "{\"available\":false,\"state\":\"unavailable\",\"message\":\"Desktop 尚未配置 AI\"}" }, {}, {})
                    val input = views(activity).filterIsInstance<EditText>().first { it.hint == "发送给 AI" }
                    input.setText("只保留在草稿中的文字"); input.requestFocus()
                    (activity.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager).showSoftInput(input, 0)
                }
                screenshot("workspace-chat-keyboard")
                scenario.onActivity { activity ->
                    val send = views(activity).filterIsInstance<ImageButton>().first { it.contentDescription == "发送给 AI" }
                    assertFalse(send.isEnabled)
                    val parent = send.parent.parent.parent as ScrollView
                    parent.fullScroll(View.FOCUS_DOWN)
                }
                screenshot("workspace-chat-keyboard-actions")
                scenario.onActivity { panel!!.close() }
            }
        } finally { instrumentation.runOnMainSync { panel?.close() }; storage.clear() }
    }

    @Test fun closingStatusDistinguishesUnknownFromConfirmedEvenWithStaleList() {
        val memory = WorkspaceMemory(context, "https://status.invalid", UUID.randomUUID().toString())
        try {
            launch().use { scenario -> scenario.onActivity { activity ->
                fun set(name: String, value: Any) { MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.set(activity, value) }
                set("memory", memory); set("connected", true); set("deviceId", "device")
                set("sessions", listOf(uniffi.ai_terminal_mobile.RemoteSession("session", "/test", false, 0u, true)))
                memory.record("device", mapOf("session" to false))
                val status = MainActivity::class.java.getDeclaredMethod("sessionAvailability", String::class.java, String::class.java).apply { isAccessible = true }
                assertEquals("在线", status.invoke(activity, "device", "session"))
                @Suppress("UNCHECKED_CAST") val uncertain = MainActivity::class.java.getDeclaredField("uncertainSessions").apply { isAccessible = true }.get(activity) as MutableSet<Pair<String, String>>
                uncertain.add("device" to "session")
                assertEquals("待确认", status.invoke(activity, "device", "session"))
                memory.record("device", emptyMap())
                assertEquals("已关闭", status.invoke(activity, "device", "session"))
                assertEquals("待确认", status.invoke(activity, "unqueried-device", "session"))
            } }
        } finally { memory.clear() }
    }

    @Test fun accountPanelShowsOnlineDevicesWithoutStaleOfflineRows() {
        launch(false).use { scenario -> scenario.onActivity { activity ->
            fun set(name: String, value: Any) { MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.set(activity, value) }
            set("accountName", "fixture")
            set("devices", listOf(
                uniffi.ai_terminal_mobile.AccountDevice("online", "Online Desktop", "desktop", true, false),
                uniffi.ai_terminal_mobile.AccountDevice("offline", "Old iPhone", "ios", false, false)
            ))
            MainActivity::class.java.getDeclaredMethod("accountPanel").apply { isAccessible = true }.invoke(activity)
            val labels = all(activity.window.decorView).filterIsInstance<TextView>().filter { it.isShown }.map { it.text.toString() }
            assertTrue(labels.contains("Online Desktop"))
            assertFalse(labels.contains("Old iPhone"))
            assertFalse(labels.contains("离线"))
        } }
    }
}
