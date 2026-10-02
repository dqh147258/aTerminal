package com.yxf.aterminal

import android.content.Intent
import android.content.ContentValues
import android.content.pm.ActivityInfo
import android.graphics.Bitmap
import android.os.SystemClock
import android.provider.MediaStore
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
import java.io.File
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/** Isolated UI contract tests use production views without touching account credentials or real tasks. */
class MobilePrototypeTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private lateinit var activity: MainActivity
    private fun <T> main(action: () -> T): T { var result: T? = null; var failure: Throwable? = null
        instrumentation.runOnMainSync { try { result = action() } catch (e: Throwable) { failure = e } }; failure?.let { throw it }; @Suppress("UNCHECKED_CAST") return result as T }
    private fun all(v: View): List<View> = listOf(v) + if (v is ViewGroup) (0 until v.childCount).flatMap { all(v.getChildAt(it)) } else emptyList()
    private fun views() = all(activity.window.decorView).filter { it.isShown }
    private fun waitFor(label: String, condition: () -> Boolean) { val end = SystemClock.elapsedRealtime() + 15000; while (SystemClock.elapsedRealtime() < end) { if (condition()) return; Thread.sleep(40) }; fail("Timeout: $label") }
    private fun field() = views().filterIsInstance<EditText>().first { it.contentDescription == "发送任务或追加消息" }
    private fun icon(name: String) = views().first { it.contentDescription == name }
    private fun button(name: String) = views().filterIsInstance<Button>().first { it.text == name }
    private fun panel() = MainActivity::class.java.getDeclaredMethod("panel", String::class.java, Boolean::class.javaPrimitiveType).apply { isAccessible = true }.invoke(activity, "AI Agent", false) as LinearLayout
    private fun shot(name: String) { instrumentation.waitForIdleSync(); Thread.sleep(800); instrumentation.uiAutomation.takeScreenshot().let { b -> File(context.filesDir,"prototype-$name.png").outputStream().use { b.compress(Bitmap.CompressFormat.PNG,100,it) }; b.recycle() } }
    private fun launch() {
        context.startActivity(Intent(context,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra("isolated_ui",true).putExtra("render_fixture",true))
        waitFor("Activity") { main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull()?.also { activity = it } != null } }
    }
    @Test fun compactToolsKeepNarrationAndFullDetailsAcrossRefresh() {
        launch()
        var chat: AgentPanel? = null
        val polls = java.util.concurrent.atomic.AtomicInteger()
        val narration = "**先读取终端**，再说明结果。"
        val narrationText = "先读取终端，再说明结果。"
        val answer = """## 工作区状态

**Shell**：idle zsh in `/Volumes/Code/My/aTerminal`

- **Last activity**：a git status run showing unstaged changes.
- *Next step*：检查当前修改。

> 保留正常回复的全部内容。

```sh
pwd
git status --short
```

[项目文档](https://example.com/docs)

~~已取消的检查~~

- [x] 已完成读取
- [ ] 等待下一步

| 项目 | 状态 |
| --- | --- |
| 终端 | 空闲 |

""" + (1..24).joinToString("\n") { "$it. 检查项 $it：这是一段需要保留完整内容的结果说明。" } + "\n\n**末尾确认**：全部 24 项均已展示。"
        val interaction = JSONObject().put("id", "tool-round").put("sequence", 2).put("kind", "interaction").put("root_user_message_id", "root")
            .put("value", JSONObject().put("text", narration).put("tools", JSONArray().put(JSONObject().put("name", "read_terminal")))
                .put("updates", JSONArray().apply { repeat(10) { put(JSONObject().put("record_id", "record-$it")) } }))
        try {
            main { chat = AgentPanel(activity, panel(), listOf("https://compact.invalid", UUID.randomUUID().toString(), "d"), "s", { true }, { _, raw ->
                when (JSONObject(raw).getString("action")) {
                    "permissions" -> """{"permission_mode":"ask","full_authorization":false,"revision":0,"can_mutate":true}"""
                "pending" -> """{"items":[],"cursor":null,"has_more":false}"""
                "state" -> JSONObject().put("history_generation", 0).put("state", "running").put("root_user_message_id", "root").put("live_text", if (polls.incrementAndGet() < 2) narration else "").toString()
                    "history" -> JSONObject().put("generation", 0).put("has_more", false).put("items", JSONArray().apply {
                        if (polls.get() >= 2) put(JSONObject().put("id", "reply").put("sequence", 3).put("kind", "assistant").put("value", JSONObject().put("text", answer)))
                        if (polls.get() >= 2) put(interaction)
                        put(JSONObject().put("id", "user").put("sequence", 1).put("kind", "user").put("value", JSONObject().put("message", "Hi")))
                    }).toString()
                    else -> error("Unexpected request")
                }
            }, {}) }
            waitFor("streaming narration") { main { views().filterIsInstance<TextView>().any { it.text.toString() == narrationText } } }
            waitFor("durable tool narration") { polls.get() >= 3 && main { views().any { it.contentDescription == "查看工具详情" } } }
            main {
                assertEquals(1, views().filterIsInstance<TextView>().count { it.text.toString() == narrationText })
                assertFalse(views().filterIsInstance<Button>().any { it.text.toString().contains("查看证据") })
                val text = views().filterIsInstance<TextView>().first { it.text.toString().contains("末尾确认：全部 24 项均已展示。") }
                assertNull(text.ellipsize); assertTrue(text.maxLines > 24); assertTrue(text.lineCount > 24)
                assertEquals(0, text.layout.getEllipsisCount(text.lineCount - 1))
                assertTrue(text.isTextSelectable)
                assertFalse(text.text.toString().contains("**Shell**"))
                val styled = text.text as android.text.Spanned
                val spans = styled.getSpans(0, styled.length, Any::class.java).map { it.javaClass.simpleName }.toSet()
                for (span in listOf("StrongEmphasisSpan", "HeadingSpan", "BulletListItemSpan", "CodeBlockSpan", "LinkSpan", "BlockQuoteSpan", "TableRowSpan", "StrikethroughSpan")) assertTrue("Missing $span in $spans", span in spans)
                assertFalse(views().any { it.contentDescription == "查看完整消息" })
                val summary = views().filterIsInstance<TextView>().first { it.text.toString().contains("read_terminal") }
                assertTrue(summary.height <= activity.dp(60))
                assertTrue(views().filterIsInstance<TextView>().any { it.text.toString() == "Hi" })
            }
            main { (AgentPanel::class.java.getDeclaredField("scroll").apply { isAccessible=true }.get(chat) as ScrollView).scrollTo(0, 0) }
            shot("markdown-chat")
            main { (AgentPanel::class.java.getDeclaredField("scroll").apply { isAccessible=true }.get(chat) as ScrollView).fullScroll(View.FOCUS_DOWN) }
            shot("markdown-tail")
            main { icon("查看工具详情").performClick() }
            waitFor("evidence in details") { main { views().any { it.contentDescription == "展开关联证据 10" } } }
            shot("compact-details")
            main { icon("返回对话").performClick(); assertTrue(views().any { it.contentDescription == "发送任务或追加消息" }) }
        } finally { main { chat?.close(); activity.finish() } }
    }
    @Test fun partialMessageLoadsAllRecordPagesAndSurvivesOfflineReopen() {
        launch()
        var chat: AgentPanel? = null
        val identity = listOf("https://markdown.invalid", UUID.randomUUID().toString(), "desktop")
        val full = (1..180).joinToString("\n") { "$it. **完整历史回复**：检查终端与当前工作目录，保留这一行的全部文字。" } + "\n\n完整原文终点"
        val body = JSONObject().put("text", full).toString()
        val available = java.util.concurrent.atomic.AtomicBoolean(false)
        val chunks = java.util.concurrent.atomic.AtomicInteger()
        val rpc: (String, String) -> String = { _, raw ->
            val command = JSONObject(raw)
            when (command.getString("action")) {
                "permissions" -> """{"permission_mode":"ask","full_authorization":false,"revision":0,"can_mutate":true}"""
                "pending" -> """{"items":[],"cursor":null,"has_more":false}"""
                "state" -> JSONObject().put("state", "completed").put("history_generation", 0).toString()
                "history" -> JSONObject().put("generation", 0).put("has_more", false).put("items", JSONArray()
                    .put(JSONObject().put("id", "long-reply").put("sequence", 2).put("kind", "assistant").put("value", JSONObject().put("text", full.take(120)).put("record_id", "long-reply").put("partial", true)))
                    .put(JSONObject().put("id", "user").put("sequence", 1).put("kind", "user").put("value", JSONObject().put("message", "完整读取历史")))).toString()
                "record" -> {
                    check(available.get()) { "temporary read failure" }
                    assertEquals("long-reply", command.getString("record_id")); chunks.incrementAndGet()
                    val offset = command.optString("cursor").toIntOrNull() ?: 0; val end = minOf(body.length, offset + 2048)
                    JSONObject().put("kind", "history_event").put("body", body.substring(offset, end)).put("cursor", if (end < body.length) end.toString() else JSONObject.NULL).toString()
                }
                else -> error("Unexpected request")
            }
        }
        try {
            main { chat = AgentPanel(activity, panel(), identity, "session", { true }, rpc, {}) }
            waitFor("partial recovery action") { main { views().filterIsInstance<Button>().any { it.text == "完整消息暂不可用，点击重试" } } }
            main {
                assertTrue(views().filterIsInstance<TextView>().any { it.text.toString() == "完整读取历史" })
                available.set(true); button("完整消息暂不可用，点击重试").performClick()
            }
            waitFor("all record pages") { main { views().filterIsInstance<TextView>().any { it.text.endsWith("完整原文终点") } } }
            assertTrue(chunks.get() > 1)
            main {
                val text = views().filterIsInstance<TextView>().first { it.text.endsWith("完整原文终点") }
                assertNull(text.ellipsize); assertEquals(180, Regex("完整历史回复").findAll(text.text).count())
                chat?.close()
                val drafts = AgentDraftStore(activity, identity); drafts.save("session", drafts.read("session").apply { remove("items") })
                chat = AgentPanel(activity, panel(), identity, "session", { false }, { _, _ -> error("offline") }, {})
            }
            waitFor("full cached message offline") { main { views().filterIsInstance<TextView>().any { it.text.endsWith("完整原文终点") } } }
            main { assertFalse(views().filterIsInstance<Button>().any { it.text == "完整消息暂不可用，点击重试" }) }
        } finally { main { chat?.close(); activity.finish() } }
    }
    @Test fun unifiedDrawerAndSettingsNavigationPreserveTerminal() {
        launch()
        fun get(name: String) = MainActivity::class.java.getDeclaredField(name).apply { isAccessible=true }.get(activity)
        fun set(name: String, value: Any) = MainActivity::class.java.getDeclaredField(name).apply { isAccessible=true }.set(activity,value)
        try {
            val terminal = main { get("terminal") }
            main {
                icon("设置").performClick(); icon("LLM 大模型").performClick()
                assertTrue(views().any { it.contentDescription=="返回上一页" })
                activity.onBackPressed(); assertTrue(views().filterIsInstance<TextView>().any { it.text.toString()=="设置" })
                assertSame(terminal,get("terminal")); icon("关闭设置").performClick()
                // Freeze transport polling while presenting explicit, isolated device/session fixtures.
                set("active", false); set("connected", true); set("sessionRefreshBusy", true); set("archiveLoading", true)
                set("deviceId", "desktop"); set("deviceName", "Mac Studio"); set("accountName", "demo"); set("selected", "one")
                set("devices", listOf(uniffi.ai_terminal_mobile.AccountDevice("desktop", "Mac Studio", "desktop", true, false), uniffi.ai_terminal_mobile.AccountDevice("offline", "Mac Studio", "desktop", false, false)))
                set("sessions", listOf(
                    uniffi.ai_terminal_mobile.RemoteSession("one", "/Volumes/Code/My/aTerminal", false, 0u, true),
                    uniffi.ai_terminal_mobile.RemoteSession("server", "/Volumes/Code/My/aTerminal/server", false, 0u, true),
                    uniffi.ai_terminal_mobile.RemoteSession("detached", "/Volumes/Code/My", false, 0u, false)))
                set("agentArchives", listOf(Conversation("desktop", "one", "old title"), Conversation("offline", "closed", "/work/archived/android")))
                icon("打开工作空间").performClick()
                assertFalse(views().filterIsInstance<RadioButton>().any { it.text.toString() in listOf("终端", "AI 历史") })
                assertEquals(1, views().count { it.tag == "one" })
                assertTrue(views().first { it.tag == "one" }.isSelected)
                assertTrue(views().any { it.tag == "closed" })
                assertTrue(views().filterIsInstance<TextView>().any { it.text == "4 个会话" })
                assertTrue(views().filterIsInstance<TextView>().any { it.text == "桌面已离开" })
                assertTrue(views().filterIsInstance<TextView>().any { it.text == "离线" })
                assertEquals(2, views().count { it.tag?.toString()?.startsWith("close-") == true })
                assertFalse(views().any { it.contentDescription?.toString()?.startsWith("打开 ") == true && it.contentDescription?.toString()?.endsWith(" 对话") == true })
                assertTrue(icon("新建会话").isEnabled)
            }
            shot("drawer")
            main {
                val search = views().filterIsInstance<EditText>().first { it.hint == "搜索终端或对话" }; search.setText("archived")
                assertEquals(0, views().count { it.tag == "one" })
                assertTrue(views().filterIsInstance<TextView>().any { it.text == "1 个会话" })
                assertFalse(views().any { it.contentDescription?.toString()?.contains("旧手机归档") == true })
                assertFalse(views().filterIsInstance<TextView>().any { it.text.contains("旧手机归档") })
                assertTrue(icon("账号与设备").isShown)
                views().first { it.tag == "closed" }.performClick()
                assertEquals("one", get("selected")); assertSame(terminal, get("terminal"))
                field().setText("离线草稿"); assertFalse(icon("发送").isEnabled)
            }
        } finally { main { activity.finish() } }
    }
    @Test fun scopesComposerImagesPagingAndSettings() {
        launch()
        var chat: AgentPanel? = null
        val identity = listOf("https://prototype.invalid",UUID.randomUUID().toString(),"desktop")
        val states = ConcurrentHashMap<String,String>()
        val commands = java.util.concurrent.CopyOnWriteArrayList<Pair<String,JSONObject>>()
        val path = java.util.concurrent.atomic.AtomicReference("/Volumes/Code/My/aTerminal")
        var media: android.net.Uri? = null
        try {
            main { icon("设置").performClick() }
            main {
                val root = MainActivity::class.java.getDeclaredField("root").apply { isAccessible = true }.get(activity) as View
                val overlay = MainActivity::class.java.getDeclaredField("overlayPanel").apply { isAccessible = true }.get(activity) as View
                assertEquals(root.width-root.paddingLeft-root.paddingRight,overlay.width)
                assertTrue(views().filterIsInstance<TextView>().any { it.text == "设置" })
                assertTrue(views().any { it.contentDescription == "LLM 大模型" })
            }
            shot("settings")
            val request: (String,String)->String = { scope, text ->
                val c = JSONObject(text); commands.add(scope to c)
                when(c.getString("action")) {
                    "permissions" -> """{"permission_mode":"ask","full_authorization":false,"revision":0,"can_mutate":true}"""
                "pending" -> """{"items":[],"cursor":null,"has_more":false}"""
                "state" -> JSONObject().put("state",states[scope] ?: "idle").put("history_generation",0).toString()
                    "send" -> { states[scope] = "running"; "{\"state\":\"running\"}" }
                    "cancel" -> { states[scope] = "stopping"; "{\"state\":\"stopping\"}" }
                    "image_begin" -> "{\"upload_id\":\"fixture-image\"}"
                    "image_chunk", "image_release" -> "{}"
                    "history" -> {
                        val old = c.has("cursor"); val items = JSONArray()
                        val range = if (old) 50 downTo 1 else 100 downTo 51
                        for (i in range) items.put(JSONObject().put("id","$scope-$i").put("sequence",i).put("kind",if(i%3==0) "user" else "assistant").put("value",JSONObject().put("message", "$scope message $i\n检查终端工作区与构建结果。")))
                        JSONObject().put("generation",0).put("items",items).put("cursor",if(old) JSONObject.NULL else "older").put("has_more",!old).toString()
                    }
                    else -> error("Unexpected $c")
                }
            }
            fun switch(id: String) { chat?.close(); chat = AgentPanel(activity,panel(),identity,id,{true},request,{},workingPath={path.get()}) }
            main { switch("session") }
            waitFor("history") { commands.any { it.second.optString("action")=="history" } }
            instrumentation.waitForIdleSync()
            main {
                assertFalse(views().filterIsInstance<Button>().any { it.text in listOf("对话","历史","设置","停止","Global","Session") })
                assertFalse(icon("发送").isEnabled)
                field().setText("Session 草稿")
                switch("other-session"); assertEquals("",field().text.toString()); field().setText("另一会话草稿")
                switch("session"); assertEquals("Session 草稿",field().text.toString())
                field().setText("")
            }
            val uri = context.contentResolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI,ContentValues().apply { put(MediaStore.Images.Media.DISPLAY_NAME,"prototype-test.png"); put(MediaStore.Images.Media.MIME_TYPE,"image/png") })!!; media=uri
            context.contentResolver.openOutputStream(uri)!!.use { out -> Bitmap.createBitmap(160,80,Bitmap.Config.ARGB_8888).apply { eraseColor(android.graphics.Color.CYAN); compress(Bitmap.CompressFormat.PNG,100,out); recycle() } }
            main { AgentPanel::class.java.getDeclaredMethod("importImages",List::class.java,String::class.java).apply { isAccessible=true }.invoke(chat,listOf(uri),"session") }
            waitFor("image ready") { main { views().any { it.contentDescription=="移除图片" } && icon("发送").isEnabled } }
            main { switch("other-session"); assertFalse(views().any { it.contentDescription=="移除图片" }); switch("session"); assertTrue(views().any { it.contentDescription=="移除图片" }) }
            shot("chat-image-draft")
            waitFor("permissions after reopening") { main { icon("发送").isEnabled } }
            main { icon("发送").performClick() }
            waitFor("image-only send") { commands.any { it.second.optString("action")=="send" } }
            val sent = commands.first { it.second.optString("action")=="send" }
            assertEquals("session",sent.first); assertEquals("",sent.second.getString("message")); assertEquals("ask",sent.second.getString("permission_mode")); assertFalse(sent.second.has("allow_input")); assertEquals(1,sent.second.getJSONArray("images").length())
            waitFor("stop button") { main { views().any { it.contentDescription=="停止" && it.isEnabled } } }
            main { field().setText("追加检查"); assertTrue(icon("追加").isEnabled); icon("追加").performClick() }
            waitFor("append") { commands.count { it.second.optString("action")=="send" }==2 }
            waitFor("empty draft") { main { field().text.isEmpty() && views().any {it.contentDescription=="停止" && it.isEnabled} } }
            main { icon("停止").performClick() }
            waitFor("cancel confirmation pending") { main { views().any { it.contentDescription=="取消中" && !it.isEnabled } } }
            main { switch("other-session"); assertEquals("另一会话草稿",field().text.toString()); assertEquals("idle",states["other-session"] ?: "idle") }
            path.set("/tmp/path-changed")
            Thread.sleep(2000)
            main { assertTrue("cwd texts=" + views().filterIsInstance<TextView>().take(8).map { it.text.toString() }, views().filterIsInstance<TextView>().any { it.text.toString()=="/tmp/path-changed" }) }
            main {
                val scroll = AgentPanel::class.java.getDeclaredField("scroll").apply { isAccessible=true }.get(chat) as ScrollView; scroll.scrollTo(0,1000); scroll.scrollTo(0,0)
            }
            waitFor("older page") { commands.any { it.second.optString("cursor")=="older" } }
            shot("chat-paging")
            main {
                field().requestFocus(); (activity.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager).showSoftInput(field(),0)
            }
            Thread.sleep(500); shot("chat-keyboard")
            main { activity.requestedOrientation=ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE }
            waitFor("landscape") { main { activity.resources.configuration.orientation==android.content.res.Configuration.ORIENTATION_LANDSCAPE } }
            shot("chat-landscape")
            val constrained = main {
                val panelView = field().parent.parent.parent as LinearLayout
                val before = panelView.layoutParams.height
                panelView.layoutParams = panelView.layoutParams.apply { height = minOf(activity.dp(180), (panelView.parent as View).height) }
                panelView to before
            }
            instrumentation.waitForIdleSync()
            // Hiding auxiliary labels requests a second layout frame after IME/size changes.
            waitFor("constrained message viewport") { main {
                val viewport = AgentPanel::class.java.getDeclaredField("scroll").apply { isAccessible=true }.get(chat) as ScrollView
                val input = field(); val rect = android.graphics.Rect()
                constrained.first.height == constrained.first.layoutParams.height && viewport.height > activity.dp(20) &&
                    input.getGlobalVisibleRect(rect) && rect.height() >= input.height - 2
            } }
            main {
                val rect = android.graphics.Rect(); val input = field()
                assertTrue("Composer fully visible with a tall IME",input.getGlobalVisibleRect(rect) && rect.height() >= input.height-2)
                val scroll = AgentPanel::class.java.getDeclaredField("scroll").apply {isAccessible=true}.get(chat) as ScrollView
                assertTrue("Message viewport remains scrollable with a tall IME",scroll.height > activity.dp(20))
                constrained.first.layoutParams = constrained.first.layoutParams.apply { height=constrained.second }
            }
            main { assertEquals("另一会话草稿",field().text.toString()); val rect=android.graphics.Rect(); assertTrue(field().getGlobalVisibleRect(rect)); chat?.close(); chat=AgentPanel(activity,panel(),identity,"session",{true},request,{}) ; switch("other-session"); assertEquals("另一会话草稿",field().text.toString()) }
        } finally { media?.let { context.contentResolver.delete(it,null,null) }; main { chat?.close(); activity.requestedOrientation=ActivityInfo.SCREEN_ORIENTATION_PORTRAIT; activity.finish() } }
    }
}
