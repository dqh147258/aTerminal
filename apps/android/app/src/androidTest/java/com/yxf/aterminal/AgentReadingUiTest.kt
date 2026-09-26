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
    private fun waitFor(label:String,condition:()->Boolean){val end=SystemClock.elapsedRealtime()+30000;while(SystemClock.elapsedRealtime()<end){if(condition())return;Thread.sleep(50)};fail("Timed out: $label")}
    private fun click(text:String)=main{views().filterIsInstance<Button>().first{it.text.toString()==text&&it.isEnabled}.performClick()}
    private fun field(hint:String)=views().filterIsInstance<EditText>().first{it.hint?.toString()==hint}
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
            context.startActivity(Intent(context,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui",true).putExtra("render_fixture",true))
            waitFor("isolated Activity"){main{ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull{it.hasWindowFocus()}?.also{activity=it}!=null}}
            main{
                MainActivity::class.java.getDeclaredMethod("show",RenderFrame::class.java).apply{isAccessible=true}.invoke(activity,frame)
                val body=panel("Agent 设置")
                body.addView(activity.label("Local Desktop · isolated test",12f,Palette.muted))
                AgentSettingsPanel(activity,body,{request->remote.configuration(request)},session,{error("File picker is outside this test")})
            }
            waitFor("reading defaults"){main{views().filterIsInstance<EditText>().any{it.hint?.toString()=="尾部保留行数（1–100）"}}}
            main{assertEquals("10",field("首部保留行数（1–100）").text.toString());assertEquals("20",field("尾部保留行数（1–100）").text.toString())}
            screenshot("defaults")
            val initial=JSONObject(remote.configuration("{\"action\":\"show\"}"))
            main{field("尾部保留行数（1–100）").setText("0")};click("保存读取设置")
            main{assertTrue(views().filterIsInstance<TextView>().any{it.text.toString().contains("首尾行数须在")})}
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
            main{field("发送任务或追加消息").setText("读取隔离终端并回答 UI_FIXTURE_DONE")};click("发送 / 追加")
            waitFor("Agent completed"){val state=JSONObject(remote.agent(session,"{\"version\":1,\"action\":\"state\"}"));if(state.optString("state")=="paused")throw AssertionError("Agent paused: "+state.optString("error"));state.optString("state")=="completed"}
            waitFor("reply rendered"){main{views().filterIsInstance<TextView>().any{it.text.toString()=="UI_FIXTURE_DONE"}}}
            screenshot("conversation")
            click("历史")
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
            val evidenceCount=main{views().filterIsInstance<Button>().count{it.text.toString()=="查看证据"}}
            var rawDialog=false
            for(index in 0 until evidenceCount){
                main{views().filterIsInstance<Button>().filter{it.text.toString()=="查看证据"}[index].performClick()}
                waitFor("evidence dialog"){instrumentation.uiAutomation.rootInActiveWindow?.findAccessibilityNodeInfosByText("证据原文")?.isNotEmpty()==true}
                if(instrumentation.uiAutomation.rootInActiveWindow?.findAccessibilityNodeInfosByText("TUI status: 01")?.isNotEmpty()==true){rawDialog=true;screenshot("evidence")}
                instrumentation.uiAutomation.rootInActiveWindow.findAccessibilityNodeInfosByText("关闭").last{it.isClickable}.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK)
                waitFor("dialog dismissed"){main{activity.hasWindowFocus()}}
                if(rawDialog)break
            }
            assertTrue("Original text was not displayed in the evidence dialog",rawDialog);report.put("evidence_dialog",true)
            main{chat?.close();chat=null;panel("终端设置")}
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
