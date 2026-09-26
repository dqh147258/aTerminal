package com.yxf.aterminal

import android.graphics.Bitmap
import android.graphics.Paint
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.view.MotionEvent
import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.inputmethod.EditorInfo
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
        assertEquals("com.yxf.aterminal", activity.packageName)
        assertTrue("Lost foreground; stop input", main { activity.hasWindowFocus() } || instrumentation.uiAutomation.rootInActiveWindow?.packageName == "com.yxf.aterminal")
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
    private fun click(label: String) {
        if (label in setOf("回车", "Tab", "退格", "Ctrl-C", "Esc")) { icon("特殊按键"); icon(label) }
        else mutate { views().filterIsInstance<Button>().first { it.text.toString() == label && it.isEnabled }.performClick() }
    }
    private fun icon(description: String) = mutate { views().first { it.contentDescription?.toString() == description && it.isEnabled && it !is EditText }.performClick() }
    private fun field(hint: String) = views().filterIsInstance<EditText>().first { it.hint?.toString() == hint }
    private fun launch() {
        instrumentation.uiAutomation.executeShellCommand("am start -W -n com.yxf.aterminal/.MainActivity --ez terminal_input_test true")
            .use { fd -> java.io.FileInputStream(fd.fileDescriptor).use { it.readBytes() } }
        val deadline = SystemClock.elapsedRealtime() + 10000
        while (SystemClock.elapsedRealtime() < deadline) {
            val current = main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull { it.hasWindowFocus() } }
            if (current != null) { activity = current; return }; Thread.sleep(40)
        }
        fail("aTerminal did not enter foreground")
    }
    private fun frame(): RenderFrame? = (get("terminal") as? TerminalView)?.let {
        TerminalView::class.java.getDeclaredField("frame").apply { isAccessible = true }.get(it) as RenderFrame
    }
    private fun lines() = frame()?.let { f -> f.cells.chunked(f.cols.toInt()).map { line -> line.filter { it.width > 0u }.joinToString("") { it.text }.trim() } }.orEmpty()
    private fun readyInput() {
        waitFor("input availability") { get("controlled") == true }
        mutate { assertTrue((get("terminal") as TerminalView).focusKeyboard()) }
    }
    private fun type(text: String) {
        readyInput()
        mutate {
            val terminal = get("terminal") as TerminalView
            val connection = terminal.onCreateInputConnection(EditorInfo())
            assertTrue(connection.commitText(text, 1))
        }
    }
    private fun input(text: String) {
        type(text)
        mutate {
            val connection = (get("terminal") as TerminalView).onCreateInputConnection(EditorInfo())
            assertTrue(connection.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER)))
        }
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
        assertEquals("com.yxf.aterminal", root.packageName)
        root.findAccessibilityNodeInfosByText("退出").last { it.isClickable }.performAction(AccessibilityNodeInfo.ACTION_CLICK)
        waitFor("test account logout") { (get("loginBox") as View).isShown }
    }
    @Test fun realTerminalWorkflowAndPlaceholder() {
        val fixture = JSONObject(File(context.filesDir, "followup-fixture.json").readText())
        val preferences = DisplayPreferences(context, "terminal-input-display"); val originalFont = preferences.fontSize; val originalOpacity = preferences.opacity
        launch()
        try {
            waitFor("test login or restored temporary account") { (get("loginBox") as View).isShown || (get("accountName") as String).isNotEmpty() }
            if (main { (get("accountName") as String).isNotEmpty() }) logout()
            click("修改服务器地址")
            mutate {
                field("服务器 https://…").setText(fixture.getString("server")); field("账号").setText(fixture.getString("username")); field("密码").setText(fixture.getString("password"))
            }
            click("保存服务器地址")
            click("登录")
            waitFor("real account login") { get("accountName") == fixture.getString("username") }
            icon("账号与设备")
            val desktop = fixture.optString("desktop_name", "Local Desktop")
            waitFor("online fixture Desktop") { views().any { it.contentDescription?.toString() == "连接 $desktop" && it.isEnabled } }
            icon("连接 $desktop")
            waitFor("PTY session") { get("selected") != null }
            val target = fixture.getString("session")
            if (main { get("selected") != target }) {
                icon("打开工作空间")
                mutate {
                    views().filterIsInstance<Button>().first { it.tag == target }.performClick()
                }
                waitFor("fixture selected") { get("selected") == target }
            }
            readyInput(); click("Ctrl-C")
            input("printf 'ANDROID_FOLLOWUP_PTY_OK\\n'")
            waitFor("real PTY output") { lines().any { it == "ANDROID_FOLLOWUP_PTY_OK" } }
            result.put("real_login_and_pty", true)
            input("git status")
            input("printf 'GIT_STATUS_COMPLETED\\n'")
            waitFor("git status keeps the live PTY usable") { lines().any { it == "GIT_STATUS_COMPLETED" } }
            result.put("git_status_survived", true)
            // Check the authoritative cell backgrounds, not just the cursor shape: Zsh
            // highlights each bracketed paste even when the mobile cursor is a thin beam.
            input("printf '\\033[2J\\033[H'")
            waitFor("cleared typing screen") { lines().none { it == "GIT_STATUS_COMPLETED" } }
            var typed = ""
            for (character in "git stat") {
                type(character.toString()); typed += character
                val expected = typed
                waitFor("character echoed: $expected") { lines().any { it.endsWith(expected.trimEnd()) } }
                waitFor("typed character has no paste highlight") {
                    val f = frame()!!
                    val row = f.cells.subList((f.cursorRow * f.cols).toInt(), ((f.cursorRow + 1u) * f.cols).toInt())
                    val start = f.cursorCol.toInt() - expected.length
                    start >= 0 && row.subList(start, f.cursorCol.toInt()).joinToString("") { it.text } == expected &&
                        row.subList(start, f.cursorCol.toInt()).all { it.background == row.last().background }
                }
            }
            screenshot("typing-no-highlight")
            result.put("per_character_typing_no_highlight", true)
            mutate { assertTrue(activity.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DEL))) }
            waitFor("backspace edits typed text") { lines().any { it.endsWith("git sta") } }
            type("\t")
            waitFor("IME Tab shows git completion candidates") { lines().any { it.contains("stash") } && lines().any { it.contains("status") } }
            screenshot("ime-tab-completion")
            result.put("ime_tab_git_completion", true)
            click("Ctrl-C")
            type("中文🙂")
            waitFor("Unicode committed text reaches PTY") { lines().any { it.contains("中文🙂") } }
            click("Ctrl-C")
            if (fixture.optBoolean("check_desktop_detach")) {
                File(context.filesDir, "desktop-detach-ready").writeText(target)
                waitFor("Desktop detach makes Mobile read-only") { get("controlled") == false && !(get("remote") as RemoteTerminal).desktopAttached() }
                try { (get("remote") as RemoteTerminal).sendText("must not run", true); fail("Detached Desktop accepted Mobile input") }
                catch (_: Exception) {}
                (get("remote") as RemoteTerminal).readHistory()
                result.put("desktop_detach_read_only", true)
                File(context.filesDir, "desktop-detached").writeText(target)
                waitFor("Desktop attach restores Mobile input") { get("controlled") == true && (get("remote") as RemoteTerminal).desktopAttached() }
                result.put("desktop_reattach_restored_input", true)
            }
            val desktopMarker = fixture.optString("desktop_marker")
            if (desktopMarker.isNotEmpty()) {
                File(context.filesDir, "shared-input-ready").writeText(target)
                waitFor("Desktop remains writable while Mobile is connected", 30) { lines().any { it == desktopMarker } }
                result.put("desktop_input_while_mobile_open", true)
            }
            val tabDirectory = "/private/tmp/t" + target.take(8)
            input("mkdir -p $tabDirectory")
            input("echo ANDROID_TAB_OK > $tabDirectory/complete_marker")
            input("echo ANDROID_TAB_READY")
            waitFor("shell finished preparing Tab target") { lines().any { it == "ANDROID_TAB_READY" } }
            type("cat $tabDirectory/comple")
            mutate {
                assertTrue(activity.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_TAB)))
                assertTrue(activity.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_TAB)))
            }
            waitFor("hardware Tab expands the path") { lines().any { it.contains("cat $tabDirectory/complete_marker") } }
            click("回车")
            waitFor("Tab completes a real shell path") { lines().any { it == "ANDROID_TAB_OK" } }
            input("rm -r $tabDirectory")
            result.put("real_tab_completion", true)
            if (main { get("keyboardOpen") == true }) { icon("特殊按键"); icon("收起系统键盘") }
            val columns = main { frame()!!.cols }
            mutate { val surface = get("surface") as HorizontalScrollView; surface.scrollTo(surface.getChildAt(0).width, 0); assertTrue(surface.scrollX > 0); surface.scrollTo(0, 0) }
            icon("终端设置")
            mutate {
                val overlay = get("overlayPanel") as View; val root = get("root") as View
                assertTrue(overlay.height < root.height)
                views().filterIsInstance<SeekBar>().first { it.contentDescription == "文字大小" }.progress = 18
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
            if (main { get("keyboardOpen") == true }) { icon("特殊按键"); icon("收起系统键盘") }; icon("打开工作空间")
            mutate { views().first { it.tag == "close-$target" && it.isEnabled }.performClick() }
            instrumentation.waitForIdleSync(); focus()
            val closeDialog = instrumentation.uiAutomation.rootInActiveWindow
            assertEquals("com.yxf.aterminal", closeDialog.packageName)
            closeDialog.findAccessibilityNodeInfosByText("关闭会话").last { it.isClickable }.performAction(AccessibilityNodeInfo.ACTION_CLICK)
            waitFor("dedicated PTY closed") {
                @Suppress("UNCHECKED_CAST") val sessions = get("sessions") as List<RemoteSession>
                sessions.none { it.id == target && !it.exited } && get("overlay") != null && get("sessionBusy") == false
            }
            Thread.sleep(350)
            val remaining = (main { get("remote") } as RemoteTerminal).sessions()
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
                result.put("screen_tail", org.json.JSONArray(lines().filter { it.isNotBlank() }.takeLast(8)))
                @Suppress("UNCHECKED_CAST") val sessions = get("sessions") as List<RemoteSession>
                result.put("remaining_sessions", org.json.JSONArray(sessions.map { JSONObject().put("id", it.id).put("exited", it.exited) }))
            }
            File(context.filesDir, "followup-results.json").writeText(result.toString(2))
            main { activity.finish() }
        }
    }
}
