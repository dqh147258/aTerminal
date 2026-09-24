package dev.aiterminal.app

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Rect
import android.os.Handler
import android.os.Build
import android.os.Looper
import android.os.SystemClock
import android.view.PixelCopy
import android.view.View
import android.view.ViewTreeObserver
import android.widget.LinearLayout
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import uniffi.ai_terminal_mobile.*
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class DisplayConsistencyTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private fun main(action: () -> Unit) {
        var failure: Throwable? = null
        instrumentation.runOnMainSync { try { action() } catch (e: Throwable) { failure = e } }
        failure?.let { throw it }
    }
    private fun cell(text: String, width: UInt = 1u, style: UInt = 0u) = RenderCell(text, width, 0xeeeeeeu, 0x101014u, style)
    private fun screen(revision: ULong = 1uL) = RenderFrame(2u, 4u, revision,
        listOf(cell("a"), cell("中", 2u), cell("", 0u), cell("b"), cell("e\u0301"), cell(" "), cell("x"), cell("y")), 0u, 0u, true, 0u)
    private fun update(frame: RenderFrame, full: Boolean, patches: List<RenderPatch>, epoch: ULong = 1uL, generation: ULong = 1uL) = RenderUpdate(
        generation, epoch, frame.revision, frame.rows, frame.cols, full, patches,
        frame.cursorRow, frame.cursorCol, frame.cursorVisible, frame.cursorShape)
    private fun snapshot(frame: RenderFrame, epoch: ULong = 1uL) = update(frame, true, frame.cells.mapIndexed { i, c -> RenderPatch(i.toUInt(), c) }, epoch)
    private fun pixels(view: TerminalView): Bitmap {
        view.measure(View.MeasureSpec.UNSPECIFIED, View.MeasureSpec.UNSPECIFIED)
        view.layout(0, 0, view.measuredWidth, view.measuredHeight)
        return Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888).also { view.draw(Canvas(it)) }
    }
    @Test fun incrementalAndFullFramesHaveIdenticalPixelsAcrossCursorStylesZoomAndResize() = main {
        val first = screen()
        val changed = first.cells.toMutableList().apply { this[0] = cell("Z", style = 15u); this[6] = cell("q", style = 16u) }
        val incremental = TerminalView(instrumentation.targetContext, first)
        incremental.apply(snapshot(first))
        for (shape in 0u..3u) {
            val next = first.copy(revision = shape.toULong() + 2uL, cells = changed, cursorRow = 1u, cursorCol = 3u, cursorShape = shape)
            incremental.apply(update(next, false, listOf(RenderPatch(0u, changed[0]), RenderPatch(6u, changed[6]))))
            val reference = TerminalView(instrumentation.targetContext, next)
            incremental.zoom(1.2f); reference.zoom(1.2f)
            assertTrue("Incremental pixels differ, cursor $shape", pixels(incremental).sameAs(pixels(reference)))
        }
        val resized = RenderFrame(1u, 2u, 8uL, listOf(cell("1"), cell("2")), 0u, 1u, true, 1u)
        incremental.apply(snapshot(resized))
        val reference = TerminalView(instrumentation.targetContext, resized).apply { zoom(1.2f) }
        assertTrue(pixels(incremental).sameAs(pixels(reference)))
    }
    @Test fun invalidBatchCannotPartiallyChangeTheScreenAndNewSessionRequiresReset() = main {
        val first = screen()
        val view = TerminalView(instrumentation.targetContext, first)
        view.apply(snapshot(first))
        val before = pixels(view)
        val next = first.copy(revision = 2uL)
        val invalid = listOf(
            update(next, false, listOf(RenderPatch(0u, cell("z")), RenderPatch(9u, cell("!")))),
            update(next, false, listOf(RenderPatch(1u, cell("z")), RenderPatch(0u, cell("!")))),
            update(next, false, listOf(RenderPatch(0u, cell("z"))), epoch = 2uL),
            update(next, true, listOf(RenderPatch(0u, cell("z"))))
        )
        for (batch in invalid) {
            try { view.apply(batch); fail("Invalid batch accepted") } catch (_: IllegalArgumentException) {}
            assertTrue("Rejected batch changed pixels", before.sameAs(pixels(view)))
        }
        val another = first.copy(cells = List(8) { cell("n") })
        view.update(another)
        view.apply(snapshot(another, epoch = 2uL))
        assertTrue(pixels(view).sameAs(pixels(TerminalView(instrumentation.targetContext, another))))
        assertEquals("The caller's immutable initial snapshot was mutated", "a", first.cells[0].text)
    }
    @Test fun hardwareRowCacheMatchesAFullRedraw() {
        assumeTrue("RenderNode cache requires API 29", Build.VERSION.SDK_INT >= 29)
        instrumentation.uiAutomation.executeShellCommand("am start -W -n dev.aiterminal.app/.MainActivity --ez isolated_ui true").use { fd -> java.io.FileInputStream(fd.fileDescriptor).use { it.readBytes() } }
        var activity: MainActivity? = null
        val deadline = SystemClock.elapsedRealtime() + 10_000
        while (activity == null && SystemClock.elapsedRealtime() < deadline) {
            main { activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull() }
            if (activity == null) Thread.sleep(30)
        }
        val host = activity ?: throw AssertionError("No resumed AI Terminal Activity")
        lateinit var incremental: TerminalView
        lateinit var reference: TerminalView
        try {
            val first = screen()
            main {
                assertEquals("dev.aiterminal.app", host.packageName)
                incremental = TerminalView(host, first)
                reference = TerminalView(host, first)
                val column = LinearLayout(host).apply { orientation = LinearLayout.VERTICAL; addView(incremental); addView(reference) }
                host.setContentView(column)
                incremental.apply(snapshot(first))
            }
            fun capture(view: TerminalView): Bitmap {
                val draw = CountDownLatch(1)
                val listener = ViewTreeObserver.OnDrawListener { draw.countDown() }
                main { view.viewTreeObserver.addOnDrawListener(listener); view.invalidate() }
                assertTrue("View did not draw", draw.await(5, TimeUnit.SECONDS))
                lateinit var image: Bitmap
                val rect = Rect()
                main {
                    assertTrue("Expected hardware rendering", view.isHardwareAccelerated)
                    view.viewTreeObserver.removeOnDrawListener(listener)
                    val position = IntArray(2); view.getLocationInWindow(position)
                    rect.set(position[0], position[1], position[0] + view.width, position[1] + view.height)
                    image = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
                }
                val copied = CountDownLatch(1); var outcome = -1
                PixelCopy.request(host.window, rect, image, { outcome = it; copied.countDown() }, Handler(Looper.getMainLooper()))
                assertTrue(copied.await(5, TimeUnit.SECONDS)); assertEquals(PixelCopy.SUCCESS, outcome)
                return image
            }
            val before = capture(incremental)
            for (shape in 0u..3u) {
                val next = first.copy(revision = shape.toULong() + 2uL, cells = first.cells.toMutableList().apply { this[0] = cell("Z", style = 15u); this[6] = cell("q", style = 16u) }, cursorRow = 1u, cursorCol = 3u, cursorShape = shape)
                main { incremental.apply(update(next, false, listOf(RenderPatch(0u, next.cells[0]), RenderPatch(6u, next.cells[6])))); reference.update(next) }
                val actual = capture(incremental)
                assertFalse("The changed screen was not rendered", before.sameAs(actual))
                assertTrue("Hardware cached rows differ from full redraw", actual.sameAs(capture(reference)))
            }
        } finally { main { host.finish() } }
    }
}
