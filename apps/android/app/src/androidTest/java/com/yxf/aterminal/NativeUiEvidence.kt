package com.yxf.aterminal

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.Rect
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.PixelCopy
import android.view.View
import android.view.ViewTreeObserver
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/** Evidence comes from a committed native window frame, after checking its real viewport. */
internal object NativeUiEvidence {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private fun <T> main(action: () -> T): T {
        var result: Result<T>? = null
        instrumentation.runOnMainSync { result = runCatching(action) }
        return result!!.getOrThrow()
    }

    fun bounds(view: View): Rect {
        val position = IntArray(2); view.getLocationInWindow(position)
        return Rect(position[0], position[1], position[0] + view.width, position[1] + view.height)
    }

    fun assertFullyVisible(view: View) {
        val label = view.tag ?: view.contentDescription ?: view.javaClass.simpleName
        assertTrue("$label must be attached and shown", view.isAttachedToWindow && view.isShown && view.alpha > 0)
        assertTrue("$label must be laid out", view.width > 0 && view.height > 0 && !view.isLayoutRequested)
        val visible = Rect()
        assertTrue("$label must be inside the visible viewport", view.getGlobalVisibleRect(visible))
        assertEquals("$label is horizontally clipped", view.width, visible.width())
        assertEquals("$label is vertically clipped", view.height, visible.height())
    }

    fun awaitFrame(activity: Activity, verify: () -> Unit = {}) {
        val decor = activity.window.decorView
        val deadline = SystemClock.elapsedRealtime() + 8_000
        while (!main { activity.hasWindowFocus() && decor.isAttachedToWindow && decor.width > 0 && decor.height > 0 }) {
            assertTrue("The fixture window did not become visible", SystemClock.elapsedRealtime() < deadline)
            Thread.sleep(20)
        }
        val committed = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>()
        val callback = Runnable { committed.countDown() }
        lateinit var observer: ViewTreeObserver
        lateinit var listener: ViewTreeObserver.OnPreDrawListener
        main {
            observer = decor.viewTreeObserver
            listener = ViewTreeObserver.OnPreDrawListener {
                if (decor.isLayoutRequested) {
                    decor.postInvalidateOnAnimation()
                    return@OnPreDrawListener true
                }
                observer.removeOnPreDrawListener(listener)
                runCatching { verify() }.onFailure { failure.set(it) }
                if (Build.VERSION.SDK_INT >= 29 && decor.isHardwareAccelerated) {
                    observer.registerFrameCommitCallback(callback)
                } else {
                    // Older devices have no commit callback. Wait through two draw turns;
                    // PixelCopy still reads the window surface rather than drawing a View tree.
                    decor.postOnAnimation { decor.postOnAnimation(callback) }
                }
                true
            }
            observer.addOnPreDrawListener(listener)
            decor.invalidate()
        }
        try {
            assertTrue("The verified native frame did not commit", committed.await(8, TimeUnit.SECONDS))
            failure.get()?.let { throw it }
        } finally {
            main { if (observer.isAlive) {
                observer.removeOnPreDrawListener(listener)
                if (Build.VERSION.SDK_INT >= 29) observer.unregisterFrameCommitCallback(callback)
            } }
        }
    }

    fun capture(activity: Activity, path: String, verify: () -> Unit, pixels: (Bitmap) -> Unit = {}) {
        val verifiedFrame = runCatching { awaitFrame(activity, verify) }
        val bitmap = if (Build.VERSION.SDK_INT >= 26) {
            val bitmap = main { Bitmap.createBitmap(activity.window.decorView.width, activity.window.decorView.height, Bitmap.Config.ARGB_8888) }
            val copied = CountDownLatch(1); var result = -1
            PixelCopy.request(activity.window, bitmap, { result = it; copied.countDown() }, Handler(Looper.getMainLooper()))
            if (!copied.await(8, TimeUnit.SECONDS) || result != PixelCopy.SUCCESS) {
                bitmap.recycle(); fail("Native window copy failed: $result")
            }
            bitmap
        } else {
            instrumentation.uiAutomation.takeScreenshot().also { assertNotNull("Native screen capture unavailable", it) }
        }
        try {
            val verifiedPixels = runCatching { main { verify(); pixels(bitmap) } }
            activity.filesDir.resolve(path).apply { parentFile!!.mkdirs() }.outputStream().use {
                assertTrue("PNG encoding failed", bitmap.compress(Bitmap.CompressFormat.PNG, 100, it))
            }
            // Keep native evidence even if a viewport/color assertion fails, while preserving
            // that original failure as the test result for CI and visual review.
            verifiedFrame.getOrThrow(); verifiedPixels.getOrThrow()
        } finally { bitmap.recycle() }
    }
}
