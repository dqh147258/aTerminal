package dev.aiterminal.app

import android.os.*
import android.text.Editable
import android.text.TextWatcher
import android.view.*
import android.view.inputmethod.EditorInfo
import android.widget.*
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.ai_terminal_mobile.*
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicBoolean

/** Real-device acceptance probe. Credentials are supplied in app-private storage, never APK assets/logs. */
class DeviceAcceptanceTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private lateinit var activity: MainActivity
    private val result = JSONObject()
    private fun assertForeground() {
        assertEquals("Unexpected target package", "dev.aiterminal.app", activity.packageName)
        assertTrue("AI Terminal lost foreground focus; stop input injection", activity.hasWindowFocus())
        assertTrue("AI Terminal is not active", get("active") == true)
    }
    private fun <T> main(action: () -> T): T { var answer: T? = null; var error: Throwable? = null; instrumentation.runOnMainSync { try { answer = action() } catch (e: Throwable) { error = e } }; error?.let { throw it }; @Suppress("UNCHECKED_CAST") return answer as T }
    private fun all(view: View): List<View> = listOf(view) + if (view is android.view.ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView)
    private fun field(hint: String): EditText = views().filterIsInstance<EditText>().first { it.hint?.toString() == hint && it.isShown }
    private fun click(text: String) { main { assertForeground(); views().filterIsInstance<Button>().first { it.text.toString() == text && it.isShown }.performClick() } }
    private fun clickDescription(text: String) { main { assertForeground(); views().first { it.contentDescription?.toString() == text && it.isShown }.performClick() } }
    private fun logout() {
        clickDescription("账号与设备"); click("退出登录")
        instrumentation.waitForIdleSync()
        instrumentation.uiAutomation.rootInActiveWindow.findAccessibilityNodeInfosByText("退出").last { it.isClickable }
            .performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK)
    }
    private fun waitFor(label: String, seconds: Long = 20, condition: () -> Boolean) { val deadline = SystemClock.elapsedRealtime() + seconds * 1000; while (SystemClock.elapsedRealtime() < deadline) { if (main(condition)) return; Thread.sleep(30) }; throw AssertionError("Timed out: $label") }
    private fun get(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun core() = get("remote") as RemoteTerminal
    private fun terminal() = get("terminal") as? TerminalView
    private val frameField = TerminalView::class.java.getDeclaredField("frame").apply { isAccessible = true }
    private fun frame() = terminal()?.let { frameField.get(it) as RenderFrame }
    private fun screenText() = frame()?.cells?.joinToString("") { it.text } ?: ""
    private fun stats(samples: List<Double>): JSONObject { assertTrue("No samples", samples.isNotEmpty()); val sorted = samples.sorted(); return JSONObject().put("count", sorted.size).put("p50_ms", sorted[sorted.size / 2]).put("p95_ms", sorted[(sorted.size * 95 + 99) / 100 - 1]).put("p99_ms", sorted[(sorted.size * 99 + 99) / 100 - 1]) }
    private fun select(id: String) {
        val sessions = core().sessions(); val index = sessions.indexOfFirst { it.id == id }; assertTrue(index >= 0)
        clickDescription("打开工作空间")
        main { views().filterIsInstance<Button>().first { it.text.toString().startsWith(sessions[index].cwd + "\n") && it.isShown }.performClick() }
        waitFor("selected session") { get("selected") == id }
        if (!main { (get("inputBox") as View).isShown }) clickDescription("显示或隐藏终端输入")
        if (!main { core().hasControl() }) { main { (get("takeControl") as CheckBox).performClick() }; waitFor("input control") { get("selected") == id && get("controlled") == true && core().hasControl() } }
    }
    private fun send(text: String) { main { field("输入文字").setText(text) }; click("发送") }
    private fun connectDesktop() {
        waitFor("signed in") { (get("workspace") as View).isShown }
        clickDescription("账号与设备")
        waitFor("online desktop") { views().any { it.contentDescription?.toString() == "连接 Local Desktop" && it.isEnabled } }
        clickDescription("连接 Local Desktop")
        waitFor("sessions") { get("selected") != null }
    }

    @Test fun accountInputAndPerformance() {
        val context = instrumentation.targetContext
        val fixture = JSONObject(File(context.filesDir, "device-fixture.json").readText())
        result.put("model", Build.MODEL).put("android", Build.VERSION.RELEASE).put("api", Build.VERSION.SDK_INT).put("native_build", InstrumentationRegistry.getArguments().getString("nativeBuild", "debug"))
        // MIUI can block an instrumentation process launching its own Activity from the background.
        instrumentation.uiAutomation.executeShellCommand("am start -W -n dev.aiterminal.app/.MainActivity --ez acceptance_test true").use { fd -> java.io.FileInputStream(fd.fileDescriptor).use { it.readBytes() } }
        val launchDeadline=SystemClock.elapsedRealtime()+10000
        var launched: MainActivity? = null
        while (launched==null && SystemClock.elapsedRealtime()<launchDeadline) { launched=main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull() }; if(launched==null) Thread.sleep(30) }
        activity=launched ?: throw AssertionError("Activity did not resume")
        waitFor("AI Terminal foreground focus") { activity.hasWindowFocus() }
        try {
            if (PairingStore(context,"acceptance-account").load() != null) { waitFor("restored account") { (get("workspace") as View).isShown }; logout(); waitFor("logout before isolated login") { (get("loginBox") as View).isShown } }
            main { field("服务器 https://…").setText(fixture.getString("server")); field("账号").setText(fixture.getString("username")); field("密码").setText(fixture.getString("password")) }
            click("登录"); connectDesktop(); select(fixture.getString("session"))
            waitFor("Wi-Fi direct", 15) { core().connectionPath() == "direct" }
            send("printf 'ANDROID_DEVICE_OK\\n'"); click("回车")
            waitFor("actual shell output") { val f = frame() ?: return@waitFor false; f.cells.chunked(f.cols.toInt()).any { line -> line.joinToString("") { it.text }.trim() == "ANDROID_DEVICE_OK" } }
            result.put("account_login_ui", true).put("direct_shell_output", true)
            // Inject actual key events into the focused native EditText.
            main { assertForeground(); field("输入文字").requestFocus(); field("输入文字").setText("") }
            instrumentation.sendStringSync("abc123")
            assertEquals("abc123", main { field("输入文字").text.toString() })
            main { assertForeground() }
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_DEL)
            assertEquals("abc12", main { field("输入文字").text.toString() })
            main {
                val edit = field("输入文字"); edit.setText("")
                val connection = edit.onCreateInputConnection(EditorInfo())
                connection.setComposingText("zhongwen", 1)
                connection.commitText("中文🙂", 1); connection.finishComposingText()
                assertEquals("中文🙂", edit.text.toString()); edit.setText("")
            }
            result.put("native_key_edit_delete", true).put("input_connection_chinese_emoji", true)
            send("sleep 30"); click("回车"); Thread.sleep(200); click("Ctrl-C")
            send("printf 'CTRL_C_RECOVERED\\n'"); click("回车")
            waitFor("Ctrl-C releases shell") { val f=frame() ?: return@waitFor false; f.cells.chunked(f.cols.toInt()).any { line -> line.joinToString("") { it.text }.trim() == "CTRL_C_RECOVERED" } }
            result.put("ctrl_c", true)
            val benchmarks = fixture.getJSONArray("benchmarks"); val runs = JSONArray()
            for (i in 0 until if (InstrumentationRegistry.getArguments().getString("mode") == "smoke") 0 else benchmarks.length()) {
                val entry=benchmarks.getJSONObject(i); select(entry.getString("id"))
                runs.put(measure(entry.getString("name"), entry.getString("name").startsWith("history")))
            }
            result.put("direct_benchmarks", runs)
            if (benchmarks.length() > 0) {
                val method = MainActivity::class.java.getDeclaredMethod("select", String::class.java, Boolean::class.javaPrimitiveType, Boolean::class.javaPrimitiveType).apply { isAccessible = true }
                val executor = get("historyWorker") as java.util.concurrent.ExecutorService
                val historyGate = java.util.concurrent.CountDownLatch(1)
                executor.execute { historyGate.await(10, java.util.concurrent.TimeUnit.SECONDS) }
                try {
                    click("历史")
                    main { method.invoke(activity, fixture.getString("session"), false, false) }
                } finally { historyGate.countDown() }
                executor.submit {}.get(10, java.util.concurrent.TimeUnit.SECONDS)
                waitFor("session after delayed history") { get("selected") == fixture.getString("session") }
                main { assertNull("Late history opened a panel for the previous session", get("overlay")) }
                result.put("stale_history_suppressed", true)
                repeat(6) { index ->
                    val id = if (index % 2 == 0) fixture.getString("session") else benchmarks.getJSONObject(0).getString("id")
                    val intermediate = if (id == fixture.getString("session")) benchmarks.getJSONObject(0).getString("id") else fixture.getString("session")
                    main { assertForeground(); method.invoke(activity, intermediate, false, false); method.invoke(activity, id, false, false) }
                    waitFor("rapid session switch") { get("selected") == id }
                    val authority = core().refresh() ?: throw AssertionError("Missing selected state")
                    waitFor("UI equals validated replica after switch") { frame() == authority }
                }
                result.put("rapid_session_switches", 6)
            }
            // Reconnect for the relay control; its account route is deliberately USB forwarded loopback.
            select(fixture.getString("session")); core().useRelay()
            send("printf 'ANDROID_RELAY_OK\\n'"); click("回车")
            waitFor("relay output") { core().connectionPath() == "relay" && screenText().contains("ANDROID_RELAY_OK") }
            result.put("usb_relay_shell_output", true)
            // Pause/stop this Activity, resume it, and use the real device/session controls again.
            instrumentation.uiAutomation.executeShellCommand("input keyevent KEYCODE_HOME").use { fd -> java.io.FileInputStream(fd.fileDescriptor).use { it.readBytes() } }
            waitFor("actual background") { get("active") == false }
            val until=SystemClock.elapsedRealtime()+10000
            while (core().hasControl() && SystemClock.elapsedRealtime()<until) Thread.sleep(30)
            assertFalse(core().hasControl())
            instrumentation.uiAutomation.executeShellCommand("am start -W -n dev.aiterminal.app/.MainActivity --ez acceptance_test true").use { fd -> java.io.FileInputStream(fd.fileDescriptor).use { it.readBytes() } }
            waitFor("actual foreground") { get("active") == true && activity.hasWindowFocus() }
            connectDesktop(); select(fixture.getString("session")); send("printf 'ANDROID_RESUME_OK\\n'"); click("回车")
            waitFor("resume output") { screenText().contains("ANDROID_RESUME_OK") }; result.put("background_release_reconnect", true)
            logout(); waitFor("logout") { (get("loginBox") as View).isShown }; result.put("logout_ui", true)
            result.put("passed", true)
        } finally {
            result.put("last_status", main { (get("status") as TextView).text.toString() })
            File(context.filesDir, "device-results.json").writeText(result.toString(2))
            main { activity.finish() }
        }
    }
    private fun measure(name: String, history: Boolean): JSONObject {
        val inputIntervalMs = InstrumentationRegistry.getArguments().getString("inputIntervalMs", "10").toLong().coerceIn(1, 100)
        val count=1000; val sent=LongArray(count); val seen=DoubleArray(count); val frameTimes=CopyOnWriteArrayList<Double>(); val drawTimes=CopyOnWriteArrayList<Double>(); val layoutTimes=CopyOnWriteArrayList<Double>(); val delayTimes=CopyOnWriteArrayList<Double>(); val metricThread=HandlerThread("device-frame-metrics").apply { start() }
        val sampling=AtomicBoolean(true)
        val animationTimes=CopyOnWriteArrayList<Double>(); val inputTimes=CopyOnWriteArrayList<Double>(); val enqueueTimes=ArrayList<Double>()
        val metrics=Window.OnFrameMetricsAvailableListener { _, metrics, _ -> if (sampling.get()) { frameTimes.add(metrics.getMetric(FrameMetrics.TOTAL_DURATION)/1_000_000.0); drawTimes.add(metrics.getMetric(FrameMetrics.DRAW_DURATION)/1_000_000.0); layoutTimes.add(metrics.getMetric(FrameMetrics.LAYOUT_MEASURE_DURATION)/1_000_000.0); delayTimes.add(metrics.getMetric(FrameMetrics.UNKNOWN_DELAY_DURATION)/1_000_000.0); animationTimes.add(metrics.getMetric(FrameMetrics.ANIMATION_DURATION)/1_000_000.0); inputTimes.add(metrics.getMetric(FrameMetrics.INPUT_HANDLING_DURATION)/1_000_000.0) } }
        val listener=ViewTreeObserver.OnDrawListener {
            val value=frame() ?: return@OnDrawListener
            val shown=value.cells.count { it.text == "x" }.coerceAtMost(count); val now=System.nanoTime()
            for (i in 0 until shown) if (sent[i]!=0L && seen[i]==0.0) seen[i]=(now-sent[i])/1_000_000.0
        }
        main { activity.window.addOnFrameMetricsAvailableListener(metrics, Handler(metricThread.looper)); terminal()!!.viewTreeObserver.addOnDrawListener(listener) }
        val historyThread=if (history) Thread { while (sampling.get()) { core().readHistory(); Thread.sleep(100) } }.apply { start() } else null
        val started=SystemClock.elapsedRealtime()
        try {
            for (i in 0 until count) {
                main { assertForeground(); sent[i]=System.nanoTime(); core().sendText("x",false); enqueueTimes.add((System.nanoTime()-sent[i])/1_000_000.0) }
                val remaining = sent[i] + inputIntervalMs * 1_000_000 - System.nanoTime()
                if (remaining > 0) Thread.sleep(remaining / 1_000_000, (remaining % 1_000_000).toInt())
            }
            val inputDuration = SystemClock.elapsedRealtime() - started
            waitFor("1000 native-rendered characters: $name", 20) { seen.all { it>0 } }
            assertEquals(1000, main { frame()!!.cells.count { it.text=="x" } })
            // Local editor receives real key events while this terminal remains active.
            val editLatency=CopyOnWriteArrayList<Double>(); val inputAt=java.util.concurrent.atomic.AtomicLong()
            val watcher=object:TextWatcher { override fun beforeTextChanged(s:CharSequence?,start:Int,count:Int,after:Int){};override fun onTextChanged(s:CharSequence?,start:Int,before:Int,count:Int){};override fun afterTextChanged(s:Editable?){val start=inputAt.get();if(start!=0L)editLatency.add((System.nanoTime()-start)/1_000_000.0)} }
            main { field("输入文字").requestFocus(); field("输入文字").setText(""); field("输入文字").addTextChangedListener(watcher) }
            repeat(60) { main { assertForeground() }; inputAt.set(System.nanoTime());instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_A);inputAt.set(0);Thread.sleep(10) }
            main { field("输入文字").removeTextChangedListener(watcher); field("输入文字").setText("") }
            val memory=android.os.Debug.MemoryInfo();android.os.Debug.getMemoryInfo(memory)
            return JSONObject().put("name",name).put("path",core().connectionPath()).put("input_interval_ms",inputIntervalMs).put("input_duration_ms",inputDuration).put("input_to_onDraw",stats(seen.toList())).put("window_frame_total",stats(frameTimes)).put("window_draw",stats(drawTimes)).put("window_layout",stats(layoutTimes)).put("window_delay",stats(delayTimes)).put("window_animation",stats(animationTimes)).put("window_input",stats(inputTimes)).put("enqueue_call",stats(enqueueTimes)).put("native_editor_event",stats(editLatency)).put("elapsed_ms",SystemClock.elapsedRealtime()-started).put("total_pss_kib",memory.totalPss).put("frames_over_16_7_ms",frameTimes.count { it>16.7 })
        } finally {
            sampling.set(false); historyThread?.join(3000)
            main { terminal()?.viewTreeObserver?.removeOnDrawListener(listener);activity.window.removeOnFrameMetricsAvailableListener(metrics) };metricThread.quitSafely()
        }
    }
}
