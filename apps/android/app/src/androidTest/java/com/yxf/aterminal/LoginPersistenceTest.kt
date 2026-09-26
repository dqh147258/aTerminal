package com.yxf.aterminal

import android.content.Intent
import android.os.Process
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.view.accessibility.AccessibilityNodeInfo
import android.widget.Button
import android.widget.EditText
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.TimeUnit

/** Each phase runs in a new process; the Python coordinator force-stops between phases. */
class LoginPersistenceTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private fun <T> main(action: () -> T): T {
        var result: T? = null
        var failure: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (e: Throwable) { failure = e } }
        failure?.let { throw it }
        @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun get(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun waitFor(label: String, predicate: () -> Boolean) {
        val until = SystemClock.elapsedRealtime() + 30000
        while (SystemClock.elapsedRealtime() < until) { if (main(predicate)) return; Thread.sleep(50) }
        fail("Timed out: $label")
    }
    private fun click(text: String) = main { views().filterIsInstance<Button>().first { it.text.toString() == text && it.isEnabled }.performClick() }
    @Test fun persistentAccountLifecycle() {
        val phase = InstrumentationRegistry.getArguments().getString("phase")!!
        val fixture = JSONObject(File(context.filesDir, "autoconnect-fixture.json").readText())
        val store = PairingStore(context, "acceptance-account")
        if (phase == "login") {
            store.clear()
            context.getSharedPreferences("acceptance-connection", 0).edit().remove("server").commit()
        }
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("acceptance_test", true))
        val until = SystemClock.elapsedRealtime() + 10000
        while (SystemClock.elapsedRealtime() < until) {
            val current = main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull() }
            if (current != null) { activity = current; break }; Thread.sleep(50)
        }
        assertTrue(::activity.isInitialized)
        if (phase == "login" || phase == "relogin") {
            waitFor("initial restoration finished") { get("loginBusy") == false }
            assertFalse(main { views().filterIsInstance<EditText>().any { it.hint == "服务器 https://…" } })
            if (phase == "login") assertEquals(BuildConfig.DEFAULT_SERVER_URL, main { (views().first { it.tag == "login.server.address" } as android.widget.TextView).text.toString() })
            else assertEquals(fixture.getString("server"), main { (views().first { it.tag == "login.server.address" } as android.widget.TextView).text.toString() })
            click("修改服务器地址")
            main {
                fun field(hint: String) = views().filterIsInstance<EditText>().first { it.hint.toString() == hint }
                field("服务器 https://…").setText(fixture.getString("server"))
                field("账号").setText(fixture.getString("username"))
                field("密码").setText(fixture.getString("password"))
            }
            click("保存服务器地址")
            assertFalse(main { views().filterIsInstance<EditText>().any { it.hint == "服务器 https://…" } })
            click("登录")
        }
        if (phase == "signedout") {
            waitFor("signed out restoration finished") { get("loginBusy") == false }
            assertEquals("", main { get("accountName") })
            assertTrue(main { views().filterIsInstance<EditText>().any { it.hint == "密码" } })
            assertNull(store.load())
        } else {
            waitFor("identity restored without typing") { get("accountName") == fixture.getString("username") }
            if (phase == "offline") {
                waitFor("offline device lookup finished") { get("loginBusy") == false }
                assertEquals(fixture.getString("username"), main { get("accountName") })
                assertEquals(false, main { get("connected") })
            } else waitFor("real Desktop reconnected") { get("connected") == true && get("selected") == fixture.getString("newest") }
            val value = store.load()!!
            val session = JSONObject(value)
            val device = session.getJSONObject("tokens").getString("device_id")
            val identityFile = File(context.filesDir, "login-test-device.txt")
            if (phase == "login") identityFile.writeText(device)
            else if (phase == "relogin") assertNotEquals(identityFile.readText(), device)
            else assertEquals(identityFile.readText(), device)
            assertFalse(value.contains(fixture.getString("password")))
            val encrypted = context.getSharedPreferences("acceptance-account", 0).getString("value", "")!!
            assertFalse(encrypted.contains(fixture.getString("username")))
            if (phase == "logout") {
                // Keep network logout queued and simulate a pre-logout heartbeat write completing late.
                val entered = CountDownLatch(1)
                val release = CountDownLatch(1)
                val staleDone = CountDownLatch(1)
                val epoch = main { get("accountEpoch") as Int }
                val worker = get("worker") as ExecutorService
                worker.execute {
                    entered.countDown()
                    if (release.await(30, TimeUnit.SECONDS)) {
                        MainActivity::class.java.getDeclaredMethod("persist", Int::class.javaPrimitiveType).apply { isAccessible = true }.invoke(activity, epoch)
                        staleDone.countDown()
                        // Coordinator kills this process before the pending network logout starts.
                        CountDownLatch(1).await(30, TimeUnit.SECONDS)
                    }
                }
                assertTrue(entered.await(10, TimeUnit.SECONDS))
                main { views().first { it.contentDescription == "账号与设备" }.performClick() }
                click("退出登录")
                instrumentation.waitForIdleSync()
                var confirmed = false
                val dialogDeadline = SystemClock.elapsedRealtime() + 5000
                while (!confirmed && SystemClock.elapsedRealtime() < dialogDeadline) {
                    val button = instrumentation.uiAutomation.rootInActiveWindow?.findAccessibilityNodeInfosByText("退出")?.firstOrNull { it.text?.toString() == "退出" }
                    confirmed = button?.performAction(AccessibilityNodeInfo.ACTION_CLICK) == true
                    if (!confirmed) Thread.sleep(50)
                }
                assertTrue("Logout confirmation appeared", confirmed)
                waitFor("local logout without waiting for network") { get("accountName") == "" }
                assertNull("Credentials must be gone before network logout", store.load())
                release.countDown()
                assertTrue(staleDone.await(10, TimeUnit.SECONDS))
                assertNull("Late persistence must not resurrect credentials", store.load())
            }
        }
        File(context.filesDir, "login-$phase.json").writeText(JSONObject().put("passed", true).put("phase", phase).put("pid", Process.myPid()).toString())
    }
}
