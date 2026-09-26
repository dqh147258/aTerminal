package com.yxf.aterminal

import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.res.Configuration
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Paint
import android.graphics.drawable.GradientDrawable
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.*
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.ai_terminal_mobile.RenderFrame
import java.io.File

/** All input is fenced by the coordinator's disposable PTY marker and session ID. */
class MobileInputLayoutTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private fun <T> main(action: () -> T): T {
        var result: T? = null; var error: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (e: Throwable) { error = e } }
        error?.let { throw it }; @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun get(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun waitFor(label: String, predicate: () -> Boolean) {
        val until = SystemClock.elapsedRealtime() + 30000
        while (SystemClock.elapsedRealtime() < until) { if (main(predicate)) return; Thread.sleep(40) }
        fail("Timed out: $label; " + main { "availableHeight=${availableHeight()}, ime=${imeVisible()}, rootHeight=${(get("root") as View).height}" })
    }
    private fun button(label: String) = main { views().filterIsInstance<Button>().last { it.text.toString() == label && it.isEnabled }.performClick() }
    private fun icon(label: String) = main { views().last { it.contentDescription?.toString() == label && it.isEnabled }.performClick() }
    private fun availableHeight(): Int { val root = get("root") as View; return root.height - root.paddingTop - root.paddingBottom }
    private fun imeVisible(): Boolean = if (android.os.Build.VERSION.SDK_INT >= 30) activity.window.decorView.rootWindowInsets.isVisible(android.view.WindowInsets.Type.ime()) else get("keyboardOpen") == true
    private fun lines(): List<String> {
        val terminal = get("terminal") as? TerminalView ?: return emptyList()
        val frame = TerminalView::class.java.getDeclaredField("frame").apply { isAccessible = true }.get(terminal) as RenderFrame
        return frame.cells.chunked(frame.cols.toInt()).map { row -> row.filter { it.width > 0u }.joinToString("") { it.text }.trim() }
    }
    private fun special(label: String) {
        icon("特殊按键")
        main {
            val panel = get("overlayPanel") as View
            assertTrue(panel.height < (get("root") as View).height)
            val key = views().filterIsInstance<ImageButton>().first { it.contentDescription == label }
            assertNotNull(key.drawable)
        }
        icon(label)
        assertNull("Special key closes after one action", main { get("overlay") })
    }
    private fun screenshot(name: String) {
        instrumentation.waitForIdleSync(); instrumentation.uiAutomation.waitForIdle(150, 3000)
        Thread.sleep(450) // Wait for the system compositor's rotation animation before visual evidence.
        instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
            File(context.filesDir, "autoconnect-$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
        }
    }
    @Test fun directInputFloatingKeysLandscapeAndFullAgent() {
        val fixture = JSONObject(File(context.filesDir, "autoconnect-fixture.json").readText())
        val prefs = DisplayPreferences(context, "acceptance-display")
        val fontBefore = prefs.fontSize; val opacityBefore = prefs.opacity
        val mainAccount = context.getSharedPreferences("account", 0).all
        val mainConnection = context.getSharedPreferences("connection", 0).all
        PairingStore(context, "acceptance-account").clear()
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("acceptance_test", true))
        val deadline = SystemClock.elapsedRealtime() + 10000
        while (SystemClock.elapsedRealtime() < deadline) {
            val current = main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull() }
            if (current != null) { activity = current; break }; Thread.sleep(40)
        }
        assertTrue(::activity.isInitialized)
        try {
            waitFor("login restored") { get("loginBusy") == false }
            button("修改服务器地址")
            main {
                fun field(hint: String) = views().filterIsInstance<EditText>().first { it.hint == hint }
                field("服务器 https://…").setText(fixture.getString("server"))
                field("账号").setText(fixture.getString("username"))
                field("密码").setText(fixture.getString("password"))
            }
            button("保存服务器地址"); button("登录")
            waitFor("isolated terminal marker") { get("selected") == fixture.getString("newest") && lines().contains("NEWEST_TERMINAL_OK") && get("entryPending") == false }
            assertNull(main { get("overlay") })
            instrumentation.waitForIdleSync(); Thread.sleep(400)
            assertFalse("Cold entry must not open the system keyboard", main { imeVisible() })
            // No keyboard button or terminal tap: hardware text is accepted immediately.
            instrumentation.sendStringSync("printf 'LAYOUT_DIRECT_INPUT_OK\\n'")
            special("回车")
            waitFor("direct keyboard reached PTY") { lines().contains("LAYOUT_DIRECT_INPUT_OK") }
            instrumentation.sendStringSync("sleep 30")
            special("回车"); special("Ctrl-C")
            instrumentation.sendStringSync("printf 'LAYOUT_CTRL_C_OK\\n'")
            special("回车")
            waitFor("Ctrl-C interrupted temporary shell command") { lines().contains("LAYOUT_CTRL_C_OK") }
            assertFalse("Hardware input and the key palette do not open IME", main { imeVisible() })
            main { (get("terminal") as TerminalView).performClick() }
            waitFor("tap Terminal opens system keyboard") { imeVisible() }
            special("Esc")
            assertTrue("Special keys preserve an already open IME", main { imeVisible() })
            icon("特殊按键"); screenshot("special-keys"); icon("关闭特殊按键")
            special("收起系统键盘")
            waitFor("explicit IME hide") { !imeVisible() }
            icon("终端设置")
            main {
                val font = views().filterIsInstance<SeekBar>().first { it.contentDescription == "文字大小" }
                val opacity = views().filterIsInstance<SeekBar>().first { it.contentDescription == "浮层不透明度" }
                assertEquals(18, font.max); assertEquals(100, opacity.max)
                font.progress = 0; opacity.progress = 0
                assertEquals(6, prefs.fontSize); assertEquals(0, prefs.opacity)
                assertEquals(0, Color.alpha(((get("overlayPanel") as View).background as GradientDrawable).color!!.defaultColor))
                opacity.progress = 100; assertEquals(100, prefs.opacity)
                opacity.progress = 25
                val paint = TerminalView::class.java.getDeclaredField("paint").apply { isAccessible = true }.get(get("terminal")) as Paint
                assertEquals(6f * activity.resources.displayMetrics.scaledDensity, paint.textSize, .1f)
            }
            icon("关闭终端设置")
            val session = main { get("selected") }; val generation = main { get("generation") }
            main { activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE }
            waitFor("landscape layout") { activity.resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE && (get("root") as View).width > (get("root") as View).height }
            main {
                assertFalse((get("workspaceHeader") as View).isShown)
                assertFalse((get("workspaceFooter") as View).isShown)
                assertTrue((get("sideWorkspace") as View).isShown); assertTrue((get("sideAccount") as View).isShown)
                if (android.os.Build.VERSION.SDK_INT >= 30) assertFalse(activity.window.decorView.rootWindowInsets.isVisible(android.view.WindowInsets.Type.statusBars()))
                else assertTrue(activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_FULLSCREEN != 0)
                assertEquals(session, get("selected")); assertEquals(generation, get("generation"))
            }
            screenshot("landscape")
            icon("打开工作空间")
            assertTrue(main { views().filterIsInstance<TextView>().any { it.text.toString().contains("UTF-8") } })
            icon("关闭工作空间")
            icon("AI 对话")
            waitFor("Agent fills landscape") { val panel = get("overlayPanel") as View; val root = get("root") as View; panel.width == root.width - root.paddingLeft - root.paddingRight && panel.height == root.height - root.paddingTop - root.paddingBottom }
            screenshot("agent-fullscreen-landscape")
            val beforeIme = main { availableHeight() }
            val draftPoint = main {
                val draft = views().filterIsInstance<EditText>().first { it.hint == "发送任务或追加消息" }
                val rect = android.graphics.Rect(); assertTrue(draft.getGlobalVisibleRect(rect))
                rect.centerX().toFloat() to rect.centerY().toFloat()
            }
            val tapTime = SystemClock.uptimeMillis()
            for (action in listOf(android.view.MotionEvent.ACTION_DOWN, android.view.MotionEvent.ACTION_UP)) {
                val event = android.view.MotionEvent.obtain(tapTime, SystemClock.uptimeMillis(), action, draftPoint.first, draftPoint.second, 0)
                instrumentation.sendPointerSync(event); event.recycle()
            }
            waitFor("landscape Agent resizes for IME") { imeVisible() && availableHeight() < beforeIme }
            main {
                val send = views().filterIsInstance<Button>().last { it.text == "发送 / 追加" }
                val rect = android.graphics.Rect()
                assertTrue("Agent send button remains visible above IME", send.getGlobalVisibleRect(rect) && rect.height() >= send.height - 2)
            }
            screenshot("agent-landscape-keyboard")
            instrumentation.sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_BACK)
            waitFor("IME closes") { !imeVisible() && availableHeight() == beforeIme }
            main { activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT }
            waitFor("Agent fills portrait") { val panel = get("overlayPanel") as View; val root = get("root") as View; root.height > root.width && panel.width == root.width - root.paddingLeft - root.paddingRight && panel.height == root.height - root.paddingTop - root.paddingBottom }
            screenshot("agent-fullscreen-portrait")
            icon("关闭AI Agent")
            assertTrue(main { (get("workspaceHeader") as View).isShown })
            val previous = activity
            main { activity.recreate() }
            waitFor("new Activity") {
                val current = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull()
                if (current != null && current !== previous) { activity = current; true } else false
            }
            waitFor("restored terminal and display preferences") { get("selected") == fixture.getString("newest") && get("entryPending") == false }
            assertEquals(6, DisplayPreferences(context, "acceptance-display").fontSize)
            assertEquals(25, DisplayPreferences(context, "acceptance-display").opacity)
            screenshot("font-six")
            assertEquals(mainAccount, context.getSharedPreferences("account", 0).all)
            assertEquals(mainConnection, context.getSharedPreferences("connection", 0).all)
            File(context.filesDir, "autoconnect-results.json").writeText(JSONObject().put("passed", true)
                .put("default_hardware_input", true).put("ime_only_after_terminal_tap", true).put("single_use_special_keys", true).put("ctrl_c", true)
                .put("font_min_sp", 6).put("opacity_range", "0-100").put("preferences_persisted", true)
                .put("rotation_preserves_session", true).put("landscape_sidebar", true).put("agent_fullscreen_both_orientations", true)
                .put("primary_preferences_unchanged", true).toString(2))
        } finally {
            prefs.fontSize = fontBefore; prefs.opacity = opacityBefore
            main { activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT; activity.finish() }
            PairingStore(context, "acceptance-account").clear()
        }
    }
}
