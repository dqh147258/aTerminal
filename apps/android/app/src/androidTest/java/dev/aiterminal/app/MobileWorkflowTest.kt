package dev.aiterminal.app

import android.graphics.Bitmap
import android.graphics.Paint
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.view.MotionEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.*
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.ai_terminal_mobile.*
import java.io.File

/** Runs only against the coordinator's disposable real account/PTY/model fixture. */
class MobileWorkflowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private val result = JSONObject()
    private fun <T> main(action: () -> T): T {
        var answer: T? = null; var failure: Throwable? = null
        instrumentation.runOnMainSync { try { answer = action() } catch (e: Throwable) { failure = e } }
        failure?.let { throw it }; @Suppress("UNCHECKED_CAST") return answer as T
    }
    private fun focus() {
        assertEquals("dev.aiterminal.app", activity.packageName)
        assertTrue("Lost foreground; stop input", main { activity.hasWindowFocus() } || instrumentation.uiAutomation.rootInActiveWindow?.packageName == "dev.aiterminal.app")
    }
    private fun mutate(action: () -> Unit) { focus(); main(action) }
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun get(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun waitFor(label: String, seconds: Int = 30, predicate: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + seconds * 1000
        while (SystemClock.elapsedRealtime() < deadline) { if (main(predicate)) return; Thread.sleep(40) }
        fail("Timed out: $label")
    }
    private fun click(label: String) = mutate { views().filterIsInstance<Button>().first { it.text.toString() == label && it.isEnabled }.performClick() }
    private fun icon(description: String) = mutate { views().first { it.contentDescription?.toString() == description && it.isEnabled && it !is EditText }.performClick() }
    private fun field(hint: String) = views().filterIsInstance<EditText>().first { it.hint?.toString() == hint }
    private fun launch() {
        instrumentation.uiAutomation.executeShellCommand("am start -W -n dev.aiterminal.app/.MainActivity --ez acceptance_test true")
            .use { fd -> java.io.FileInputStream(fd.fileDescriptor).use { it.readBytes() } }
        val deadline = SystemClock.elapsedRealtime() + 10000
        while (SystemClock.elapsedRealtime() < deadline) {
            val current = main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull { it.hasWindowFocus() } }
            if (current != null) { activity = current; return }; Thread.sleep(40)
        }
        fail("AI Terminal did not enter foreground")
    }
    private fun frame(): RenderFrame? = (get("terminal") as? TerminalView)?.let {
        TerminalView::class.java.getDeclaredField("frame").apply { isAccessible = true }.get(it) as RenderFrame
    }
    private fun lines() = frame()?.let { f -> f.cells.chunked(f.cols.toInt()).map { line -> line.joinToString("") { it.text }.trim() } }.orEmpty()
    private fun input(text: String) {
        if (!main { (get("inputBox") as View).isShown }) icon("显示或隐藏终端输入")
        if (!main { get("controlled") == true }) {
            mutate { (get("takeControl") as CheckBox).performClick() }; waitFor("input ownership") { get("controlled") == true }
        }
        mutate { field("输入文字").setText(text) }; click("发送"); click("回车")
    }
    private fun screenshot(name: String) {
        focus(); instrumentation.waitForIdleSync(); Thread.sleep(400); focus()
        instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
            File(context.filesDir, "followup-$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
        }
    }
    private fun logout() {
        icon("账号与设备"); click("退出登录"); instrumentation.waitForIdleSync(); focus()
        val root = instrumentation.uiAutomation.rootInActiveWindow
        assertEquals("dev.aiterminal.app", root.packageName)
        root.findAccessibilityNodeInfosByText("退出").last { it.isClickable }.performAction(AccessibilityNodeInfo.ACTION_CLICK)
        waitFor("test account logout") { (get("loginBox") as View).isShown }
    }
    @Test fun realTerminalWorkflowAndPlaceholder() {
        val fixture = JSONObject(File(context.filesDir, "followup-fixture.json").readText())
        val preferences = DisplayPreferences(context, "acceptance-display"); val originalFont = preferences.fontSize; val originalOpacity = preferences.opacity
        launch()
        try {
            waitFor("test login or restored temporary account") { (get("loginBox") as View).isShown || (get("accountName") as String).isNotEmpty() }
            if (main { (get("accountName") as String).isNotEmpty() }) logout()
            mutate {
                field("服务器 https://…").setText(fixture.getString("server")); field("账号").setText(fixture.getString("username")); field("密码").setText(fixture.getString("password"))
            }
            click("登录")
            waitFor("real account login") { get("accountName") == fixture.getString("username") }
            icon("账号与设备")
            waitFor("online fixture Desktop") { views().any { it.contentDescription?.toString() == "连接 Local Desktop" && it.isEnabled } }
            icon("连接 Local Desktop")
            waitFor("PTY session") { get("selected") != null }
            val target = fixture.getString("session")
            if (main { get("selected") != target }) {
                icon("打开工作空间")
                mutate {
                    views().filterIsInstance<Button>().first { it.tag == target }.performClick()
                }
                waitFor("fixture selected") { get("selected") == target }
            }
            input("printf 'ANDROID_FOLLOWUP_PTY_OK\\n'")
            waitFor("real PTY output") { lines().any { it == "ANDROID_FOLLOWUP_PTY_OK" } }
            result.put("real_login_and_pty", true)
            if (main { (get("inputBox") as View).isShown }) icon("隐藏终端输入")
            val columns = main { frame()!!.cols }
            mutate { val surface = get("surface") as HorizontalScrollView; surface.scrollTo(surface.getChildAt(0).width, 0); assertTrue(surface.scrollX > 0); surface.scrollTo(0, 0) }
            icon("终端设置")
            mutate {
                val overlay = get("overlayPanel") as View; val root = get("root") as View
                assertTrue(overlay.height < root.height)
                views().filterIsInstance<SeekBar>().first { it.contentDescription == "文字大小" }.progress = 12
            }
            waitFor("live terminal font") {
                val terminal = get("terminal") as TerminalView
                val paint = TerminalView::class.java.getDeclaredField("paint").apply { isAccessible = true }.get(terminal) as Paint
                paint.textSize >= 24 * activity.resources.displayMetrics.scaledDensity - .1f
            }
            assertEquals(columns, main { frame()!!.cols }); screenshot("settings-live-pty")
            click("恢复默认"); icon("关闭终端设置")
            result.put("original_columns_horizontal_scroll", columns.toInt()).put("partial_overlay_live_font", true)
            val point = main { val root = get("root") as View; val position = IntArray(2); root.getLocationOnScreen(position); Triple(position[0] + activity.dp(30), position[1] + activity.dp(140), activity.dp(150)) }
            val down = SystemClock.uptimeMillis()
            for ((action, x) in listOf(MotionEvent.ACTION_DOWN to point.first, MotionEvent.ACTION_MOVE to point.first + point.third, MotionEvent.ACTION_UP to point.first + point.third)) {
                focus(); val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, x.toFloat(), point.second.toFloat(), 0)
                instrumentation.sendPointerSync(event); event.recycle()
            }
            waitFor("edge gesture opens workspace") { (get("overlay") as? View)?.tag == "drawer" }
            result.put("edge_swipe_drawer", true); icon("关闭工作空间")

            icon("AI 对话")
            waitFor("AI placeholder") { views().filterIsInstance<TextView>().any { it.text.toString() == "AI 助手即将开放" } }
            assertTrue(main { !field("AI 对话暂未开放").isEnabled })
            assertFalse(main { views().any { it.contentDescription == "语音输入" || it.contentDescription == "发送给 AI" } })
            screenshot("assistant-placeholder"); icon("关闭AI Agent")
            result.put("assistant_placeholder_no_actions", true)

            focus()
            instrumentation.uiAutomation.executeShellCommand("input keyevent KEYCODE_HOME").use { fd -> java.io.FileInputStream(fd.fileDescriptor).use { it.readBytes() } }
            waitFor("background disconnect") { get("active") == false }
            launch()
            waitFor("automatic last-session restore") { get("selected") == target }
            assertEquals(columns, main { frame()!!.cols }); screenshot("restored-terminal")
            result.put("automatic_last_session_restore", true)
            input("sleep 30")
            waitFor("sleep command reaches real PTY") { lines().any { it.endsWith("sleep 30") } }
            click("Ctrl-C"); input("printf 'ANDROID_FOLLOWUP_CTRL_C_OK\\n'")
            waitFor("shell accepts input after Ctrl-C") { lines().any { it == "ANDROID_FOLLOWUP_CTRL_C_OK" } }
            result.put("real_ctrl_c_recovery", true)
            result.put("connection_path", main { (get("remote") as RemoteTerminal).connectionPath() })
            icon("隐藏终端输入"); icon("打开工作空间")
            mutate { views().first { it.tag == "close-$target" && it.isEnabled }.performClick() }
            instrumentation.waitForIdleSync(); focus()
            val closeDialog = instrumentation.uiAutomation.rootInActiveWindow
            assertEquals("dev.aiterminal.app", closeDialog.packageName)
            closeDialog.findAccessibilityNodeInfosByText("关闭会话").last { it.isClickable }.performAction(AccessibilityNodeInfo.ACTION_CLICK)
            waitFor("dedicated PTY closed") {
                @Suppress("UNCHECKED_CAST") val sessions = get("sessions") as List<RemoteSession>
                sessions.none { it.id == target && !it.exited } && get("overlay") != null && get("sessionBusy") == false
            }
            Thread.sleep(350)
            val remaining = (main { get("remote") } as RemoteTerminal).sessions()
            assertTrue("Close must preserve the channel for remaining sessions", remaining.isNotEmpty())
            assertTrue(remaining.none { it.id == target })
            result.put("channel_alive_after_close", true)
            val memory = main { get("memory") as WorkspaceMemory }
            assertTrue(memory.closed(main { get("deviceId") as String }, target))
            result.put("closed_terminal_status", true)
            screenshot("closed-terminal")
            icon("关闭工作空间")
            logout(); result.put("temporary_account_logout", true).put("passed", true)
        } finally {
            preferences.fontSize = originalFont; preferences.opacity = originalOpacity
            main {
                result.put("last_status", (get("status") as TextView).text.toString())
                result.put("session_busy", get("sessionBusy"))
                @Suppress("UNCHECKED_CAST") val sessions = get("sessions") as List<RemoteSession>
                result.put("remaining_sessions", org.json.JSONArray(sessions.map { JSONObject().put("id", it.id).put("exited", it.exited) }))
            }
            File(context.filesDir, "followup-results.json").writeText(result.toString(2))
            main { activity.finish() }
        }
    }
}
