package com.yxf.aterminal

import android.content.Intent
import android.graphics.Bitmap
import android.os.SystemClock
import android.view.View
import android.widget.HorizontalScrollView
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.ai_terminal_mobile.*
import java.io.File

/** Real swipes against a production Activity and isolated encrypted PTY, without user credentials. */
class TerminalScrollTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private val gestures = mutableListOf<Int>()
    private fun <T> main(action: () -> T): T {
        if (android.os.Looper.myLooper() == android.os.Looper.getMainLooper()) return action()
        var value: T? = null; var failure: Throwable? = null
        instrumentation.runOnMainSync { try { value = action() } catch (e: Throwable) { failure = e } }
        failure?.let { throw it }; @Suppress("UNCHECKED_CAST") return value as T
    }
    private fun get(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun set(name: String, value: Any?) { MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.set(activity, value) }
    private fun waitFor(label: String, test: () -> Boolean) {
        val until = SystemClock.elapsedRealtime() + 15000
        while (SystemClock.elapsedRealtime() < until) { if (test()) return; Thread.sleep(40) }
        fail("Timed out: $label")
    }
    private fun terminal() = get("terminal") as TerminalView
    private fun displayed(): RenderFrame = main {
        val view = terminal()
        (TerminalView::class.java.getDeclaredField("historyFrame").apply { isAccessible = true }.get(view)
            ?: TerminalView::class.java.getDeclaredField("frame").apply { isAccessible = true }.get(view)) as RenderFrame
    }
    private fun text() = displayed().cells.filter { it.width > 0u }.joinToString("") { it.text }
    private fun swipe(older: Boolean, horizontal: Boolean = false) {
        val bounds = main {
            assertTrue(activity.hasWindowFocus())
            val view = (get("surface") as View).parent as View
            val location = IntArray(2); view.getLocationOnScreen(location)
            intArrayOf(location[0], location[1], view.width, view.height)
        }
        val x = bounds[0] + bounds[2] / 3
        val top = bounds[1] + bounds[3] / 5
        val bottom = bounds[1] + bounds[3] * 4 / 5
        val command = if (horizontal) "input swipe ${bounds[2] * 3 / 4} $top ${bounds[2] / 4} $top 450"
            else "input swipe $x ${if (older) top else bottom} $x ${if (older) bottom else top} 450"
        val output = instrumentation.uiAutomation.executeShellCommand(command).use { fd -> java.io.FileInputStream(fd.fileDescriptor).use { String(it.readBytes()) } }
        assertTrue("Swipe command failed: $output", output.isBlank())
        Thread.sleep(150)
    }
    private fun screenshot(name: String) {
        instrumentation.waitForIdleSync()
        instrumentation.uiAutomation.takeScreenshot().let { image ->
            File(context.filesDir, "terminal-scroll-$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }; image.recycle()
        }
    }
    @Test fun mainSurfaceReadsHistoryAndReturnsToLiveWithoutLosingUpdates() {
        val fixture = JSONObject(File(context.filesDir, "terminal-scroll-fixture.json").readText())
        val originalAccount = context.getSharedPreferences("account", 0).all.toMap()
        val account = Account(); val sender = RemoteTerminal()
        try {
            context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
                .putExtra("isolated_ui", true).putExtra("render_fixture", true))
            waitFor("Activity") { main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED)
                .filterIsInstance<MainActivity>().firstOrNull { it.hasWindowFocus() }?.also { activity = it } != null } }
            val remote = main { get("remote") as RemoteTerminal }
            account.login(fixture.getString("server"), fixture.getString("username"), fixture.getString("password"), "scroll fixture", "android", "")
            var desktop = ""
            waitFor("Desktop") { desktop = account.devices().firstOrNull { it.platform == "desktop" && it.online }?.id.orEmpty(); desktop.isNotEmpty() }
            val session = fixture.getString("session")
            account.connect(desktop, remote)
            val initial = remote.select(session, false)
            main {
                set("selected", session); set("connected", true)
                MainActivity::class.java.getDeclaredMethod("show", RenderFrame::class.java).apply { isAccessible = true }.invoke(activity, initial)
                val viewport = (get("surface") as View).parent as TerminalScrollView
                val original = viewport.scrollHistory
                viewport.scrollHistory = { gestures.add(it); original(it) }
            }
            waitFor("latest output") { text().contains("UI_LOG_059") }
            screenshot("live")
            val initialVerticalRange = main { ((get("surface") as View).parent as View).canScrollVertically(-1) }
            assertFalse("Fixture should fit in the live viewport", initialVerticalRange)
            repeat(5) { if (!text().contains("UI_LOG_000")) swipe(true) }
            waitFor("main view reached earliest output") { text().contains("UI_LOG_000") }
            assertTrue(main { (get("terminalScrollback") as TerminalScrollback).reading })
            assertFalse(displayed().cursorVisible)
            screenshot("history")
            val reading = text()
            account.connect(desktop, sender); sender.select(session, true)
            sender.typeText("printf '__LIVE_APPEND__\\n'"); sender.sendKey("enter")
            waitFor("live replica continues") { remote.refresh()?.cells?.joinToString("") { it.text }?.contains("__LIVE_APPEND__") == true }
            assertEquals("Output moved the reading view", reading, text())
            swipe(true, true)
            assertTrue("Horizontal pan stopped working", main { (get("surface") as HorizontalScrollView).scrollX > 0 })
            repeat(6) { if (main { (get("terminalScrollback") as TerminalScrollback).reading }) swipe(false) }
            waitFor("returned to live") { !main { (get("terminalScrollback") as TerminalScrollback).reading } && text().contains("__LIVE_APPEND__") }
            screenshot("returned")
            main {
                set("accountName", "isolated-scroll-test"); set("heartbeatBusy", true)
                MainActivity::class.java.getDeclaredMethod("select", String::class.java, Boolean::class.javaPrimitiveType, Boolean::class.javaPrimitiveType)
                    .apply { isAccessible = true }.invoke(activity, session, true, false)
            }
            waitFor("input-enabled production selection") { main { get("selected") == session && get("controlled") == true && get("terminal") != null && get("entryPending") == false } }
            swipe(true)
            waitFor("history before typing") { main { (get("terminalScrollback") as TerminalScrollback).reading } }
            main {
                assertTrue(terminal().sendText("printf '\\137\\137TYPED_ONCE\\137\\137\\n'"))
                assertTrue(terminal().sendKey("enter"))
                assertFalse((get("terminalScrollback") as TerminalScrollback).reading)
            }
            waitFor("typed output after leaving history") { text().contains("__TYPED_ONCE__") }
            val typed = displayed()
            assertEquals(1, typed.cells.chunked(typed.cols.toInt()).count { row -> row.filter { it.width > 0u }.joinToString("") { it.text }.trim() == "__TYPED_ONCE__" })
            main {
                val viewport = (get("surface") as View).parent as TerminalScrollView
                val point = android.view.MotionEvent.PointerCoords().apply { x = 150f; y = 150f; setAxisValue(android.view.MotionEvent.AXIS_VSCROLL, 50f) }
                val pointer = android.view.MotionEvent.PointerProperties().apply { id = 0; toolType = android.view.MotionEvent.TOOL_TYPE_MOUSE }
                val now = SystemClock.uptimeMillis()
                val event = android.view.MotionEvent.obtain(now, now, android.view.MotionEvent.ACTION_SCROLL, 1, arrayOf(pointer), arrayOf(point), 0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_MOUSE, 0)
                assertTrue(viewport.dispatchGenericMotionEvent(event)); event.recycle()
            }
            waitFor("mouse wheel reads history") { text().contains("UI_LOG_000") }
            assertEquals(originalAccount, context.getSharedPreferences("account", 0).all)
            File(context.filesDir, "terminal-scroll-results.json").writeText(JSONObject().put("passed", true)
                .put("main_view_history", true).put("continuous_live_replica", true).put("horizontal_pan", true).put("typing_returns_live_once", true).put("mouse_wheel", true).put("user_account_preserved", true).toString())
        } catch (error: Throwable) {
            if (::activity.isInitialized) {
                screenshot("failed")
                val diagnostic = main {
                    val viewport = (get("surface") as View).parent as TerminalScrollView
                    JSONObject().put("passed", false).put("error", error.toString()).put("gestures", gestures.toString())
                        .put("canRead", viewport.canReadHistory()).put("reading", viewport.isReading())
                        .put("status", (get("status") as android.widget.TextView).text.toString())
                        .put("text", text().take(200))
                }
                File(context.filesDir, "terminal-scroll-results.json").writeText(diagnostic.toString())
            }
            throw error
        } finally {
            runCatching { sender.disconnect() }; sender.close(); account.close()
            if (::activity.isInitialized) main { activity.finish() }
        }
    }
}
