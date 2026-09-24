package dev.aiterminal.app

import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.util.UUID

class WorkspaceStateTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    @Test fun conversationStorageSeparatesServerAccountDeviceAndSession() {
        val name = "test-" + UUID.randomUUID()
        val owner = ChatStore(context, "https://one.invalid/", name)
        val otherAccount = ChatStore(context, "https://one.invalid", name + "-other")
        val otherServer = ChatStore(context, "https://two.invalid", name)
        try {
            val chat = owner.get("desktop", "session", "same-title")
            chat.messages.add(ChatMessage("user", "private-marker"))
            chat.draft = "unfinished"; chat.requestId = "request-1"; chat.state = "unknown"
            owner.save(chat)
            assertEquals("private-marker", ChatStore(context, "https://one.invalid", name).get("desktop", "session", "").messages.single().content)
            assertEquals("unfinished", owner.get("desktop", "session", "").draft)
            assertEquals("request-1", owner.get("desktop", "session", "").requestId)
            assertEquals("unknown", owner.get("desktop", "session", "").state)
            assertTrue(otherAccount.list().isEmpty()); assertTrue(otherServer.list().isEmpty())
            assertTrue(owner.get("other-desktop", "session", "same-title").messages.isEmpty())
            assertTrue(owner.get("desktop", "other-session", "same-title").messages.isEmpty())
            assertEquals(1, owner.list("PRIVATE-MARKER").size)
            assertTrue(owner.list("not-present").isEmpty())
        } finally { owner.clear(); otherAccount.clear(); otherServer.clear() }
    }

    @Test fun contextIsBoundedAndScreenIsOnlySentWhenExplicitlyRequested() {
        val history = (0..25).map { ChatMessage(if (it % 2 == 0) "user" else "assistant", "中".repeat(1000)) }
        val raw = AssistantRequest.send("id", "解释错误", false, history)
        val payload = JSONObject(raw)
        assertFalse(payload.getBoolean("include_screen"))
        assertTrue(payload.getJSONArray("messages").length() <= 12)
        assertTrue(raw.toByteArray(Charsets.UTF_8).size < 16000)
        assertEquals("解释错误", payload.getString("message"))
        assertTrue(JSONObject(AssistantRequest.send("id", "hello", true, emptyList())).getBoolean("include_screen"))
    }

    @Test fun invalidOrOversizedMessagesNeverProduceRequests() {
        for (message in listOf(" ", "a".repeat(4001), "\u0000".repeat(4000))) {
            try { AssistantRequest.send("id", message, false, emptyList()); fail("Invalid message accepted") }
            catch (_: IllegalArgumentException) {}
        }
    }

    @Test fun displayPreferencesPersistClampAndReset() {
        val prefs = DisplayPreferences(context, "acceptance-display")
        val originalFont = prefs.fontSize; val originalOpacity = prefs.opacity
        try {
            prefs.fontSize = 100; prefs.opacity = 0
            assertEquals(24, DisplayPreferences(context, "acceptance-display").fontSize); assertEquals(60, DisplayPreferences(context, "acceptance-display").opacity)
            prefs.fontSize = 0; prefs.opacity = 100
            assertEquals(12, prefs.fontSize); assertEquals(96, prefs.opacity)
            prefs.reset(); assertEquals(16, prefs.fontSize); assertEquals(88, prefs.opacity)
        } finally { prefs.fontSize = originalFont; prefs.opacity = originalOpacity }
    }

    @Test fun serverNormalizationRejectsCredentialsAndNonWebSchemes() {
        assertEquals("https://example.com", ChatStore.canonicalServer(" https://EXAMPLE.com/ "))
        for (server in listOf("example.com", "file:///tmp", "https://name:secret@example.com", "https://example.com?token=secret")) {
            try { ChatStore.canonicalServer(server); fail("Invalid server accepted") } catch (_: IllegalArgumentException) {}
        }
    }

    @Test fun eventCursorsPersistPerRequestAndNeverDuplicateReply() {
        val name = UUID.randomUUID().toString()
        val store = ChatStore(context, "https://events.invalid", name)
        try {
            val result = JSONObject("""{"available":true,"state":"monitoring","request_id":"r1","reply":"duplicate","monitoring":true,"events":[{"id":1,"kind":"input","text":"input accepted","revision":0},{"id":2,"kind":"observation","text":"screen changed","revision":42}]}""")
            store.response("d", "s", "title", result)
            store.response("d", "s", "title", result)
            val persisted = ChatStore(context, "https://events.invalid", name).get("d", "s", "")
            assertEquals(2, persisted.messages.size); assertEquals(2L, persisted.eventCursors["r1"])
            assertEquals(42L, persisted.messages.last().revision); assertTrue(persisted.monitoring)
            result.put("request_id", "r2")
            store.response("d", "s", "title", result)
            assertEquals(4, store.get("d", "s", "").messages.size)
            store.response("d", "s", "title", JSONObject("""{"state":"stopped","request_id":"r2","monitoring":false}"""), "r2")
            assertEquals("stopped", store.get("d", "s", "").state)
            store.response("d", "s", "title", JSONObject("""{"available":true,"state":"idle","request_id":""}"""))
            assertEquals("stopped", store.get("d", "s", "").state)
            store.response("d", "s", "title", result.put("request_id", "r1"), "r1")
            assertEquals("stopped", store.get("d", "s", "").state)
        } finally { store.clear() }
    }

    @Test fun lastWorkspaceAndConfirmedClosuresArePrivateAndPersistent() {
        val name = UUID.randomUUID().toString()
        val memory = WorkspaceMemory(context, "https://restore.invalid", name)
        val other = WorkspaceMemory(context, "https://restore.invalid", name + "-other")
        try {
            memory.remember("desktop", "session")
            memory.record("desktop", mapOf("session" to false, "gone" to false))
            memory.record("desktop", mapOf("session" to false))
            val restored = WorkspaceMemory(context, "https://restore.invalid", name)
            assertEquals("desktop" to "session", restored.last())
            assertTrue(restored.closed("desktop", "gone")); assertFalse(restored.closed("desktop", "session"))
            assertFalse(restored.closed("unqueried", "session")); assertNull(other.last())
        } finally { memory.clear(); other.clear() }
    }
}
