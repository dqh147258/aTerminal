package com.yxf.aterminal

import android.content.Intent
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.*
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** Regressions found in the workspace review: cross-panel drafts and versioned history records. */
class WorkspaceReviewRegressionTest {
    val i = InstrumentationRegistry.getInstrumentation()
    val context = i.targetContext
    lateinit var activity: MainActivity
    fun <T> main(action: () -> T): T { var result: T? = null; var error: Throwable? = null
        i.runOnMainSync { try { result=action() } catch(e: Throwable) { error=e } }; error?.let { throw it }; @Suppress("UNCHECKED_CAST") return result as T }
    fun all(v: View): List<View> = listOf(v) + if(v is ViewGroup) (0 until v.childCount).flatMap { all(v.getChildAt(it)) } else emptyList()
    fun views() = all(activity.window.decorView).filter { it.isShown }
    fun input() = views().filterIsInstance<EditText>().first()
    fun body() = MainActivity::class.java.getDeclaredMethod("panel",String::class.java,Boolean::class.javaPrimitiveType).apply { isAccessible=true }.invoke(activity,"AI Agent",false) as LinearLayout
    fun waitFor(name:String, check:()->Boolean) { val end=SystemClock.elapsedRealtime()+12000; while(SystemClock.elapsedRealtime()<end) { if(check())return; Thread.sleep(20) }; fail(name) }
    fun launch() { context.startActivity(Intent(context,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui",true).putExtra("render_fixture",true))
        waitFor("activity") { main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull {it.hasWindowFocus()}?.also {activity=it} != null } } }
    @Test fun evidenceIndexPreservesParameterAndResultLabels() {
        val item=JSONObject().put("value",JSONObject()
            .put("records",JSONArray().put(JSONObject().put("record_id","old-call").put("source","tool_call")).put(JSONObject().put("record_id","new-result")))
            .put("updates",JSONArray().put(JSONObject().put("result_record_id","new-result"))))
        val evidence=AgentTimeline.evidence(item).associate {it.id to it.label}
        assertEquals(mapOf("old-call" to "调用参数", "new-result" to "执行结果"),evidence)
    }
    @Test fun reopeningDuringSendCannotRestoreAlreadyClearedDraft() {
        launch(); val identity=listOf("review",UUID.randomUUID().toString(),"desktop")
        val sent=CountDownLatch(1);val ack=CountDownLatch(1);val newHistory=CountDownLatch(1)
        val store=AgentDraftStore(activity,identity);var old:AgentPanel?=null;var current:AgentPanel?=null
        val empty=JSONObject().put("generation",0).put("has_more",false).put("items",JSONArray()).toString()
        try {
            main { old=AgentPanel(activity,body(),identity,"s",{true},{_,raw -> when(JSONObject(raw).getString("action")) {
                "permissions" -> """{"permission_mode":"ask","full_authorization":false,"revision":0,"can_mutate":true}"""
                "pending" -> """{"items":[],"cursor":null,"has_more":false}"""
                "state" -> "{\"state\":\"idle\",\"history_generation\":0}"
                "history" -> empty
                "send" -> { sent.countDown();check(ack.await(8,TimeUnit.SECONDS));"{\"state\":\"running\"}" }
                else -> error("Unexpected")
            }},{});input().setText("任务已被服务端接受") }
            waitFor("Desktop permissions") { main { views().first{it.contentDescription=="发送"}.isEnabled } }
            main { views().first{it.contentDescription=="发送"}.performClick() }
            assertTrue(sent.await(5,TimeUnit.SECONDS))
            main { old?.close();current=AgentPanel(activity,body(),identity,"s",{false},{_,_ -> check(newHistory.await(8,TimeUnit.SECONDS));empty},{}) }
            ack.countDown();waitFor("ack clears origin draft") { store.read("s").optString("text")=="" }
            newHistory.countDown()
            waitFor("new history callback persisted") { main { !AgentPanel::class.java.getDeclaredField("loading").apply { isAccessible=true }.getBoolean(current) } }
            assertEquals("", store.read("s").optString("text"))
            waitFor("composer reconciled") { main { input().text.isEmpty() } }
            main { input().setText("下一条草稿"); AgentPanel::class.java.getDeclaredMethod("load",Boolean::class.javaPrimitiveType).apply {isAccessible=true}.invoke(current,true) }
            waitFor("subsequent history loaded") { main { !AgentPanel::class.java.getDeclaredField("loading").apply {isAccessible=true}.getBoolean(current) } }
            assertEquals("下一条草稿",store.read("s").optString("text"))
        } finally {ack.countDown();newHistory.countDown();main{old?.close();current?.close();activity.finish()}}
    }
    @Test fun versionedInteractionSnapshotUpdatesCachedToolState() {
        launch();val phase=AtomicInteger();val records=AtomicInteger();var chat:AgentPanel?=null
        fun original()=JSONObject().put("text","说明".repeat(7000)).put("tools",JSONArray().put(JSONObject().put("name","read_terminal"))).put("updates",JSONArray().put(JSONObject().put("state",if(phase.get()==0)"running" else "finished").put(if(phase.get()==0)"call_record_id" else "result_record_id",if(phase.get()==0)"call" else "result")))
        fun item():JSONObject { @Suppress("UNCHECKED_CAST") val rows=AgentPanel::class.java.getDeclaredField("items").apply{isAccessible=true}.get(chat) as Map<String,JSONObject>;return rows["unit"] ?: JSONObject() }
        try {
            main {chat=AgentPanel(activity,body(),listOf("review",UUID.randomUUID().toString(),"desktop"),"s",{true},{_,raw -> when(JSONObject(raw).getString("action")) {
                "permissions" -> """{"permission_mode":"ask","full_authorization":false,"revision":0,"can_mutate":true}"""
                "pending" -> """{"items":[],"cursor":null,"has_more":false}"""
                "state" -> "{\"state\":\"running\",\"history_generation\":0}"
                "history" -> JSONObject().put("generation",0).put("has_more",false).put("items",JSONArray().put(JSONObject().put("id","unit").put("sequence",1).put("created_at",phase.get()+1).put("kind","interaction").put("value",JSONObject().put("partial",true).put("record_id","unit-${phase.get()}").put("text","说明")))).toString()
                "record" -> {records.incrementAndGet();JSONObject().put("kind","history_event").put("body",original().toString()).put("cursor",JSONObject.NULL).toString()}
                else -> error("Unexpected")
            }},{})}
            waitFor("initial full interaction") { main{item().optJSONObject("value")?.optJSONArray("updates")!=null} }
            phase.set(1)
            main {AgentPanel::class.java.getDeclaredMethod("refresh").apply{isAccessible=true}.invoke(chat)}
            waitFor("new history version applied") { main{item().optInt("created_at")==2} }
            val observed=main{item().getJSONObject("value").getJSONArray("updates").getJSONObject(0).getString("state")}
            assertEquals("finished",observed);assertEquals(2,records.get())
        } finally {main{chat?.close();activity.finish()}}
    }
}
