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
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.File

/** Explicit local-login helper: uses the normal account namespace and leaves it signed in. */
class LocalLoginTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private fun <T> main(action: () -> T): T {
        var value: T? = null
        var failure: Throwable? = null
        instrumentation.runOnMainSync { try { value = action() } catch (e: Throwable) { failure = e } }
        failure?.let { throw it }
        @Suppress("UNCHECKED_CAST") return value as T
    }
    private fun get(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun waitFor(label: String, predicate: () -> Boolean) {
        val until = SystemClock.elapsedRealtime() + 30000
        while (SystemClock.elapsedRealtime() < until) { if (main(predicate)) return; Thread.sleep(50) }
        fail("Timed out: $label")
    }
    @Test fun loginWithBundledServerAndRestore() {
        val fixtureFile = File(context.filesDir, "local-login-fixture.json")
        if (!fixtureFile.exists()) org.junit.Assume.assumeTrue("Explicit local-login fixture required", false)
        val fixture = JSONObject(fixtureFile.readText())
        val restoreOnly = InstrumentationRegistry.getArguments().getString("phase") == "restored"
        assertEquals(fixture.getString("server"), BuildConfig.DEFAULT_SERVER_URL)
        assertTrue("Local HTTPS CA is bundled", BuildConfig.DEFAULT_SERVER_CA_PEM.isNotEmpty())
        val store = PairingStore(context, "account")
        val saved = store.load()
        if (restoreOnly) assertNotNull("Previous login was persisted", saved)
        if (saved != null) {
            val previous = JSONObject(saved)
            assertEquals("Do not replace another account", fixture.getString("username"), previous.getJSONObject("tokens").getString("username"))
            assertEquals("Do not replace another server", fixture.getString("server"), previous.getString("server"))
        }
        val launchedAt = SystemClock.elapsedRealtime()
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK))
        val until = SystemClock.elapsedRealtime() + 10000
        while (SystemClock.elapsedRealtime() < until) {
            val current = main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull() }
            if (current != null) { activity = current; break }; Thread.sleep(50)
        }
        assertTrue(::activity.isInitialized)
        waitFor("normal startup restoration") { get("loginBusy") == false }
        if (saved == null) {
            main {
                val views = all(activity.window.decorView).filter { it.isShown }
                assertEquals(fixture.getString("server"), (views.first { it.tag == "login.server.address" } as TextView).text.toString())
                assertFalse(views.filterIsInstance<EditText>().any { it.hint == "服务器 https://…" })
            }
            waitFor("login page drawn") { activity.hasWindowFocus() && (get("loginBox") as View).isLaidOut }
            instrumentation.waitForIdleSync()
            instrumentation.uiAutomation.waitForIdle(250, 3000)
            instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
                File(context.filesDir, "local-login-page.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
            }
            main {
                val views = all(activity.window.decorView).filter { it.isShown }
                views.filterIsInstance<EditText>().first { it.hint == "账号" }.setText(fixture.getString("username"))
                views.filterIsInstance<EditText>().first { it.hint == "密码" }.setText(fixture.getString("password"))
                views.filterIsInstance<Button>().first { it.text == "登录" }.performClick()
            }
        }
        waitFor("local account signed in") { get("accountName") == fixture.getString("username") && get("loginBusy") == false && get("entryPending") == false }
        assertEquals(fixture.getString("server"), main { get("serverUrl") })
        val session = JSONObject(store.load()!!)
        val phase = if (restoreOnly) "restored" else "login"
        File(context.filesDir, "local-login-$phase.json").writeText(JSONObject()
            .put("passed", true).put("phase", phase).put("pid", android.os.Process.myPid())
            .put("username", fixture.getString("username")).put("server", fixture.getString("server"))
            .put("device_id", session.getJSONObject("tokens").getString("device_id"))
            .put("terminal_selected", main { get("selected") } ?: JSONObject.NULL)
            .put("connected", main { get("connected") }).put("ready_ms", SystemClock.elapsedRealtime() - launchedAt).toString())
    }
}
