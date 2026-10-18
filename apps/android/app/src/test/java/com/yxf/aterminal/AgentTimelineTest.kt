package com.yxf.aterminal

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class AgentTimelineTest {
    @Test fun interruptedModelReplyIsDistinctFromPagingAndToolInteractions() {
        val interrupted = JSONObject("""{"kind":"interaction","value":{"source":"model_interrupted","interrupted":true,"state":"interrupted","text":"已收到的回答"}}""")
        assertTrue(AgentTimeline.interrupted(interrupted))
        assertEquals("已收到的回答", AgentTimeline.text(interrupted))
        assertFalse(interrupted.getJSONObject("value").optBoolean("partial"))
        assertFalse(AgentTimeline.interrupted(JSONObject("""{"kind":"interaction","value":{"partial":true,"record_id":"record-1"}}""")))
        assertFalse(AgentTimeline.interrupted(JSONObject("""{"kind":"interaction","value":{"tools":[{"name":"read_terminal"}]}}""")))
    }
}
