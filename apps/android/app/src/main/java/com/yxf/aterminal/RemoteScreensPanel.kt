package com.yxf.aterminal

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Base64
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import java.text.DateFormat
import java.util.Date

internal sealed class RemoteScreenResult {
    data class Screens(val screens: List<RemoteScreen>) : RemoteScreenResult()
    data class Frame(val bitmap: Bitmap, val capturedAt: Long) : RemoteScreenResult()
}

/** Native read-only overlay. It never selects a terminal or obtains terminal control. */
internal class RemoteScreensPanel(
    private val context: Context,
    body: LinearLayout,
    private val requests: RemoteScreenRequests<RemoteScreenResult>,
    private val desktopName: String,
    private val connected: () -> Boolean,
    private val listScreens: () -> String,
    private val screenFrame: (String, UInt) -> String,
    private val selectDevice: () -> Unit
) {
    private val content = context.column(12).apply { tag = "remote-screens.content" }
    private var selected: RemoteScreen? = null
    private var image: ImageView? = null
    private var status: TextView? = null
    private var retry: View? = null
    private var closed = false

    init { body.grow(content); showList() }

    fun back(): Boolean {
        if (selected == null) return false
        showList()
        return true
    }

    fun close() {
        closed = true; requests.stop(); releaseImage()
    }

    fun connectionInterrupted(message: String = "连接暂时中断，恢复后继续查看") {
        if (closed) return
        requests.stop(); releaseImage()
        status?.text = message
        retry?.visibility = View.GONE
    }

    fun connectionRestored() {
        if (closed || !connected()) return
        selected?.let { showScreen(it) } ?: showList()
    }

    private fun clear() {
        requests.stop(); releaseImage(); image = null; status = null; retry = null
        content.removeAllViews()
    }

    private fun releaseImage() {
        image?.setImageDrawable(null)
        // A displayed Bitmap may still be referenced by a RenderThread display list. Let Android
        // reclaim it after detaching; only undisplayed, stale decode results are explicitly recycled.
    }

    private fun showList() {
        if (closed) return
        clear(); selected = null
        if (!connected()) {
            status = context.label("请先连接 Desktop，再查看它的远程屏幕。", 14f, Palette.muted)
            content.addView(status)
            content.addView(context.actionButton("选择设备", true, selectDevice))
            return
        }
        content.addView(context.row().apply {
            fill(context.label(desktopName, 14f, Palette.secondary))
            addView(context.iconButton(R.drawable.ic_refresh_cw, "刷新显示器列表") { showList() })
        })
        status = context.label("正在加载显示器…", 14f, Palette.muted).apply { accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE }
        content.addView(status)
        val rows = context.column()
        content.grow(context.scroll(rows))
        retry = context.actionButton("重试") { showList() }.apply { visibility = View.GONE }
        content.addView(retry)
        requests.start(work = { current ->
            check(current() && connected()) { "设备连接已变化" }
            val result = RemoteScreensProtocol.screens(listScreens())
            check(current() && connected()) { "设备连接已变化" }
            RemoteScreenResult.Screens(result)
        }) { result ->
            if (closed || !connected()) return@start
            result.fold(onSuccess = { value ->
                val screens = (value as RemoteScreenResult.Screens).screens
                status?.text = if (screens.isEmpty()) "此 Desktop 暂无可用显示器，可刷新重试。" else "${screens.size} 个显示器 · 选择一个查看"
                screens.forEach { screen ->
                    rows.addView(context.settingsRow(screen.name, screen.details, R.drawable.ic_monitor_smartphone) { showScreen(screen) }.apply {
                        tag = "remote-screen.${screen.id}"
                        contentDescription = "${screen.name}，${screen.details}，查看远程屏幕"
                    })
                    rows.gap(8)
                }
                retry?.visibility = if (screens.isEmpty()) View.VISIBLE else View.GONE
            }, onFailure = { error ->
                status?.text = "显示器列表加载失败：${RemoteScreensErrors.describe(error)}"
                retry?.visibility = View.VISIBLE
            })
        }
    }

    private fun showScreen(screen: RemoteScreen) {
        if (closed) return
        clear(); selected = screen
        content.addView(context.row().apply {
            addView(context.iconButton(R.drawable.ic_arrow_left, "返回显示器列表") { showList() })
            fill(context.column(8).apply {
                addView(context.label(screen.name, 16f).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END })
                addView(context.label(screen.details, 12f, Palette.muted))
            })
            addView(context.actionButton("切换") { showList() })
        })
        val viewport = FrameLayout(context).apply { setBackgroundColor(android.graphics.Color.BLACK) }
        image = ImageView(context).apply {
            scaleType = ImageView.ScaleType.FIT_CENTER
            contentDescription = "${screen.name}的远程屏幕画面，仅查看"
            tag = "remote-screens.image"
        }
        viewport.addView(image, FrameLayout.LayoutParams(-1, -1, Gravity.CENTER))
        content.grow(viewport)
        status = context.label("正在获取屏幕画面…", 12f, Palette.muted)
        content.addView(status)
        retry = context.actionButton("重试") { showScreen(screen) }.apply { visibility = View.GONE }
        content.addView(retry)
        val maxWidth = context.resources.displayMetrics.widthPixels.coerceIn(640, 1920).toUInt()
        requests.start(intervalMillis = 1_000L, work = { current ->
            check(current() && connected()) { "设备连接已变化" }
            val frame = RemoteScreensProtocol.frame(screenFrame(screen.id, maxWidth), screen.id)
            check(current() && connected()) { "设备连接已变化" }
            val bytes = Base64.decode(frame.imageBase64, Base64.DEFAULT)
            require(bytes.isNotEmpty() && bytes.size <= RemoteScreensProtocol.MAX_JPEG_BYTES) { "屏幕图片数据超出限制" }
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            require(bounds.outMimeType == "image/jpeg" && bounds.outWidth == frame.width && bounds.outHeight == frame.height &&
                bounds.outWidth in 1..RemoteScreensProtocol.MAX_FRAME_EDGE &&
                bounds.outHeight in 1..RemoteScreensProtocol.MAX_FRAME_EDGE) { "屏幕图片无法解码或尺寸无效" }
            check(current() && connected()) { "屏幕请求已取消" }
            val decoded = BitmapFactory.decodeByteArray(bytes, 0, bytes.size) ?: error("屏幕图片无法解码")
            RemoteScreenResult.Frame(decoded, frame.capturedAt)
        }) { result ->
            if (closed || !connected()) {
                (result.getOrNull() as? RemoteScreenResult.Frame)?.bitmap?.recycle()
                requests.stop()
                return@start
            }
            result.fold(onSuccess = { value ->
                val frame = value as RemoteScreenResult.Frame
                image?.setImageBitmap(frame.bitmap)
                status?.text = "仅查看 · 更新于 ${DateFormat.getTimeInstance().format(Date(frame.capturedAt))}"
                retry?.visibility = View.GONE
            }, onFailure = { error ->
                // A persistent capture error must not generate an endless retry flood.
                requests.stop()
                status?.text = "屏幕画面获取失败：${RemoteScreensErrors.describe(error)}"
                retry?.visibility = View.VISIBLE
            })
        }
    }
}
