package com.yxf.aterminal

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class RemoteScreensProtocolTest {
    private val screen = """{"id":"display-1","name":"Built-in Display","width":2560,"height":1600,"is_primary":true}"""
    private fun frame() = JSONObject().put("screen_id", "display-1").put("mime_type", "image/jpeg")
        .put("width", 1280).put("height", 800).put("captured_at_ms", 1000L).put("image_base64", "aGVsbG8=")
    private fun rejected(action: () -> Unit) {
        try { action(); fail("Invalid protocol response accepted") } catch (_: Exception) {}
    }

    @Test fun allMonitorsKeepServerOrderAndPrimaryResolutionMetadata() {
        val second = JSONObject(screen).put("id", "display-2").put("is_primary", false).put("width", 1920).put("height", 1080)
        val screens = RemoteScreensProtocol.screens("{\"screens\":[$screen,$second]}")
        assertEquals(listOf("display-1", "display-2"), screens.map { it.id })
        assertEquals("2560 × 1600 · 主屏", screens.first().details)
        assertEquals("1920 × 1080", screens.last().details)
    }

    @Test fun emptyDisplaysAreDistinctFromMalformedOrDuplicateDisplays() {
        assertTrue(RemoteScreensProtocol.screens("{\"screens\":[]}").isEmpty())
        for (json in listOf("{}", "{\"screens\":null}", "{\"screens\":[$screen,$screen]}",
            "{\"screens\":[${JSONObject(screen).put("width", 0)}]}", "not-json")) {
            rejected { RemoteScreensProtocol.screens(json) }
        }
    }

    @Test fun frameMetadataBelongsToTheRequestedDisplay() {
        val parsed = RemoteScreensProtocol.frame(frame().toString(), "display-1")
        assertEquals(1280, parsed.width); assertEquals(800, parsed.height); assertEquals(1000L, parsed.capturedAt)
        rejected { RemoteScreensProtocol.frame(frame().toString(), "display-2") }
    }

    @Test fun unsupportedImagesAndInvalidFrameDimensionsTimestampsOrPayloadFail() {
        for (invalid in listOf(frame().put("mime_type", "image/png"), frame().put("width", -1),
            frame().put("height", 0), frame().put("width", 1921), frame().put("height", 1921), frame().put("width", 4294967297L),
            frame().put("captured_at_ms", -1), frame().put("image_base64", ""))) {
            rejected { RemoteScreensProtocol.frame(invalid.toString(), "display-1") }
        }
    }

    @Test fun boundedContractAcceptsLimitsAndRejectsOversizedResponses() {
        assertEquals(1920, RemoteScreensProtocol.frame(frame().put("width", 1920).put("height", 1920)
            .put("image_base64", "a".repeat(160 * 1024)).toString(), "display-1").width)
        rejected { RemoteScreensProtocol.frame(frame().put("image_base64", "a".repeat(160 * 1024 + 1)).toString(), "display-1") }
        rejected { RemoteScreensProtocol.frame(frame().put("padding", "中".repeat(56 * 1024)).toString(), "display-1") }
        val displays = (1..64).joinToString(",") { JSONObject(screen).put("id", "display-$it").toString() }
        assertEquals(64, RemoteScreensProtocol.screens("{\"screens\":[$displays]}").size)
        rejected { RemoteScreensProtocol.screens("{\"screens\":[$displays,${JSONObject(screen).put("id", "extra")}]}") }
        rejected { RemoteScreensProtocol.screens("{\"screens\":[${JSONObject(screen).put("width", 32769)}]}") }
        rejected { RemoteScreensProtocol.screens("{\"screens\":[${JSONObject(screen).put("width", 4294967297L)}]}") }
    }

    @Test fun knownCoreFailuresOfferActionableChineseGuidanceAndUnknownErrorsKeepDetails() {
        for ((detail, expected) in listOf("remote_screens_unavailable" to "升级 Desktop", "screen_capture_permission_denied" to "系统设置",
            "Please allow screen recording" to "屏幕录制", "unknown_screen" to "刷新", "capture busy" to "重试",
            "request timeout" to "重试", "capture timed out" to "重试", "Wayland unsupported" to "X11",
            "Wayland is not supported" to "X11")) {
            assertTrue(detail, RemoteScreensErrors.describe(IllegalStateException(detail)).contains(expected))
        }
        assertEquals("native capture failed: display 7", RemoteScreensErrors.describe(IllegalStateException("native capture failed: display 7")))
    }
}
