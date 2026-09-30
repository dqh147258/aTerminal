package com.yxf.aterminal

import android.content.Intent
import android.graphics.Bitmap
import android.os.Build
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.ai_terminal_mobile.Account
import uniffi.ai_terminal_mobile.RemoteTerminal
import uniffi.ai_terminal_mobile.RenderFrame
import java.io.File
import java.util.UUID

/** Production panels + native core + encrypted RPC + a disposable Desktop/PTY.
 * Account credentials stay in the test's in-memory Account, never the user's preferences.
 */
class AgentReadingUiTest {
    private val instrumentation=InstrumentationRegistry.getInstrumentation()
    private val context=instrumentation.targetContext
    private lateinit var activity:MainActivity
    private fun <T> main(action:()->T):T {
        var result:T?=null;var failure:Throwable?=null
        instrumentation.runOnMainSync{try{result=action()}catch(error:Throwable){failure=error}}
        failure?.let{throw it};@Suppress("UNCHECKED_CAST") return result as T
    }
    private fun all(view:View):List<View> = listOf(view)+if(view is ViewGroup)(0 until view.childCount).flatMap{all(view.getChildAt(it))}else emptyList()
    private fun views():List<View>{assertTrue("Target lost foreground",activity.hasWindowFocus());return all(activity.window.decorView).filter{it.isShown}}
    private fun waitFor(label:String,condition:()->Boolean){val end=SystemClock.elapsedRealtime()+30000;while(SystemClock.elapsedRealtime()<end){if(condition())return;Thread.sleep(50)}
        val nodes=mutableListOf<String>();fun collect(node:android.view.accessibility.AccessibilityNodeInfo?) { if(node==null)return;nodes.add("${node.className}: ${node.text} ${node.contentDescription}");for(i in 0 until node.childCount)collect(node.getChild(i)) };collect(instrumentation.uiAutomation.rootInActiveWindow)
        File(context.filesDir,"agent-ui-timeout.txt").writeText(nodes.joinToString("\n"))
        instrumentation.uiAutomation.takeScreenshot()?.let { b -> File(context.filesDir,"agent-ui-timeout.png").outputStream().use {b.compress(Bitmap.CompressFormat.PNG,100,it)};b.recycle() }
        fail("Timed out: $label")}
    private fun click(text:String)=main{views().filterIsInstance<Button>().first{it.text.toString()==text&&it.isEnabled}.performClick()}
    private fun field(hint:String)=views().filterIsInstance<EditText>().first{it.hint?.toString()==hint || it.contentDescription?.toString()==hint}
    private fun panel(title:String)=MainActivity::class.java.getDeclaredMethod("panel",String::class.java,Boolean::class.javaPrimitiveType).apply{isAccessible=true}.invoke(activity,title,false) as LinearLayout
    private fun screenshot(name:String){instrumentation.waitForIdleSync();waitFor("screenshot foreground window"){instrumentation.uiAutomation.rootInActiveWindow?.packageName?.toString()==context.packageName};instrumentation.uiAutomation.takeScreenshot().let{bitmap->File(context.filesDir,"agent-ui-$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()}}
    @Test fun readingSettingsAndAgentEvidenceRoundTrip(){
        val fixture=JSONObject(File(context.filesDir,"agent-ui-fixture.json").readText())
        val accountBefore=context.getSharedPreferences("account",0).all.toMap()
        val connectionBefore=context.getSharedPreferences("connection",0).all.toMap()
        val account=Account();val remote=RemoteTerminal();var chat:AgentPanel?=null
        val cache=File(context.cacheDir,"agent-ui-${UUID.randomUUID()}.sqlite3")
        val report=JSONObject().put("api",Build.VERSION.SDK_INT).put("model",Build.MODEL).put("real_encrypted_rpc",true).put("model_source","local deterministic fixture")
        try{
            account.login(fixture.getString("server"),fixture.getString("username"),fixture.getString("password"),"Agent UI fixture","android","")
            var desktop=""
            waitFor("online temporary Desktop"){desktop=account.devices().firstOrNull{it.platform=="desktop"&&it.online}?.id.orEmpty();desktop.isNotEmpty()}
            account.connect(desktop,remote)
            val session=fixture.getString("session");val frame=remote.select(session,false)
            val cwd = JSONObject(remote.agent(session,JSONObject().put("version",1).put("action","context").toString())).optString("cwd")
            assertTrue("Current path must come from the live Desktop process",cwd.startsWith("/")); report.put("current_process_cwd",cwd)
            context.startActivity(Intent(context,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui",true).putExtra("render_fixture",true))
            waitFor("isolated Activity"){main{ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull{it.hasWindowFocus()}?.also{activity=it}!=null}}
            main{
                MainActivity::class.java.getDeclaredMethod("show",RenderFrame::class.java).apply{isAccessible=true}.invoke(activity,frame)
                val body=panel("Agent 设置")
                body.addView(activity.label("Local Desktop · isolated test",12f,Palette.muted))
                AgentSettingsPanel(activity,body,{request->remote.configuration(request)},session,{error("File picker is outside this test")},"reading")
            }
            waitFor("reading defaults"){main{views().filterIsInstance<EditText>().any{it.hint?.toString()=="尾部保留行数（1–100）"}}}
            main{assertEquals("10",field("首部保留行数（1–100）").text.toString());assertEquals("20",field("尾部保留行数（1–100）").text.toString())}
            screenshot("defaults")
            val initial=JSONObject(remote.configuration("{\"action\":\"show\"}"))
            main{field("尾部保留行数（1–100）").setText("0")};click("保存读取设置")
            main{assertEquals("尾部行数须在 1–100 之间",field("尾部保留行数（1–100）").error.toString())}
            assertEquals(initial.getLong("revision"),JSONObject(remote.configuration("{\"action\":\"show\"}")).getLong("revision"))
            main{field("首部保留行数（1–100）").setText("7");field("尾部保留行数（1–100）").setText("31")};click("保存读取设置")
            waitFor("Desktop configuration saved"){JSONObject(remote.configuration("{\"action\":\"show\"}")).getJSONObject("config").getJSONObject("terminal_reading").optInt("tail_lines")==31}
            waitFor("form reload"){main{field("首部保留行数（1–100）").text.toString()=="7"&&field("尾部保留行数（1–100）").text.toString()=="31"}}
            screenshot("settings-saved")
            report.put("defaults",JSONObject().put("head_lines",10).put("tail_lines",20)).put("saved",JSONObject().put("head_lines",7).put("tail_lines",31)).put("invalid_value_rejected",true)
            main{
                val body=panel("AI Agent")
                chat=AgentPanel(activity,body,listOf(fixture.getString("server"),fixture.getString("username"),desktop),session,{true},{id,request->remote.agent(id,request)},{},cachePath=cache.path)
            }
            main{field("发送任务或追加消息").setText("读取隔离终端并回答 UI_FIXTURE_DONE")};main{views().first{it.contentDescription=="发送"}.performClick()}
            waitFor("Agent completed"){val state=JSONObject(remote.agent(session,"{\"version\":1,\"action\":\"state\"}"));if(state.optString("state")=="paused")throw AssertionError("Agent paused: "+state.optString("error"));state.optString("state")=="completed"}
            waitFor("reply rendered"){main{views().filterIsInstance<TextView>().any{it.text.toString()=="UI_FIXTURE_DONE"}}}
            screenshot("conversation")
            waitFor("history rendered"){main{views().filterIsInstance<TextView>().any{it.text.toString()=="UI_FIXTURE_DONE"}}}
            val history=JSONObject(remote.agent(session,"{\"version\":1,\"action\":\"history\"}"));val items=history.getJSONArray("items");var record=""
            for(i in 0 until items.length()){
                val updates=items.getJSONObject(i).optJSONObject("value")?.optJSONArray("updates")?:continue
                for(j in 0 until updates.length()){val id=updates.getJSONObject(j).optString("record_id");if(id.isNotEmpty()&&id!="null")record=id}
            }
            assertTrue("No terminal observation",record.isNotEmpty())
            val anchors=JSONObject(remote.agent(session,JSONObject().put("version",1).put("action","record").put("record_id",record).put("part","anchors").toString()))
            assertEquals(7,anchors.getJSONObject("head_anchor").getJSONArray("lines").length());assertEquals(31,anchors.getJSONObject("tail_anchor").getJSONArray("lines").length())
            val search=anchors.getJSONObject("search_tail_anchor").getJSONArray("lines");assertEquals(31,search.length())
            for(i in 0 until search.length()){assertTrue(search.getString(i).startsWith("UI_LOG_"))}
            val original=JSONObject(remote.agent(session,JSONObject().put("version",1).put("action","record").put("record_id",record).put("part","body").toString()))
            assertTrue(original.getString("body").contains("TUI status: 01"));assertTrue(anchors.getJSONObject("tail_anchor").getJSONArray("lines").toString().contains("TUI status: 01"))
            report.put("agent_reply_rendered",true).put("history_rendered",true).put("tui_filtered_search",true).put("raw_tui_preserved",true).put("anchors",anchors)
            screenshot("history")
            val toolItem = (0 until items.length()).map { items.getJSONObject(it) }.first { AgentTimeline.evidence(it).any { e -> e.id == record } }
            val unit = toolItem.getString("id")
            main { views().first { it.contentDescription == "查看工具详情" && it.tag == unit }.performClick() }
            waitFor("tool details page") { main { views().any { it.tag == "record-toggle:$record" } } }
            main { views().first { it.tag == "record-toggle:$record" }.performClick() }
            waitFor("inline original evidence") { main { views().filterIsInstance<TextView>().any { it.text.contains("TUI status: 01") } } }
            main { assertTrue(activity.hasWindowFocus()); assertTrue(views().any { it.contentDescription == "返回对话" }) }
            screenshot("evidence")
            main {
                views().first { it.tag == "record-toggle:$record" }.performClick()
                assertFalse(views().filterIsInstance<TextView>().any { it.text.contains("TUI status: 01") })
                views().first { it.contentDescription == "返回对话" }.performClick()
            }
            waitFor("returned to chat") { main { views().any { it.contentDescription == "发送任务或追加消息" } } }
            report.put("evidence_inline",true).put("tool_details_full_page",true)
            val imageUri = context.contentResolver.insert(android.provider.MediaStore.Images.Media.EXTERNAL_CONTENT_URI,android.content.ContentValues().apply {
                put(android.provider.MediaStore.Images.Media.DISPLAY_NAME,"agent-wire-test.png"); put(android.provider.MediaStore.Images.Media.MIME_TYPE,"image/png")
            })!!
            try {
                context.contentResolver.openOutputStream(imageUri)!!.use { output -> Bitmap.createBitmap(120,60,Bitmap.Config.ARGB_8888).apply { eraseColor(android.graphics.Color.BLUE); compress(Bitmap.CompressFormat.PNG,100,output); recycle() } }
                main { AgentPanel::class.java.getDeclaredMethod("importImages",List::class.java,String::class.java).apply { isAccessible=true }.invoke(chat,listOf(imageUri),session) }
                waitFor("image draft ready") { main { views().any { it.contentDescription=="移除图片" } && views().any { it.contentDescription=="发送" && it.isEnabled } } }
                main { views().first { it.contentDescription=="发送" }.performClick() }
                waitFor("visual content reached HTTP model") { main { views().filterIsInstance<TextView>().any { it.text.toString()=="VISION_FIXTURE_DONE" } } }
                val imageHistory = JSONObject(remote.agent(session,"{\"version\":1,\"action\":\"history\"}")).getJSONArray("items")
                val picture = (0 until imageHistory.length()).map { imageHistory.getJSONObject(it) }.first { it.optJSONObject("value")?.optJSONArray("images") != null }.getJSONObject("value").getJSONArray("images").getJSONObject(0)
                val stored = JSONObject(remote.agent(session,JSONObject().put("version",1).put("action","record").put("record_id",picture.getString("record_id")).put("part","body").toString()))
                assertEquals("base64url",stored.getString("encoding"))
                assertTrue(android.util.Base64.decode(stored.getString("body"),android.util.Base64.URL_SAFE).take(4).toByteArray().contentEquals(byteArrayOf(-119,80,78,71)))
                report.put("image_android_to_desktop_to_http_model",true).put("image_only_message",true).put("binary_image_record",true)
                screenshot("vision")
            } finally { context.contentResolver.delete(imageUri,null,null) }
            // Exercise multiple real durable global agents through the encrypted mobile transport.
            fun global(command: JSONObject) = JSONObject(remote.agent("", command.put("version", 1).toString()))
            val creation = JSONObject().put("action", "global_create").put("request_id", "global-ui-a")
            val first = global(creation).getJSONObject("scope")
            assertEquals(first.toString(), global(creation).getJSONObject("scope").toString())
            val second = global(JSONObject().put("action", "global_create").put("request_id", "global-ui-b")).getJSONObject("scope")
            assertNotEquals(first.getString("agent"), second.getString("agent")); assertTrue(first.isNull("session")); assertTrue(second.isNull("session"))
            global(JSONObject().put("action", "send").put("request_id", "legacy-global-message").put("message", "GLOBAL_FIXTURE_LEGACY"))
            for ((scope, message) in listOf(first to "GLOBAL_FIXTURE_A", second to "GLOBAL_FIXTURE_B")) {
                global(JSONObject().put("action", "send").put("agent_id", scope.getString("agent")).put("request_id", "same-message-id").put("message", message))
            }
            main {
                chat?.close()
                chat = AgentPanel(activity, panel("全局AI助手"), listOf(fixture.getString("server"), fixture.getString("username"), desktop), "", { true }, { id, json -> remote.agent(id, json) }, {},
                    workingPath = { "Fixture Desktop" }, globalConversation = JSONObject().put("scope", second).put("title", "新会话"))
            }
            waitFor("independent global tasks complete") {
                listOf(first, second).all { global(JSONObject().put("action", "state").put("agent_id", it.getString("agent"))).optString("state") == "completed" }
            }
            waitFor("global reply rendered") { main { views().filterIsInstance<TextView>().any { it.text.toString() == "GLOBAL_FIXTURE_DONE" } } }
            for ((scope, message) in listOf(first to "GLOBAL_FIXTURE_A", second to "GLOBAL_FIXTURE_B")) {
                val messages = global(JSONObject().put("action", "history").put("agent_id", scope.getString("agent"))).getJSONArray("items")
                val users = (0 until messages.length()).map { messages.getJSONObject(it) }.filter { it.optString("kind") == "user" }
                assertEquals(listOf(message), users.map { it.getJSONObject("value").getString("message") })
            }
            val catalog = global(JSONObject().put("action", "global_list")).getJSONArray("conversations")
            assertTrue((0 until catalog.length()).any { catalog.getJSONObject(it).optBoolean("legacy") })
            assertTrue((0 until catalog.length()).filter { catalog.getJSONObject(it).getJSONObject("scope").getString("agent") in listOf(first.getString("agent"), second.getString("agent")) }.all { catalog.getJSONObject(it).optLong("last_reply_sequence") > 0 })
            screenshot("global-wire")
            report.put("global_encrypted_rpc", true).put("global_independent_history", true).put("global_legacy_preserved", true).put("global_idempotent_creation", true)
            main{chat?.close();chat=null;panel("设置")}
            assertTrue("Primary account preferences changed",accountBefore==context.getSharedPreferences("account",0).all)
            assertTrue("Primary connection preferences changed",connectionBefore==context.getSharedPreferences("connection",0).all)
            report.put("primary_preferences_unchanged",true).put("passed",true)
        }catch(error:Throwable){report.put("passed",false).put("error",error.toString());throw error}
        finally{
            main{chat?.close();if(::activity.isInitialized)activity.finish()}
            try{remote.disconnect()}catch(_:Exception){}
            try{account.logout()}catch(_:Exception){}
            remote.close();account.close()
            File(context.filesDir,"agent-ui-results.json").writeText(report.toString(2))
            listOf("","-wal","-shm").forEach{File(cache.path+it).delete()}
        }
    }
}
