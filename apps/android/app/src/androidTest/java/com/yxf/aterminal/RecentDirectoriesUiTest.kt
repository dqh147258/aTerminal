package com.yxf.aterminal

import android.app.AlertDialog
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Rect
import android.os.Build
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.ScrollView
import android.widget.TextView
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import uniffi.ai_terminal_mobile.Account
import uniffi.ai_terminal_mobile.RemoteSession
import uniffi.ai_terminal_mobile.RemoteTerminal
import java.io.File
import java.util.concurrent.Callable
import java.util.concurrent.ExecutionException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Opt-in, normal saved-login UI test. No login/logout, fake RPC, shell input or host file changes.
 * files/recent-directory-fixture.json: {valid_directory, invalid_directory, timeout_seconds}.
 * Coordinator seeds both canonical absolute paths in the connected Desktop's MRU, then deletes
 * only invalid_directory. No concurrent session creation/account switching during this test.
 * Creation/cancellation use production controls. Only IDs returned to this UI after our own
 * create clicks may be closed; cleanup uses a separate connection to preserve App selection.
 */
class RecentDirectoriesUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val reader = Executors.newSingleThreadExecutor()
    private lateinit var activity: MainActivity
    private lateinit var remote: RemoteTerminal
    private lateinit var account: Account
    private var deadline = 0L
    private var originalIdentity: List<String>? = null
    private var desktop = ""
    private var originalSelected: String? = null
    private var baseline: List<RemoteSession>? = null
    private var pendingBefore: Set<String>? = null
    private val owned = linkedSetOf<String>()
    private val report = JSONObject().put("passed", false).put("real_encrypted_rpc", true)
    private val screenshots = JSONArray()

    private fun <T> main(action: () -> T): T {
        var result: T? = null; var failure: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (error: Throwable) { failure = error } }
        failure?.let { throw it }
        @Suppress("UNCHECKED_CAST") return result as T
    }
    private fun member(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun dialog() = member("createDialog") as? AlertDialog
    private fun dialogViews() = all(dialog()!!.window!!.decorView).filter { it.isShown }
    private fun field() = dialogViews().filterIsInstance<EditText>().single()
    private fun waitFor(label: String, predicate: () -> Boolean) {
        while (SystemClock.elapsedRealtime() < deadline) {
            if (predicate()) return
            Thread.sleep(100)
        }
        throw AssertionError("Timed out: $label")
    }
    private fun <T> read(label: String, action: () -> T): T {
        val remaining = deadline - SystemClock.elapsedRealtime()
        assertTrue("Deadline exhausted: $label", remaining > 0)
        val future = reader.submit(Callable { action() })
        try { return future.get(minOf(remaining, 20000L), TimeUnit.MILLISECONDS) }
        catch (error: ExecutionException) { throw error.cause ?: error }
        catch (error: Exception) { future.cancel(true); throw AssertionError("RPC failed: $label", error) }
    }
    private fun identity(): List<String> {
        val device = read("current mobile identity") { account.devices().single { it.current }.id }
        return main { listOf(member("serverUrl") as String, member("accountName") as String, device) }
    }
    private fun connection() = main {
        assertEquals("Desktop changed", desktop, member("deviceId"))
        assertEquals("Account changed", originalIdentity!![1], member("accountName"))
        assertEquals("Server changed", originalIdentity!![0], member("serverUrl"))
        assertEquals("Desktop disconnected", true, member("connected"))
    }
    private fun sessions(): List<RemoteSession> { connection(); return read("sessions") { remote.sessions() } }
    private fun recent(): List<String> { connection(); return read("recent directories") { remote.recentDirectories() } }
    private fun ids(rows: List<RemoteSession>) = rows.map { it.id }.toSet()
    private fun click(description: String) = main {
        assertTrue(views().single { it.contentDescription?.toString() == description && it.isEnabled }.performClick())
    }
    private fun drainUi() {
        val worker = main { member("worker") as ExecutorService }
        // Creation's UI callback enqueues select on the same worker: drain both hops.
        repeat(2) {
            val barrier = java.util.concurrent.CountDownLatch(1)
            worker.execute { activity.runOnUiThread { barrier.countDown() } }
            assertTrue("Queued creation did not settle; refusing speculative cleanup", barrier.await(20, TimeUnit.SECONDS))
            instrumentation.waitForIdleSync()
        }
    }
    private fun screenshot(name: String) {
        instrumentation.waitForIdleSync()
        assertTrue("App/dialog lost foreground", main { (dialog()?.window?.decorView ?: activity.window.decorView).hasWindowFocus() })
        val bitmap = instrumentation.uiAutomation.takeScreenshot() ?: error("Screenshot unavailable")
        val file = File(context.filesDir, "recent-directory-$name.png")
        try { file.outputStream().use { assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) } }
        finally { bitmap.recycle() }
        screenshots.put(file.name)
    }
    private fun openCreation() {
        connection()
        waitFor("idle workspace") { main { member("entryPending") == false && member("connecting") == false && member("sessionBusy") == false } }
        if (!main { views().any { it.contentDescription == "关闭工作空间" } }) click("打开工作空间")
        click("新建会话")
        waitFor("recent rows loaded") { main {
            dialog()?.isShowing == true && dialogViews().filterIsInstance<TextView>().any { it.text.toString() == "选择后可编辑，再点击创建" }
        } }
        assertLayout("normal")
    }
    private fun pathRows(path: String) = dialogViews().filterIsInstance<TextView>().filter { it !is EditText && it.text.toString() == path }
    private fun choose(path: String) {
        main {
            val label = pathRows(path).single()
            val row = label.parent.parent as View
            assertTrue("Recent item must be a production clickable row", row.isClickable)
            row.requestRectangleOnScreen(Rect(0, 0, row.width, row.height), true)
        }
        instrumentation.waitForIdleSync()
        main { assertTrue((pathRows(path).single().parent.parent as View).performClick()) }
        main {
            assertEquals("Selecting a row must fill the exact path", path, field().text.toString())
            field().requestRectangleOnScreen(Rect(0, 0, field().width, field().height), true)
        }
        instrumentation.waitForIdleSync()
    }
    private fun button(which: Int) = main {
        val button = dialog()!!.getButton(which)
        assertTrue("Dialog button disabled", button.isEnabled)
        assertTrue(button.performClick())
    }
    private fun assertLayout(label: String) = main {
        val window = dialog()!!.window!!; val decor = window.decorView
        assertEquals("Small dialogs must wrap content", ViewGroup.LayoutParams.WRAP_CONTENT, window.attributes.height)
        val visible = Rect(); decor.getWindowVisibleDisplayFrame(visible)
        val metrics = JSONObject().put("visible_height", visible.height()).put("dialog_height", decor.height)
        for ((name, which) in listOf("create" to AlertDialog.BUTTON_POSITIVE, "cancel" to AlertDialog.BUTTON_NEGATIVE)) {
            val button = dialog()!!.getButton(which); val xy = IntArray(2); button.getLocationOnScreen(xy)
            val rect = Rect(xy[0], xy[1], xy[0] + button.width, xy[1] + button.height)
            assertTrue("$label: $name button clipped by screen/IME", button.isShown && visible.contains(rect))
            metrics.put(name + "_bounds", rect.toShortString())
        }
        val positive = dialog()!!.getButton(AlertDialog.BUTTON_POSITIVE)
        val bottom = IntArray(2); positive.getLocationOnScreen(bottom)
        val origin = IntArray(2); decor.getLocationOnScreen(origin)
        assertTrue("Blank space below standard buttons", origin[1] + decor.height - bottom[1] - positive.height <= activity.dp(64))
        val scroll = dialogViews().filterIsInstance<ScrollView>().single()
        if (scroll.getChildAt(0).height > scroll.height) {
            assertTrue("Long content must scroll", scroll.canScrollVertically(1) || scroll.canScrollVertically(-1))
            scroll.scrollTo(0, scroll.getChildAt(0).height)
            assertTrue("Last recent row cannot be reached", scroll.scrollY > 0 && !scroll.canScrollVertically(1))
            scroll.scrollTo(0, 0)
        }
        report.put("layout_$label", metrics)
    }
    private fun imeVisible(): Boolean {
        val decor = dialog()!!.window!!.decorView
        if (Build.VERSION.SDK_INT >= 30) return decor.rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == true
        val visible = Rect(); decor.getWindowVisibleDisplayFrame(visible)
        return activity.resources.displayMetrics.heightPixels - visible.bottom > activity.dp(100)
    }
    private fun hideKeyboard() {
        main {
            val input = activity.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as InputMethodManager
            input.hideSoftInputFromWindow(field().windowToken, 0); field().clearFocus()
        }
        waitFor("IME hidden") { main { !imeVisible() } }
    }
    private fun checkKeyboardLayout() {
        hideKeyboard()
        val before = main { Rect().also { dialog()!!.window!!.decorView.getWindowVisibleDisplayFrame(it) }.height() }
        main {
            assertTrue(field().requestFocus())
            val input = activity.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as InputMethodManager
            input.showSoftInput(field(), InputMethodManager.SHOW_IMPLICIT)
        }
        var lastHeight = -1; var stable = 0
        waitFor("IME visible above dialog with settled layout") { main {
            val visible = Rect(); dialog()!!.window!!.decorView.getWindowVisibleDisplayFrame(visible)
            stable = if (visible.height() == lastHeight) stable + 1 else 0
            lastHeight = visible.height()
            imeVisible() && stable >= 3 && visible.height() < before - activity.dp(80)
        } }
        assertLayout("ime"); screenshot("ime")
        hideKeyboard()
        waitFor("IME dismissed") { main {
            val visible = Rect(); dialog()!!.window!!.decorView.getWindowVisibleDisplayFrame(visible)
            visible.height() >= before - activity.dp(8)
        } }
    }
    private fun observeCreated(): String {
        val before = pendingBefore ?: error("No UI creation in flight")
        var created = ""
        waitFor("new Session selected by production creation") {
            main {
                val selected = member("selected") as? String
                if (dialog() == null && member("entryPending") == false && selected != null && selected !in before) {
                    created = selected; owned.add(selected); true
                } else false
            }
        }
        pendingBefore = null
        return created
    }
    private fun createValid(path: String, number: Int) {
        choose(path); screenshot("selected-$number")
        val before = ids(sessions()); pendingBefore = before
        button(AlertDialog.BUTTON_POSITIVE)
        val id = observeCreated()
        val after = sessions()
        assertEquals("Each UI click must create exactly one Session", setOf(id), ids(after) - before)
        assertTrue("Creation removed an existing Session", ids(after).containsAll(before))
        val actual = after.single { it.id == id }
        assertEquals("SessionInfo.cwd differs from canonical fixture", path, actual.cwd)
        val observed = read("OS cwd") { JSONObject(remote.agent(id, "{\"version\":1,\"action\":\"context\"}")) }
        assertTrue("Desktop process observation unavailable", observed.getBoolean("available") && !observed.isNull("cwd"))
        assertEquals("Real Desktop process cwd differs", actual.cwd, observed.getString("cwd"))
        val paths = recent()
        assertEquals("MRU must not duplicate the same canonical directory", 1, paths.count { it == path })
        assertEquals("Most recently used directory must lead", path, paths.first())
        report.put("creation_$number", JSONObject().put("session", id).put("cwd", actual.cwd).put("observed_cwd", observed.getString("cwd")).put("mru", JSONArray(paths)))
        screenshot("success-$number")
    }

    @Test fun recentDirectoriesCreateThroughNormalWorkspace() {
        val file = File(context.filesDir, "recent-directory-fixture.json")
        assumeTrue("Optional recent-directory-fixture.json is absent", file.isFile)
        val started = SystemClock.elapsedRealtime()
        var failure: Throwable? = null
        try {
            assertTrue("Fixture exceeds 64 KiB", file.length() <= 65536)
            val fixture = JSONObject(file.readText())
            val valid = fixture.getString("valid_directory"); val invalid = fixture.getString("invalid_directory")
            val seconds = fixture.optLong("timeout_seconds", 180)
            require(valid.startsWith('/') && invalid.startsWith('/') && valid.length > 1 && invalid.length > 1 && valid != invalid && seconds in 30L..900L)
            require(fixture.keys().asSequence().toSet() == setOf("valid_directory", "invalid_directory", "timeout_seconds")) { "Fixture must contain only the three noncredential fields" }
            deadline = started + seconds * 1000
            report.put("valid_directory", valid).put("invalid_directory", invalid).put("timeout_seconds", seconds)
            context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT))
            waitFor("normal saved-login MainActivity") { main {
                ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>()
                    .firstOrNull { it.hasWindowFocus() }?.also { activity = it } != null && member("entryPending") == false && member("loginBusy") == false && member("connecting") == false
            } }
            main {
                for (extra in listOf("isolated_ui", "terminal_input_test", "acceptance_test", "render_fixture")) assertFalse("Normal Activity required", activity.intent.getBooleanExtra(extra, false))
                assertTrue("Saved login required; harness never logs in", (member("accountName") as String).isNotBlank())
                assertEquals("Connect intended Desktop before running", true, member("connected"))
                assertNull("Close other overlays before running", member("overlay"))
                account = member("account") as Account; remote = member("remote") as RemoteTerminal
                desktop = member("deviceId") as String; originalSelected = member("selected") as? String
            }
            originalIdentity = identity()
            baseline = sessions()
            report.put("sessions_before", JSONArray(baseline!!.map { it.id }))
            assertTrue("Need room for two test sessions", baseline!!.size <= 14)
            assertTrue("Start on an existing attached live Session so UI selection can be restored safely",
                baseline!!.any { it.id == originalSelected && it.desktopAttached && !it.exited })
            val seeded = recent()
            assertTrue("Coordinator must seed both canonical paths", valid in seeded && invalid in seeded)
            openCreation(); screenshot("normal"); checkKeyboardLayout()
            choose(invalid); screenshot("invalid-selected")
            val beforeInvalid = ids(sessions()); pendingBefore = beforeInvalid
            button(AlertDialog.BUTTON_POSITIVE)
            waitFor("Desktop rejection rendered with reusable buttons") { main {
                dialog()?.isShowing == true && !field().error.isNullOrBlank() && field().isEnabled &&
                    dialog()!!.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled && dialog()!!.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled
            } }
            drainUi()
            assertEquals("Rejected directory must not create a Session", beforeInvalid, ids(sessions()))
            pendingBefore = null
            main {
                assertEquals("Failed path must remain editable", invalid, field().text.toString())
                val error = field().error.toString()
                assertTrue("Expected Desktop directory validation error, not a transport/UI error", error.contains("working directory is unavailable"))
                report.put("directory_error", error)
            }
            report.put("invalid_rejected_without_session", true)
            assertLayout("rejected"); screenshot("rejected")
            createValid(valid, 1)
            openCreation(); createValid(valid, 2)
            openCreation()
            main {
                assertEquals("UI must render one valid MRU row", 1, pathRows(valid).size)
                val renderedPaths = dialogViews().filterIsInstance<TextView>().filter { it !is EditText && it.text.toString().startsWith('/') }
                assertEquals("UI must keep the latest directory first", valid, renderedPaths.first().text.toString())
            }
            choose(valid)
            val beforeCancel = ids(sessions())
            screenshot("cancel-prepared"); button(AlertDialog.BUTTON_NEGATIVE); drainUi()
            assertEquals("Cancel must not create a Session", beforeCancel, ids(sessions()))
            report.put("cancel_without_session", true)
        } catch (error: Throwable) { failure = error }
        finally {
            deadline = SystemClock.elapsedRealtime() + 60000
            if (::activity.isInitialized && originalIdentity != null) {
                try {
                    drainUi(); connection()
                    // If an assertion interrupted a successful click, only its newly selected ID is owned.
                    pendingBefore?.let { before -> main {
                        val selected = member("selected") as? String
                        if (dialog() == null && selected != null && selected !in before) owned.add(selected)
                    } }
                    if (failure != null) runCatching { screenshot("failed") }.onFailure { report.put("screenshot_error", it.toString()) }
                    main { dialog()?.dismiss() }
                    if (main { views().any { it.contentDescription == "关闭工作空间" } }) click("关闭工作空间")
                    // Restore the original visible Session via its production workspace row before cleanup.
                    originalSelected?.let { original ->
                        if (main { member("selected") != original } && sessions().any { it.id == original && it.desktopAttached && !it.exited }) {
                            click("打开工作空间")
                            main { assertTrue(views().single { it.tag == original && it.isClickable }.performClick()) }
                            waitFor("original Session restored") { main { member("selected") == original && member("entryPending") == false } }
                        }
                    }
                    if (owned.isNotEmpty()) {
                        assertTrue("Refuse cleanup of pre-existing sessions", owned.intersect(ids(baseline!!)).isEmpty())
                        read("close only test-created sessions") {
                            val cleanup = RemoteTerminal()
                            try {
                                account.connect(desktop, cleanup)
                                for (id in owned) {
                                    connection()
                                    if (cleanup.sessions().any { it.id == id }) { cleanup.select(id, false); cleanup.closeSelected() }
                                }
                            } finally { runCatching { cleanup.disconnect() }; cleanup.close() }
                        }
                    }
                    val remaining = sessions()
                    baseline?.let { report.put("unclaimed_new_session_ids", JSONArray((ids(remaining) - ids(it) - owned).toList())) }
                    assertTrue("Test-created sessions remain", owned.intersect(ids(remaining)).isEmpty())
                    for (old in baseline.orEmpty()) {
                        val preserved = remaining.single { it.id == old.id }
                        assertEquals("Existing Session cwd changed", old.cwd, preserved.cwd)
                        assertEquals("Existing Session exited", old.exited, preserved.exited)
                    }
                    assertEquals("Normal mobile account/device identity changed", originalIdentity, identity())
                    report.put("account_identity_preserved", true).put("existing_sessions_preserved", true)
                        .put("closed_test_sessions", JSONArray(owned.toList())).put("sessions_after", JSONArray(remaining.map { it.id }))
                    // Refresh cached rows through the normal workspace after out-of-band cleanup.
                    click("打开工作空间"); click("刷新会话"); drainUi(); click("关闭工作空间")
                } catch (error: Throwable) {
                    report.put("cleanup_error", error.toString())
                    if (failure == null) failure = error else failure.addSuppressed(error)
                }
            }
            reader.shutdownNow()
            failure?.let { report.put("error", it.toString()) }
            report.put("passed", failure == null).put("elapsed_ms", SystemClock.elapsedRealtime() - started).put("screenshots", screenshots)
            File(context.filesDir, "recent-directory-results.json").writeText(report.toString(2))
        }
        failure?.let { throw it }
    }
}
