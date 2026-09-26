package com.yxf.aterminal

import android.app.Activity
import android.app.AlertDialog
import org.json.JSONObject
import org.xml.sax.Attributes
import org.xml.sax.helpers.DefaultHandler
import uniffi.ai_terminal_mobile.AgentCache
import java.io.File
import java.io.Writer
import java.security.MessageDigest
import java.util.concurrent.Executors
import javax.xml.parsers.SAXParserFactory

/** Never constructs SharedPreferences or a whole conversation JSON string. */
class LegacyAgentHistory(private val activity: Activity, server: String, account: String, namespace: String, private val valid: () -> Boolean = {true}) {
    private val worker = Executors.newSingleThreadExecutor()
    private val cache = AgentCache.open(activity.filesDir.resolve("agent-history.sqlite3").path)
    private val identity = hash(ChatStore.canonicalServer(server) + "\u0000" + account + if (namespace.isEmpty()) "" else "\u0000" + namespace)
    private val prefix = "legacy/$identity/"
    fun open() { worker.execute { try { migrate(); list(null) } catch (e: Exception) { activity.runOnUiThread { AlertDialog.Builder(activity).setMessage("旧归档导入失败，原文件已保留：${e.message}").setPositiveButton("关闭", null).show() } } } }
    private fun migrate() {
        val source = File(activity.applicationInfo.dataDir, "shared_prefs/chats-$identity.xml")
        if (!source.isFile) return
        val markers=activity.getSharedPreferences("agent-legacy-imports",Activity.MODE_PRIVATE)
        val stamp="${source.length()}:${source.lastModified()}"
        if(markers.getString(identity,null)==stamp)return
        val temp = File.createTempFile("legacy-chat-", ".json", activity.cacheDir)
        try {
            val factory = SAXParserFactory.newInstance()
            factory.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true)
            factory.setFeature("http://xml.org/sax/features/external-general-entities", false)
            factory.setFeature("http://xml.org/sax/features/external-parameter-entities", false)
            var output: Writer? = null; var key = ""
            try {
                factory.newSAXParser().parse(source, object : DefaultHandler() {
                    override fun startElement(uri: String?, localName: String?, qName: String?, attributes: Attributes) {
                        if (qName == "string") { key = attributes.getValue("name"); output = temp.bufferedWriter() }
                    }
                    override fun characters(ch: CharArray, start: Int, length: Int) { output?.write(ch, start, length) }
                    override fun endElement(uri: String?, localName: String?, qName: String?) {
                        if (qName == "string") { output?.close(); output = null; cache.importLegacy(prefix + key, temp.path) }
                    }
                })
            } finally { output?.close() }
            check(markers.edit().putString(identity,stamp).commit()){"迁移标记无法保存"}
        } finally { temp.delete() }
    }
    private fun list(before: String?) {
        val rows = JSONObject(cache.legacyScopes(prefix, before)).getJSONArray("items")
        val names = (0 until rows.length()).map { rows.getJSONObject(it).optJSONObject("metadata")?.optString("title").orEmpty().ifEmpty { "旧对话" } }
        activity.runOnUiThread {
            if(!valid()) return@runOnUiThread
            AlertDialog.Builder(activity).setTitle("旧手机归档 · 只读")
                .setItems((names + if (rows.length() == 50) listOf("下一页") else emptyList()).toTypedArray()) { _, index ->
                    worker.execute { if (index == rows.length()) list(rows.getJSONObject(index - 1).getString("scope")) else page(rows.getJSONObject(index).getString("scope"), null) }
                }.setNegativeButton("关闭", null).show()
        }
    }
    private fun page(scope: String, before: Long?) {
        val result = JSONObject(cache.legacyPage(scope, before)); val rows = result.getJSONArray("items")
        val text = (0 until rows.length()).joinToString("\n\n") { val value = rows.getJSONObject(it).getJSONObject("value"); value.optString("role") + "\n" + value.optString("content") }
        activity.runOnUiThread {
            if(!valid()) return@runOnUiThread
            val dialog = AlertDialog.Builder(activity).setTitle("旧手机归档 · 每页 50 条")
                .setView(activity.scroll(activity.column(12).apply { addView(activity.label(text).apply { setTextIsSelectable(true) }) })).setNegativeButton("关闭", null)
            if (result.optBoolean("has_more")) dialog.setPositiveButton("更早的 50 条") { _, _ -> worker.execute { page(scope, result.getLong("before")) } }
            dialog.show()
        }
    }
    private fun hash(value: String) = MessageDigest.getInstance("SHA-256").digest(value.toByteArray()).joinToString("") { "%02x".format(it) }
}
