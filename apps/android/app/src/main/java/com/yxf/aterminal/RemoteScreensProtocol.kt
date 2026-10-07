package com.yxf.aterminal

import org.json.JSONObject

internal data class RemoteScreen(val id: String, val name: String, val width: Int, val height: Int, val primary: Boolean) {
    val details: String get() = "$width × $height" + if (primary) " · 主屏" else ""
}

internal data class RemoteScreenFrame(val imageBase64: String, val width: Int, val height: Int, val capturedAt: Long)

internal object RemoteScreensProtocol {
    const val MAX_FRAME_JSON_BYTES = 164 * 1024
    const val MAX_BASE64_CHARS = 160 * 1024
    const val MAX_JPEG_BYTES = 120 * 1024
    const val MAX_FRAME_EDGE = 1920
    const val MAX_DISPLAY_EDGE = 32768
    const val MAX_DISPLAYS = 64

    fun screens(json: String): List<RemoteScreen> {
        val rows = JSONObject(json).getJSONArray("screens")
        require(rows.length() <= MAX_DISPLAYS) { "显示器数量超出限制" }
        val ids = mutableSetOf<String>()
        return (0 until rows.length()).map { index ->
            val row = rows.getJSONObject(index)
            val id = row.getString("id")
            val width = row.getLong("width"); val height = row.getLong("height")
            require(id.isNotBlank() && ids.add(id) && width in 1L..MAX_DISPLAY_EDGE.toLong() && height in 1L..MAX_DISPLAY_EDGE.toLong()) { "显示器信息无效，请刷新列表" }
            RemoteScreen(id, row.getString("name").ifBlank { "显示器 ${index + 1}" }, width.toInt(), height.toInt(), row.getBoolean("is_primary"))
        }
    }

    fun frame(json: String, expectedScreen: String): RemoteScreenFrame {
        require(json.length <= MAX_FRAME_JSON_BYTES && json.toByteArray(Charsets.UTF_8).size <= MAX_FRAME_JSON_BYTES) { "屏幕图片数据超出限制" }
        val value = JSONObject(json)
        require(value.getString("screen_id") == expectedScreen) { "返回的屏幕已变化，请重新选择显示器" }
        require(value.getString("mime_type") == "image/jpeg") { "不支持的屏幕图片格式" }
        val width = value.getLong("width"); val height = value.getLong("height")
        val capturedAt = value.getLong("captured_at_ms")
        val image = value.getString("image_base64")
        require(width in 1L..MAX_FRAME_EDGE.toLong() && height in 1L..MAX_FRAME_EDGE.toLong() && capturedAt >= 0 &&
            image.isNotBlank() && image.length <= MAX_BASE64_CHARS) { "屏幕图片数据无效" }
        return RemoteScreenFrame(image, width.toInt(), height.toInt(), capturedAt)
    }
}

internal object RemoteScreensErrors {
    fun describe(error: Throwable): String {
        val detail = error.message.orEmpty()
        return when {
            detail.contains("remote_screens_unavailable", true) -> "当前 Desktop 不支持远程屏幕，请升级 Desktop 后重试。"
            detail.contains("unknown_screen", true) -> "显示器已断开，请返回显示器列表并刷新后重新选择。"
            detail.contains("wayland", true) && (detail.contains("unsupported", true) || detail.contains("not supported", true)) ->
                "当前暂不支持 Wayland 屏幕抓取，请在电脑切换至 X11 会话后重试。"
            detail.contains("busy", true) || detail.contains("timeout", true) || detail.contains("timed out", true) ->
                "Desktop 正忙或请求超时，请稍后重试。"
            detail.contains("screen_capture_permission_denied", true) || detail.contains("allow screen recording", true) ->
                "请在电脑的系统设置中允许屏幕录制，然后重试。"
            else -> detail.ifBlank { "请重试" }
        }
    }
}
