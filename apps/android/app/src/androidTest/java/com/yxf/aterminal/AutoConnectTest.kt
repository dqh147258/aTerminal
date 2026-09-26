package com.yxf.aterminal

import android.content.Intent
import android.graphics.Bitmap
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import androidx.test.runner.lifecycle.ActivityLifecycleCallback
import org.junit.After
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.ai_terminal_mobile.AccountDevice
import uniffi.ai_terminal_mobile.RenderFrame
import java.io.File

/** Uses a disposable server/account and real Desktop PTYs supplied by the coordinator. */
class AutoConnectTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private var auditEntry = false
    private var auditNextConnection = false
    private var readyDraws = 0
    private val emptyDraws = mutableListOf<String>()
    private val lifecycle = ActivityLifecycleCallback { candidate, stage ->
        if (candidate is MainActivity && stage == Stage.STARTED) {
            candidate.window.decorView.viewTreeObserver.addOnPreDrawListener {
                fun value(name: String) = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(candidate)
                if (auditNextConnection && (value("entryPending") == true || value("selected") != null)) { auditEntry = true; auditNextConnection = false }
                if (auditEntry && value("active") == true && value("accountName") != "" && (value("workspace") as View).isShown) {
                    if (value("terminal") == null || value("selected") == null) emptyDraws.add((value("status") as TextView).text.toString())
                    else readyDraws++
                }
                true
            }
        }
    }
    @After fun removeLifecycleObserver() { main { ActivityLifecycleMonitorRegistry.getInstance().removeLifecycleCallback(lifecycle) } }
    private fun assertEntryHasTerminal() {
        waitFor("first home draw has a Terminal") { readyDraws > 0 }
        main { assertTrue("Home flashed before first Terminal: $emptyDraws", emptyDraws.isEmpty()); auditEntry = false; readyDraws = 0 }
    }
    private fun <T> main(action: () -> T): T {
        var answer: T? = null
        var failure: Throwable? = null
        instrumentation.runOnMainSync { try { answer = action() } catch (e: Throwable) { failure = e } }
        failure?.let { throw it }
        @Suppress("UNCHECKED_CAST") return answer as T
    }
    private fun get(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun waitFor(label: String, predicate: () -> Boolean) {
        val until = SystemClock.elapsedRealtime() + 30000
        while (SystemClock.elapsedRealtime() < until) { if (main(predicate)) return; Thread.sleep(50) }
        fail("Timed out: $label; status=" + main { (get("status") as TextView).text })
    }
    private fun icon(description: String) = main { views().first { it.contentDescription?.toString() == description }.performClick() }
    private fun click(text: String) = main { views().filterIsInstance<Button>().first { it.text.toString() == text && it.isEnabled }.performClick() }
    private fun screenshot(name: String) {
        instrumentation.waitForIdleSync()
        instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
            File(context.filesDir, "autoconnect-$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }
    @Test fun singleDesktopSelectsNewestAndHidesThisDevice() {
        val fixture = JSONObject(File(context.filesDir, "autoconnect-fixture.json").readText())
        PairingStore(context, "acceptance-account").clear()
        main { ActivityLifecycleMonitorRegistry.getInstance().addLifecycleCallback(lifecycle) }
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("acceptance_test", true))
        val until = SystemClock.elapsedRealtime() + 10000
        while (SystemClock.elapsedRealtime() < until) {
            val current = main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull() }
            if (current != null) { activity = current; break }; Thread.sleep(50)
        }
        assertTrue(::activity.isInitialized)
        waitFor("account restoration") { get("loginBusy") == false }
        click("修改服务器地址")
        main {
            fun field(hint: String) = views().filterIsInstance<EditText>().first { it.hint.toString() == hint }
            field("服务器 https://…").setText(fixture.getString("server"))
            field("账号").setText(fixture.getString("username"))
            field("密码").setText(fixture.getString("password"))
        }
        click("保存服务器地址")
        main { auditEntry = !fixture.optBoolean("late_desktop") && !fixture.optBoolean("late_session") }
        click("登录")
        if (fixture.optBoolean("late_desktop")) {
            waitFor("no Desktop available ends loading") { get("accountName") == fixture.getString("username") && get("entryPending") == false && get("connected") == false }
            main { auditNextConnection = true }
            File(context.filesDir, "autoconnect-waiting.json").writeText("ready")
        }
        if (fixture.optBoolean("late_session")) {
            waitFor("empty Desktop ends loading") { get("accountName") == fixture.getString("username") && get("entryPending") == false && get("connected") == true && get("selected") == null }
            main { auditNextConnection = true }
            File(context.filesDir, "autoconnect-waiting.json").writeText("ready")
            waitFor("coordinator created PTYs") {
                val updated = try { JSONObject(File(context.filesDir, "autoconnect-fixture.json").readText()) } catch (_: Exception) { return@waitFor false }
                if (updated.optString("newest").isBlank()) false else { fixture.put("newest", updated.getString("newest")); fixture.put("oldest", updated.getString("oldest")); true }
            }
        }
        val newest = fixture.getString("newest")
        waitFor("automatic connection without pressing Connect") { get("connected") == true && get("selected") == newest }
        waitFor("newest PTY screen") {
            val terminal = get("terminal") as? TerminalView ?: return@waitFor false
            val frame = TerminalView::class.java.getDeclaredField("frame").apply { isAccessible = true }.get(terminal) as? RenderFrame
            frame?.cells?.chunked(frame.cols.toInt())?.any { row -> row.joinToString("") { it.text }.contains("NEWEST_TERMINAL_OK") } == true
        }
        assertEntryHasTerminal()
        screenshot("latest")
        icon("账号与设备")
        click("刷新设备")
        waitFor("presence after the first heartbeat") { get("deviceBusy") == false }
        main {
            @Suppress("UNCHECKED_CAST") val devices = get("devices") as List<AccountDevice>
            assertEquals(1, devices.count { it.current && it.online })
            val own = devices.single { it.current }
            assertFalse(views().filterIsInstance<TextView>().any { it.text.toString() == own.name || it.text.toString().contains("本机") })
            assertEquals(1, views().count { it.contentDescription?.toString()?.startsWith("连接 ") == true })
        }
        screenshot("devices")
        click("刷新设备")
        waitFor("device refresh completed") { get("deviceBusy") == false }
        assertEquals(newest, main { get("selected") })
        main { MainActivity::class.java.getDeclaredMethod("closeOverlay", Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(activity, true) }
        icon("打开工作空间")
        main { views().filterIsInstance<Button>().first { it.tag == fixture.getString("oldest") }.performClick() }
        waitFor("manual older selection") { get("selected") == fixture.getString("oldest") }
        icon("账号与设备"); click("刷新设备")
        waitFor("refresh preserves manual selection") { get("deviceBusy") == false }
        assertEquals(fixture.getString("oldest"), main { get("selected") })
        val previous = activity
        main { auditEntry = true; activity.recreate() }
        waitFor("activity recreation") {
            val current = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull()
            if (current != null && current !== previous) { activity = current; true } else false
        }
        waitFor("restored login chooses newest instead of last selected") { get("connected") == true && get("selected") == newest }
        assertEntryHasTerminal()
        screenshot("reconnected")
        File(context.filesDir, "autoconnect-results.json").writeText(JSONObject()
            .put("passed", true).put("first_home_draw_has_terminal", true).put("late_desktop", fixture.optBoolean("late_desktop")).put("late_session", fixture.optBoolean("late_session")).put("self_hidden", true).put("single_device_auto_connect", true)
            .put("newest_terminal", newest).put("refresh_preserves_selection", true)
            .put("reconnect_prefers_newest_over_previous", true).toString(2))
        main { activity.finish() }
        PairingStore(context, "acceptance-account").clear()
    }
}
