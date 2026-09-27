package com.yxf.aterminal

import android.app.Activity
import android.app.AlertDialog
import android.text.InputType
import android.widget.*
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.Executors

/** All forms commit one revision-checked candidate through Desktop ConfigService. */
class AgentSettingsPanel(private val activity: Activity, private val body: LinearLayout,
    private val request: (String) -> String, private val session: String, private val pickFolder: (((android.net.Uri) -> Unit) -> Unit), private val page: String = "llm") {
    private val worker = Executors.newSingleThreadExecutor()
    private var view = JSONObject()
    private val content = activity.column(12)
    private val status = activity.label("正在读取 Desktop 设置…", 12f, Palette.muted)
    init {
        body.addView(status); body.grow(activity.scroll(content));
        body.addOnAttachStateChangeListener(object:android.view.View.OnAttachStateChangeListener{override fun onViewAttachedToWindow(v:android.view.View){};override fun onViewDetachedFromWindow(v:android.view.View){worker.shutdown()}})
        load()
    }
    private fun run(command: JSONObject, done: (JSONObject) -> Unit) {
        worker.execute { try { val result = JSONObject(request(command.toString())); activity.runOnUiThread { if (body.isAttachedToWindow) { status.text = "修改在下次任务生效"; done(result) } } }
        catch (e: Exception) {
            val conflict = e.message.orEmpty().contains("revision", true)
            val refreshed = if (conflict) runCatching { JSONObject(request(JSONObject().put("action", "show").toString())) }.getOrNull() else null
            activity.runOnUiThread { if (body.isAttachedToWindow) {
                if (refreshed != null) { view = refreshed; render() }
                status.text = if (conflict) "配置已被其他客户端修改，已刷新。请重新检查后保存。" else "未保存：${e.message}"
            } }
        } }
    }
    private fun load() = run(JSONObject().put("action", "show")) { view = it; render() }
    private fun save(config: JSONObject, secrets: JSONObject = JSONObject()) = run(JSONObject().put("action", "replace").put("expected_revision", view.getLong("revision")).put("config", config).put("secrets", secrets)) { load() }
    private fun copy() = JSONObject(view.getJSONObject("config").toString())
    private fun render() { with(activity) {
        content.removeAllViews(); val config = view.getJSONObject("config")
        if (page == "reading") {
        content.addView(heading("终端读取锚点"))
        val reading=config.optJSONObject("terminal_reading") ?: JSONObject()
        val head=field("首部保留行数（1–100）").apply {inputType=InputType.TYPE_CLASS_NUMBER;setText(reading.optInt("head_lines",10).toString())}
        val tail=field("尾部保留行数（1–100）").apply {inputType=InputType.TYPE_CLASS_NUMBER;setText(reading.optInt("tail_lines",20).toString())}
        content.addView(label("首部保留行数（1–100）",12f,Palette.muted));content.addView(head)
        content.addView(label("尾部保留行数（1–100）",12f,Palette.muted));content.addView(tail)
        content.addView(label("搜索排除已识别的动态 TUI 行，原文仍完整保留。",12f,Palette.muted))
        content.addView(actionButton("保存读取设置") {
            val h=head.text.toString().toIntOrNull();val t=tail.text.toString().toIntOrNull()
            if(h==null || t==null || h !in 1..100 || t !in 1..100){status.text="首尾行数须在 1–100 之间"}
            else {val candidate=copy();candidate.put("terminal_reading",JSONObject().put("head_lines",h).put("tail_lines",t));save(candidate)}
        })
        }
        if (page == "llm") {
        content.addView(heading("默认模型"))
        val bindings = config.getJSONObject("bindings")
        val scopes = listOf("global" to "Global 默认", "session-default" to "Session 默认") + if (session.isNotEmpty()) listOf("session/$session" to "当前终端覆盖") else emptyList()
        scopes.forEach { (scope, title) ->
            val selected = bindings.optJSONObject(scope)?.optString("model_id").orEmpty().ifEmpty { "未配置" }
            content.addView(actionButton("$title · $selected") {
                val models = config.getJSONObject("models").keys().asSequence().toList()
                val options = models + if (scope.startsWith("session/")) listOf("继承 Session 默认") else emptyList()
                AlertDialog.Builder(activity).setTitle(title).setItems(options.toTypedArray()) { _, index ->
                    val candidate = copy(); val updated = candidate.getJSONObject("bindings")
                    if (index == models.size) updated.remove(scope) else updated.put(scope, JSONObject().put("model_id", models[index]))
                    save(candidate)
                }.setNegativeButton("取消", null).show()
            }); content.gap(8)
        }
        content.gap(16); content.addView(heading("供应商"))
        content.addView(actionButton("添加供应商") { provider(null) })
        config.getJSONObject("providers").keys().forEach { id -> content.addView(actionButton(id) { provider(id) }) }
        content.addView(heading("模型与思考强度"))
        content.addView(actionButton("添加模型") { model(null) })
        config.getJSONObject("models").keys().forEach { id -> content.addView(actionButton(id) { model(id) }) }
        }
        if (page == "mcp") {
        content.addView(heading("MCP")); content.addView(label("builtin/terminal · 内置，只读", 12f, Palette.muted))
        content.addView(actionButton("导入 MCP JSON") { mcp() })
        config.getJSONObject("mcp").keys().forEach { id -> content.addView(actionButton(id) { extension("mcp", id) }) }
        }
        if (page == "skills") {
        content.addView(heading("Skills")); content.addView(label("终端截图、会话管理、Agent 管理、等待、历史定位 · 内置，只读", 12f, Palette.muted))
        content.addView(actionButton("安装 Desktop 上的 Skill") { skill() })
        content.addView(actionButton("从手机文件夹导入 Skill") { pickFolder { uri -> upload(uri) } })
        config.getJSONObject("skills").keys().forEach { id -> content.addView(actionButton(id) { extension("skills", id) }) }
        }
    } }
    private fun spinner(values: List<String>, selected: String = "") = Spinner(activity).apply {
        adapter = ArrayAdapter(activity, android.R.layout.simple_spinner_dropdown_item, values)
        setSelection(values.indexOf(selected).coerceAtLeast(0))
    }
    private fun provider(id: String?) { with(activity) {
        val old = id?.let { view.getJSONObject("config").getJSONObject("providers").getJSONObject(it) }
        val fields = column(12); val name = field("供应商 ID").apply { setText(id.orEmpty()); isEnabled = id == null }
        val protocol = spinner(listOf("openai_responses", "openai_chat", "anthropic", "gemini", "azure_openai", "ollama"), old?.getJSONObject("connection")?.optString("protocol").orEmpty())
        val endpoint = field("API 地址").apply { setText(old?.getJSONObject("connection")?.optString("endpoint") ?: "https://api.openai.com/v1") }
        val apiVersion = field("Azure API version").apply { setText(old?.getJSONObject("connection")?.optString("api_version").orEmpty().removePrefix("null")) }
        val key = field("API 密钥（留空保留）").apply { inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD }
        val enabled = CheckBox(activity).apply { text = "启用供应商"; isChecked = old?.optBoolean("enabled", true) ?: true }
        listOf(name, protocol, endpoint, apiVersion, key, enabled).forEach { if (it is EditText) fields.addView(label(it.hint.toString(), 12f, Palette.muted)); fields.addView(it) }
        protocol.onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
            override fun onNothingSelected(parent: AdapterView<*>?) {}
            override fun onItemSelected(parent: AdapterView<*>?, v: android.view.View?, position: Int, id: Long) { apiVersion.visibility = if (protocol.selectedItem == "azure_openai") android.view.View.VISIBLE else android.view.View.GONE }
        }
        AlertDialog.Builder(activity).setTitle("供应商连接").setView(scroll(fields)).setNegativeButton("取消", null).setPositiveButton("保存") { _, _ ->
            val config = copy(); val pid = name.text.toString(); val provider = old?.let { JSONObject(it.toString()) } ?: JSONObject().put("id", pid).put("name", pid).put("credential_revision", 0).put("enabled", true)
            provider.put("enabled", enabled.isChecked)
            provider.put("connection", JSONObject().put("protocol", protocol.selectedItem.toString()).put("endpoint", endpoint.text.toString()).put("api_version", apiVersion.text.toString().takeIf { protocol.selectedItem == "azure_openai" && it.isNotEmpty() }))
            config.getJSONObject("providers").put(pid, provider)
            val secrets = JSONObject(); if (key.text.isNotEmpty()) secrets.put(pid, key.text.toString()); key.setText(""); save(config, secrets)
        }.show()
    } }
    private fun model(id: String?) { with(activity) {
        val config = copy(); val providers = config.getJSONObject("providers").keys().asSequence().toList()
        if (providers.isEmpty()) { status.text = "请先添加供应商"; return }
        val old = id?.let { config.getJSONObject("models").getJSONObject(it) }
        val fields = column(12); val name = field("配置 ID").apply { setText(id.orEmpty()); isEnabled = id == null }
        val provider = spinner(providers, old?.optString("provider_id").orEmpty())
        val model = field("模型 ID / Azure deployment").apply { setText(old?.optString("model").orEmpty()) }
        val context = field("上下文窗口").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText((old?.optLong("context_window") ?: 128000).toString()) }
        val output = field("最大输出 tokens").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText((old?.optLong("max_tokens") ?: 4096).toString()) }
        val temperature = field("温度（可留空）").apply { inputType=InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL;setText(old?.opt("temperature")?.takeUnless {it==JSONObject.NULL}?.toString().orEmpty()) }
        val topP = field("Top P（可留空）").apply { inputType=InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL;setText(old?.opt("top_p")?.takeUnless {it==JSONObject.NULL}?.toString().orEmpty()) }
        val caps = old?.optJSONObject("capabilities")?.let { JSONObject(it.toString()) } ?: JSONObject()
        val tools = CheckBox(activity).apply { text = "已确认模型支持工具"; isChecked = caps.optBoolean("tools") }
        val vision = CheckBox(activity).apply { text = "已确认模型支持图片"; isChecked = caps.optBoolean("vision") }
        val levels = field("供应商声明的思考等级（逗号分隔）").apply { setText(caps.optJSONArray("reasoning_levels")?.let { a -> (0 until a.length()).joinToString(",") { a.getString(it) } }.orEmpty()) }
        val modes = spinner(listOf("provider_default", "level", "budget", "adaptive", "disabled"), old?.optJSONObject("reasoning")?.optString("mode").orEmpty())
        val strength = field("思考等级 / token 预算").apply { setText(old?.optJSONObject("reasoning")?.let { it.optString("level").ifEmpty { it.optString("tokens") } }.orEmpty()) }
        val budgetMin = field("支持的预算下限").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText(caps.optJSONArray("reasoning_budget")?.optLong(0)?.toString().orEmpty()) }
        val budgetMax = field("支持的预算上限").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText(caps.optJSONArray("reasoning_budget")?.optLong(1)?.toString().orEmpty()) }
        val adaptive = CheckBox(activity).apply { text = "供应商支持 adaptive"; isChecked = caps.optBoolean("reasoning_adaptive") }
        val disabled = CheckBox(activity).apply { text = "供应商支持关闭思考"; isChecked = caps.optBoolean("reasoning_disabled") }
        model.addTextChangedListener(watcher {
            modes.setSelection(0);strength.setText("");levels.setText("");budgetMin.setText("");budgetMax.setText("");tools.isChecked=false;vision.isChecked=false;adaptive.isChecked=false;disabled.isChecked=false
        })
        val binding = spinner(listOf("不改默认", "全局默认", "终端默认") + if (session.isNotEmpty()) listOf("当前终端") else emptyList())
        fields.addView(label("配置 ID", 12f, Palette.muted)); fields.addView(name); fields.addView(label("供应商", 12f, Palette.muted)); fields.addView(provider); fields.addView(label("模型 ID / Azure deployment", 12f, Palette.muted)); fields.addView(model)
        fields.addView(actionButton("搜索供应商模型目录") {
            val search = field("搜索模型")
            fun discover(cursor: String? = null) {
                val command = JSONObject().put("action", "discover").put("provider", provider.selectedItem.toString()).put("search", search.text.toString()).put("refresh", cursor == null)
                cursor?.let { command.put("cursor", it) }
                run(command) { page ->
                    val models = page.getJSONArray("models"); val names = (0 until models.length()).map { models.getJSONObject(it).getString("id") }
                    val next = page.optString("cursor").takeUnless { it.isEmpty() || it == "null" }
                    AlertDialog.Builder(activity).setTitle("选择模型").setItems((names + if (next != null) listOf("下一页") else emptyList()).toTypedArray()) { _, index ->
                        if (index == names.size) discover(next) else {
                            val selected=models.getJSONObject(index);model.setText(names[index]);modes.setSelection(0);strength.setText("")
                            selected.optLong("context_window").takeIf{it>0}?.let{context.setText(it.toString())}
                            selected.optLong("max_output_tokens").takeIf{it>0}?.let{output.setText(it.toString())}
                            val metadata=selected.optJSONObject("capabilities") ?: JSONObject()
                            tools.isChecked=metadata.optBoolean("tools");vision.isChecked=metadata.optBoolean("vision")
                            levels.setText(metadata.optJSONArray("reasoning_levels")?.let { a -> (0 until a.length()).joinToString(","){a.getString(it)} }.orEmpty())
                            val budget=metadata.optJSONArray("reasoning_budget");budgetMin.setText(budget?.optLong(0)?.toString().orEmpty());budgetMax.setText(budget?.optLong(1)?.toString().orEmpty())
                            adaptive.isChecked=metadata.optBoolean("reasoning_adaptive");disabled.isChecked=metadata.optBoolean("reasoning_disabled")
                        }
                    }.setNegativeButton("取消", null).show()
                }
            }
            AlertDialog.Builder(activity).setTitle("模型目录").setView(search).setPositiveButton("搜索") { _, _ -> discover() }.setNegativeButton("取消", null).show()
        })
        fun addLabelled(container: LinearLayout, control: android.view.View, title: String? = null) {
            if (control is EditText) container.addView(label(title ?: control.hint.toString(), 12f, Palette.muted))
            else if (title != null) container.addView(label(title, 12f, Palette.muted))
            container.addView(control)
        }
        listOf(context, output, tools, vision).forEach { addLabelled(fields, it) }
        val advanced = column().apply { visibility = android.view.View.GONE }
        fields.addView(actionButton("高级参数与思考能力") { advanced.visibility = if (advanced.visibility == android.view.View.GONE) android.view.View.VISIBLE else android.view.View.GONE })
        listOf(temperature, topP, levels, adaptive, disabled, budgetMin, budgetMax).forEach { addLabelled(advanced, it) }
        addLabelled(advanced, modes, "思考模式"); addLabelled(advanced, strength)
        fields.addView(advanced); addLabelled(fields, binding, "默认模型绑定")
        var previousProvider = provider.selectedItemPosition
        provider.onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
            override fun onNothingSelected(parent: AdapterView<*>?) {}
            override fun onItemSelected(parent: AdapterView<*>?, v: android.view.View?, position: Int, id: Long) {
                if (position != previousProvider) { previousProvider = position; model.setText("") }
            }
        }
        AlertDialog.Builder(activity).setTitle("模型与思考强度").setView(scroll(fields)).setNegativeButton("取消", null).setPositiveButton("保存") { _, _ ->
            try {
                val mid = name.text.toString(); val m = old?.let { JSONObject(it.toString()) } ?: JSONObject().put("id", mid).put("name", mid)
                caps.put("tools", tools.isChecked).put("vision", vision.isChecked).put("source", "user_declared").put("reasoning_levels", JSONArray(levels.text.toString().split(',').map { it.trim() }.filter { it.isNotEmpty() })).put("reasoning_adaptive", adaptive.isChecked).put("reasoning_disabled", disabled.isChecked)
                if (budgetMin.text.isNotEmpty() && budgetMax.text.isNotEmpty()) caps.put("reasoning_budget", JSONArray(listOf(budgetMin.text.toString().toLong(), budgetMax.text.toString().toLong()))) else caps.remove("reasoning_budget")
                val reasoning = JSONObject().put("mode", modes.selectedItem.toString())
                if (modes.selectedItem == "level") reasoning.put("level", strength.text.toString())
                if (modes.selectedItem == "budget") reasoning.put("tokens", strength.text.toString().toLong())
                m.put("provider_id", provider.selectedItem.toString()).put("model", model.text.toString()).put("context_window", context.text.toString().toLong()).put("max_tokens", output.text.toString().toLong()).put("capabilities", caps).put("read_only", !tools.isChecked).put("reasoning", reasoning)
                m.put("temperature",temperature.text.toString().takeIf{it.isNotEmpty()}?.toDouble()).put("top_p",topP.text.toString().takeIf{it.isNotEmpty()}?.toDouble())
                config.getJSONObject("models").put(mid, m)
                if(old!=null && (old.optString("model")!=model.text.toString() || old.optString("provider_id")!=provider.selectedItem.toString())) {
                    val bindings=config.getJSONObject("bindings");bindings.keys().forEach { scope -> val value=bindings.getJSONObject(scope);if(value.optString("model_id")==mid)value.remove("reasoning") }
                }
                val key = when (binding.selectedItemPosition) { 1 -> "global"; 2 -> "session-default"; 3 -> "session/$session"; else -> null }
                key?.let { config.getJSONObject("bindings").put(it, JSONObject().put("model_id", mid).put("reasoning", reasoning)) }; save(config)
            } catch (e: Exception) { status.text = "参数无效：${e.message}" }
        }.show()
    } }
    private fun mcp() { with(activity) {
        val input = field("{\"mcpServers\": {...}}").apply { isSingleLine = false; minLines = 5 }
        AlertDialog.Builder(activity).setTitle("导入 MCP JSON").setView(input).setNegativeButton("取消", null).setPositiveButton("保存") { _, _ ->
            try { val servers = JSONObject(input.text.toString()).getJSONObject("mcpServers"); val config = copy(); servers.keys().forEach { config.getJSONObject("mcp").put(it, servers.get(it)) }; save(config) }
            catch (e: Exception) { status.text = "JSON 无效：${e.message}" }
        }.show()
    } }
    private fun skill() { with(activity) {
        val form = column(12); val id = field("Skill ID"); val path = field("Desktop 上的绝对目录"); form.addView(id); form.addView(path)
        AlertDialog.Builder(activity).setTitle("安装 Codex Skill").setView(form).setNegativeButton("取消", null).setPositiveButton("安装") { _, _ ->
            run(JSONObject().put("action", "skill_install").put("id", id.text.toString()).put("path", path.text.toString()).put("expected_revision", view.getLong("revision"))) { load() }
        }.show()
    } }
    private fun upload(tree: android.net.Uri) {
        val resolver=activity.contentResolver
        val document=android.provider.DocumentsContract.getTreeDocumentId(tree)
        val field=activity.field("Skill ID")
        AlertDialog.Builder(activity).setTitle("导入 Skill 文件夹").setView(field).setNegativeButton("取消",null).setPositiveButton("上传") { _, _ ->
            val id=field.text.toString(); val revision=view.getLong("revision")
            worker.execute { try {
                val start=JSONObject(request(JSONObject().put("action","skill_upload_begin").put("id",id).put("expected_revision",revision).toString()))
                val token=start.getString("upload_id");var total=0L;var files=0
                fun walk(parent:String,prefix:String,depth:Int) {
                    check(depth<=8){"文件夹层级超过限制"}
                    val children=android.provider.DocumentsContract.buildChildDocumentsUriUsingTree(tree,parent)
                    resolver.query(children,arrayOf("document_id","_display_name","mime_type"),null,null,null)?.use { cursor ->
                        while(cursor.moveToNext()) {
                            val child=cursor.getString(0);val name=cursor.getString(1);val path=prefix+name
                            check(!name.contains('/') && name!=".." && name!="."){"无效文件名"}
                            if(cursor.getString(2)==android.provider.DocumentsContract.Document.MIME_TYPE_DIR) walk(child,"$path/",depth+1)
                            else {
                                files++;check(files<=256){"文件数量超过限制"}
                                resolver.openInputStream(android.provider.DocumentsContract.buildDocumentUriUsingTree(tree,child))!!.use { input ->
                                    var offset=0L;val buffer=ByteArray(49152)
                                    while(true) {
                                        val size=input.read(buffer);if(size<0 && offset>0)break
                                        val count=size.coerceAtLeast(0);total+=count;check(total<=8388608 && offset+count<=2097152){"Skill 文件过大"}
                                        val data=android.util.Base64.encodeToString(buffer,0,count,android.util.Base64.NO_WRAP)
                                        request(JSONObject().put("action","skill_upload_chunk").put("upload_id",token).put("path",path).put("offset",offset).put("data",data).toString())
                                        offset+=count;if(size<0)break
                                    }
                                }
                            }
                        }
                    }
                }
                walk(document,"",0)
                request(JSONObject().put("action","skill_upload_commit").put("upload_id",token).toString())
                activity.runOnUiThread{load()}
            } catch(e:Exception){activity.runOnUiThread{status.text="导入失败：${e.message}"}} }
        }.show()
    }
    private fun extension(kind: String, id: String) {
        AlertDialog.Builder(activity).setTitle(id).setItems(arrayOf("启用", "停用", "删除", "查看 / 编辑")) { _, index ->
            if (index == 3) { if (kind == "skills") run(JSONObject().put("action", "skill_read").put("id", id)) { result ->
                    val editor=activity.field("SKILL.md").apply{setText(result.optString("body"));isSingleLine=false;minLines=8}
                    AlertDialog.Builder(activity).setTitle("编辑用户 SKILL.md").setView(activity.scroll(activity.column(12).apply{addView(editor)})).setNegativeButton("取消",null).setPositiveButton("保存"){_,_->
                        run(JSONObject().put("action","skill_edit").put("id",id).put("path","SKILL.md").put("body",editor.text.toString()).put("expected_revision",view.getLong("revision"))){load()}
                    }.show()
                }
                else {
                    val editor = activity.field("MCP JSON").apply { setText(copy().getJSONObject(kind).getJSONObject(id).toString(2)); isSingleLine = false; minLines = 8 }
                    AlertDialog.Builder(activity).setTitle("编辑 MCP").setView(activity.scroll(editor)).setNegativeButton("取消", null).setPositiveButton("保存") { _, _ ->
                        try { val config = copy(); config.getJSONObject(kind).put(id, JSONObject(editor.text.toString())); save(config) } catch (e: Exception) { status.text = "JSON 无效：${e.message}" }
                    }.show()
                }
            } else { val config = copy(); if (index == 2) config.getJSONObject(kind).remove(id) else config.getJSONObject(kind).getJSONObject(id).put("enabled", index == 0); save(config) }
        }.show()
    }
}
