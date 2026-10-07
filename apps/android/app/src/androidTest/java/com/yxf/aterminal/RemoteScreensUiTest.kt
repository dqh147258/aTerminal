package com.yxf.aterminal

import android.graphics.Bitmap
import android.os.Looper
import android.util.Base64
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.ImageView
import android.widget.TextView
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.util.ArrayDeque
import uniffi.ai_terminal_mobile.CoreException
import uniffi.ai_terminal_mobile.RemoteTerminal

/** Isolated, in-memory production panel; no account data, real Desktop, or terminal writes. */
class RemoteScreensUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private fun <T> main(action: () -> T): T {
        var value: T? = null
        var failure: Throwable? = null
        instrumentation.runOnMainSync { try { value = action() } catch (error: Throwable) { failure = error } }
        failure?.let { throw it }
        @Suppress("UNCHECKED_CAST") return value as T
    }
    private fun all(view: View): List<View> = listOf(view) + if (view is ViewGroup) (0 until view.childCount).flatMap { all(view.getChildAt(it)) } else emptyList()

    @Test fun generatedScreenBindingsMatchThePackagedNativeLibrary() {
        val core = RemoteTerminal()
        try {
            assertThrows(CoreException::class.java) { core.remoteScreensJson() }
            assertThrows(CoreException::class.java) { core.remoteScreenFrameJson("screen-1", 640u) }
        } finally { core.close() }
    }
    private val screens = """{"screens":[{"id":"screen-1","name":"Primary","width":2560,"height":1600,"is_primary":true},{"id":"screen-2","name":"Portrait","width":1080,"height":1920,"is_primary":false}]}"""
    private fun jpegFrame(id: String, declaredWidth: Int = 8): String {
        val bitmap = Bitmap.createBitmap(8, 5, Bitmap.Config.ARGB_8888)
        val bytes = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.JPEG, 80, bytes); bitmap.recycle()
        return JSONObject().put("screen_id", id).put("mime_type", "image/jpeg").put("width", declaredWidth).put("height", 5)
            .put("captured_at_ms", 1000).put("image_base64", Base64.encodeToString(bytes.toByteArray(), Base64.NO_WRAP)).toString()
    }
    private inner class Fixture(initialConnected: Boolean = true) {
        val work = ArrayDeque<() -> Unit>()
        val ui = ArrayDeque<() -> Unit>()
        val timers = mutableListOf<() -> Unit>()
        var connected = initialConnected
        var list: () -> String = { screens }
        var frame: (String) -> String = { jpegFrame(it) }
        var frameCalls = 0
        var deviceChoices = 0
        val body = main { context.column() }
        val requests = RemoteScreenRequests<RemoteScreenResult>(
            { work.add(it) }, { ui.add(it) }, { _, callback ->
                timers.add(callback)
                val cancel: () -> Unit = { timers.remove(callback) }
                cancel
            }, { value -> if (value is RemoteScreenResult.Frame) value.bitmap.recycle() }
        )
        val panel = main { RemoteScreensPanel(context, body, requests, "Test Desktop", { connected },
            { assertNotEquals(Looper.getMainLooper(), Looper.myLooper()); list() },
            { id, _ -> assertNotEquals(Looper.getMainLooper(), Looper.myLooper()); frameCalls++; frame(id) },
            { deviceChoices++ }) }
        fun finish() { work.removeFirst()(); main { ui.removeFirst()() } }
        fun click(description: String) = main { assertTrue(all(body).first { it.contentDescription?.toString() == description }.performClick()) }
        fun select(id: String) = main { assertTrue(all(body).first { it.tag == "remote-screen.$id" }.performClick()) }
        fun text() = main { all(body).filterIsInstance<TextView>().joinToString("\n") { it.text.toString() } }
        fun image() = all(body).filterIsInstance<ImageView>().first { it.tag == "remote-screens.image" }
        fun close() = main { panel.close() }
    }

    @Test fun nativeListAndFitCenterViewerCanSwitchWithoutTerminalSelection() {
        val f = Fixture()
        try {
            assertTrue(f.text().contains("正在加载")); f.finish()
            assertTrue(f.text().contains("2560 × 1600 · 主屏")); assertTrue(f.text().contains("1080 × 1920"))
            f.select("screen-2"); f.finish()
            main { assertEquals(ImageView.ScaleType.FIT_CENTER, f.image().scaleType); assertNotNull(f.image().drawable) }
            assertEquals(1, f.frameCalls)
            main { assertTrue(f.panel.back()) }; assertTrue(f.timers.isEmpty()); f.finish()
            f.select("screen-1"); f.finish()
            assertEquals(2, f.frameCalls)
            f.click("返回显示器列表"); f.finish()
            main { assertFalse(f.panel.back()) }
        } finally { f.close() }
    }

    @Test fun closeAfterDecodeCannotRestoreTheImageOrScheduleAnotherFrame() {
        val f = Fixture()
        try {
            f.finish(); f.select("screen-1"); f.work.removeFirst()()
            f.close(); main { f.ui.removeFirst()() }
            main { assertNull(f.image().drawable) }
            assertTrue(f.timers.isEmpty()); assertTrue(f.work.isEmpty()); assertEquals(1, f.frameCalls)
        } finally { f.close() }
    }

    @Test fun interruptionDiscardsOldListAndARecoveredConnectionReloadsIt() {
        val f = Fixture()
        try {
            f.work.removeFirst()()
            main { f.connected = false; f.panel.connectionInterrupted("连接中断") }
            main { f.ui.removeFirst()() }
            assertEquals(0, main { all(f.body).count { it.tag == "remote-screen.screen-1" } })
            assertTrue(f.text().contains("连接中断"))
            main { f.connected = true; f.panel.connectionRestored() }; f.finish()
            assertTrue(f.text().contains("主屏"))
        } finally { f.close() }
    }

    @Test fun emptyListAndCapabilityFailureOfferRetryAndUpgradeGuidance() {
        val f = Fixture()
        try {
            f.list = { "{\"screens\":[]}" }; f.finish()
            assertTrue(f.text().contains("暂无可用显示器"))
            f.list = { error("remote_screens_unavailable") }
            f.click("刷新显示器列表"); f.finish()
            assertTrue(f.text().contains("升级 Desktop"))
            main { assertTrue(all(f.body).filterIsInstance<Button>().any { it.text == "重试" && it.visibility == View.VISIBLE }) }
        } finally { f.close() }
    }

    @Test fun invalidJpegDimensionsStopPollingAndAllowRetry() {
        val f = Fixture()
        try {
            f.frame = { jpegFrame(it, declaredWidth = 9) }
            f.finish(); f.select("screen-1"); f.finish()
            assertTrue(f.text().contains("尺寸无效")); assertTrue(f.timers.isEmpty())
            main { assertNull(f.image().drawable) }
            f.frame = { jpegFrame(it) }
            main { all(f.body).filterIsInstance<Button>().first { it.text == "重试" }.performClick() }; f.finish()
            main { assertNotNull(f.image().drawable) }
        } finally { f.close() }
    }

    @Test fun disconnectedEntryExplainsTheRequirementAndOffersDeviceSelection() {
        val f = Fixture(initialConnected = false)
        try {
            assertTrue(f.text().contains("请先连接 Desktop")); assertTrue(f.work.isEmpty())
            main { all(f.body).filterIsInstance<Button>().first { it.text == "选择设备" }.performClick() }
            assertEquals(1, f.deviceChoices); assertEquals(0, f.frameCalls)
        } finally { f.close() }
    }
}
