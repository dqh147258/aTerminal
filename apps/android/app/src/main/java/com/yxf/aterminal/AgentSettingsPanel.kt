package com.yxf.aterminal

import android.app.Activity
import android.app.AlertDialog
import android.text.InputType
import android.view.View
import android.widget.*
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.Executors

/** One snapshot and worker across full-page forms. Only the originating screen consumes a result. */
class AgentSettingsPanel(private val activity: Activity, private val body: LinearLayout,
    private val request: (String) -> String, private val session: String,
    private val pickFolder: (((android.net.Uri) -> Unit) -> Unit), private val page: String = "llm",
    private val titleChanged: (String) -> Unit = {}) {
    private val worker = Executors.newSingleThreadExecutor()
    private var view = JSONObject()
    private val host = activity.column()
    private val status = activity.label("正在读取 Desktop 设置…", 12f, Palette.muted).apply {
        setPadding(activity.dp(16), activity.dp(4), activity.dp(16), activity.dp(4))
        accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
    }
    private class Screen(val title: String, val fields: LinearLayout, val actions: LinearLayout, val clear: () -> Unit, val error: (String) -> Unit) {
        var busy = false
        var serial = 0
        val disabled = mutableListOf<View>()
    }
    private val screens = mutableListOf<Screen>()
    private var closed = false
    private val current get() = screens.lastOrNull()
    init {
        body.grow(host); body.addView(status)
        body.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
            override fun onViewAttachedToWindow(v: View) {}
            override fun onViewDetachedFromWindow(v: View) { close() }
        })
        status.setOnClickListener { if (screens.size <= 1 && current?.busy != true) load() }
        load()
    }
    fun close() { if (closed) return; closed = true; screens.forEach { it.clear() }; screens.clear(); worker.shutdown() }
    fun back(): Boolean {
        if (screens.size <= 1) return false
        dismissKeyboard(); screens.removeAt(screens.lastIndex).clear(); display()
        if (screens.size == 1) load()
        return true
    }
    private fun dismissKeyboard() {
        (activity.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager).hideSoftInputFromWindow(body.windowToken, 0)
        body.requestFocus()
    }
    private fun display() {
        val screen = current ?: return
        host.removeAllViews()
        (screen.fields.parent as? android.view.ViewGroup)?.removeView(screen.fields)
        (screen.actions.parent as? android.view.ViewGroup)?.removeView(screen.actions)
        host.grow(activity.scroll(screen.fields)); host.addView(screen.actions)
        titleChanged(screen.title); status.text = "修改在下次任务生效"
    }
    private fun push(title: String, fields: LinearLayout, clear: () -> Unit = {}, saveTitle: String = "保存", error: (String) -> Unit = {}, save: (() -> Unit)? = null) {
        val actions = activity.row().apply { setPadding(activity.dp(16), activity.dp(12), activity.dp(16), activity.dp(12)); setBackgroundColor(Palette.surface) }
        val screen = Screen(title, fields, actions, clear, error)
        if (save != null) {
            actions.fill(activity.actionButton("取消") { back() })
            actions.addView(View(activity), LinearLayout.LayoutParams(activity.dp(12), 1))
            actions.fill(activity.actionButton(saveTitle, true) { if (!screen.busy) save() }.apply { tag = "settings-save" })
        } else actions.visibility = View.GONE
        screens.add(screen); display()
    }
    private fun busy(screen: Screen, value: Boolean) {
        screen.busy = value
        fun disable(v: View) {
            if (v.isEnabled) { screen.disabled.add(v); v.isEnabled = false }
            if (v is android.view.ViewGroup) (0 until v.childCount).forEach { disable(v.getChildAt(it)) }
        }
        if (value) { disable(screen.fields); disable(screen.actions); status.text = "正在处理…" }
        else { screen.disabled.forEach { it.isEnabled = true }; screen.disabled.clear() }
    }
    private fun run(command: JSONObject, mutation: Boolean = false, done: (JSONObject) -> Unit) {
        if (closed || current?.busy == true) return
        val origin = current; val serial = origin?.let { ++it.serial }
        origin?.let { busy(it, true) }
        worker.execute {
            try {
                val result = JSONObject(request(command.toString()))
                val fresh = if (mutation) result else null
                activity.runOnUiThread {
                    if (!closed && current === origin && (origin == null || origin.serial == serial)) {
                        origin?.let { busy(it, false) }; fresh?.let { view = it }
                        status.text = if (mutation) "已保存，下次任务生效" else "修改在下次任务生效"
                        done(result)
                    }
                }
            } catch (e: Exception) {
                val conflict = e.message.orEmpty().contains("revision", true)
                val fresh = if (conflict) runCatching { JSONObject(request(JSONObject().put("action", "show").toString())) }.getOrNull() else null
                activity.runOnUiThread {
                    if (!closed && current === origin && (origin == null || origin.serial == serial)) {
                        origin?.let { busy(it, false) }; fresh?.let { view = it }
                        // Keep controls and non-sensitive draft intact. A second explicit save uses the refreshed revision.
                        status.text = if (conflict) "配置已变化，${if (fresh != null) "已刷新" else "刷新失败"}。请检查草稿后重新保存。" else "未完成：${e.message}"
                        if (!conflict) origin?.error?.invoke(e.message.orEmpty())
                    }
                }
            }
        }
    }
    private fun load() = run(JSONObject().put("action", "show")) { view = it; render() }
    private fun save(config: JSONObject, secrets: JSONObject = JSONObject(), done: () -> Unit = { render() }) =
        run(JSONObject().put("action", "replace").put("expected_revision", view.getLong("revision")).put("config", config).put("secrets", secrets), true) { done() }
    private fun copy() = JSONObject(view.getJSONObject("config").toString())
    private fun reject(field: EditText, message: String): Boolean { field.error = message; field.requestFocus(); status.text = message; return false }
    private fun validId(field: EditText, kind: String, editing: Boolean = false): Boolean {
        val id = field.text.toString()
        val check = if (kind == "mcp" || kind == "skills") id.removePrefix("user/") else id
        if (!check.matches(Regex("[A-Za-z0-9_-]{1,128}")) || check == "builtin" || ((kind == "mcp" || kind == "skills") && id.startsWith("builtin"))) return reject(field, "ID 须为 1–128 位字母、数字、- 或 _，不能使用内置 ID")
        if (editing && !view.getJSONObject("config").getJSONObject(kind).has(id)) return reject(field, "此配置已删除，请返回列表重新检查")
        if (!editing && view.getJSONObject("config").getJSONObject(kind).let { it.has(id) || (kind == "skills" && (it.has(check) || it.has("user/$check"))) }) return reject(field, "此 ID 已存在")
        return true
    }
    private fun spinner(values: List<String>, selected: String = "", titles: List<String> = values) = Spinner(activity).apply {
        adapter = object : ArrayAdapter<String>(activity, android.R.layout.simple_spinner_dropdown_item, values) {
            override fun getView(position: Int, convertView: View?, parent: android.view.ViewGroup): View = (super.getView(position, convertView, parent) as TextView).apply { text = titles[position]; setTextColor(Palette.text); textSize = 16f; setCompoundDrawablesRelativeWithIntrinsicBounds(0, 0, R.drawable.ic_arrow_down, 0); compoundDrawableTintList = android.content.res.ColorStateList.valueOf(Palette.muted) }
            override fun getDropDownView(position: Int, convertView: View?, parent: android.view.ViewGroup): View = (super.getDropDownView(position, convertView, parent) as TextView).apply { text = titles[position]; setTextColor(Palette.text); setBackgroundColor(Palette.control); minHeight = activity.dp(48) }
        }
        minimumHeight = activity.dp(50); background = activity.shape(Palette.surface, true)
        setSelection(values.indexOf(selected).coerceAtLeast(0))
    }
    private fun section(fields: LinearLayout, title: String) { fields.gap(20); fields.addView(activity.label(title, 12f, Palette.muted)); fields.gap(8) }
    private fun render() { with(activity) {
        dismissKeyboard()
        screens.forEach { it.clear() }; screens.clear()
        val fields = column(16); val config = view.getJSONObject("config")
        push(mapOf("reading" to "终端读取", "mcp" to "MCP", "skills" to "Skills")[page] ?: "LLM 大模型", fields)
        if (page == "reading") { reading(fields); return }
        if (page == "llm") {
            val binding = config.getJSONObject("bindings").let { if (session.isNotEmpty()) it.optJSONObject("session/$session") ?: it.optJSONObject("session-default") else it.optJSONObject("global") }
            val model = binding?.optString("model_id")?.let { config.getJSONObject("models").optJSONObject(it) }
            val provider = model?.optString("provider_id")?.let { config.getJSONObject("providers").optJSONObject(it) }
            fields.addView(settingsRow(model?.optString("model") ?: "尚未绑定模型", if (provider?.optBoolean("enabled", true) == false) "供应商已停用" else if (session.isNotEmpty()) "当前终端使用的模型" else "Global 默认模型", R.drawable.ic_cpu))
            section(fields, "供应商")
            config.getJSONObject("providers").let { items ->
                if (items.length() == 0) fields.addView(label("添加供应商以连接模型服务", 14f, Palette.muted))
                items.keys().forEach { id -> val item = items.getJSONObject(id); fields.addView(settingsRow(item.optString("name", id), protocolName(item.getJSONObject("connection").optString("protocol")) + if (item.optBoolean("enabled", true)) "" else " · 已停用") { provider(id) }); fields.settingsDivider() }
            }
            fields.gap(); fields.addView(actionButton("添加供应商") { provider(null) })
            section(fields, "模型")
            config.getJSONObject("models").let { items ->
                if (items.length() == 0) fields.addView(label("尚未添加模型", 14f, Palette.muted))
                items.keys().forEach { id -> val item = items.getJSONObject(id); fields.addView(settingsRow(item.optString("model", id), "$id · ${item.optString("provider_id")}") { model(id) }); fields.settingsDivider() }
            }
            fields.gap(); fields.addView(actionButton("添加模型") { model(null) })
            section(fields, "默认模型"); fields.addView(settingsRow("作用域绑定", "全局、终端默认、当前终端", R.drawable.ic_sliders_horizontal) { bindings() })
        } else {
            section(fields, "内置 · 只读")
            if (page == "mcp") fields.addView(settingsRow("Terminal", "builtin/terminal · 内置终端工具，只读", R.drawable.ic_lock))
            else fields.addView(settingsRow("终端与 Agent 能力", "截图、会话管理、Agent 管理、等待、历史定位 · 只读", R.drawable.ic_lock))
            section(fields, "已安装")
            val items = config.getJSONObject(page)
            if (items.length() == 0) fields.addView(settingsRow("暂无已安装的${if (page == "mcp") "服务" else " Skills"}", "添加后可在下次任务中使用"))
            items.keys().forEach { id -> val item = items.getJSONObject(id); fields.addView(settingsRow(item.optString("name", id), "$id · ${if (item.optBoolean("enabled", true)) "已启用" else "已停用"}") { extension(page, id) }); fields.settingsDivider() }
            fields.gap(20)
            if (page == "mcp") fields.addView(actionButton("导入 MCP JSON", true) { mcp() })
            else { fields.addView(actionButton("安装 Desktop 上的 Skill", true) { skill() }); fields.gap(); fields.addView(actionButton("从手机文件夹导入 Skill") {
                val origin = current; pickFolder { uri -> if (!closed && current === origin) upload(uri) }
            }) }
        }
    } }
    private fun reading(fields: LinearLayout) { with(activity) {
        val reading = copy().optJSONObject("terminal_reading") ?: JSONObject()
        val head = field("首部保留行数（1–100）").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText(reading.optInt("head_lines", 10).toString()) }
        val tail = field("尾部保留行数（1–100）").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText(reading.optInt("tail_lines", 20).toString()) }
        fields.addView(heading("终端读取锚点")); fields.labelled(head.hint.toString(), head); fields.labelled(tail.hint.toString(), tail)
        fields.addView(label("搜索排除已识别的动态 TUI 行，原文仍完整保留。", 12f, Palette.muted))
        fields.addView(actionButton("保存读取设置", true) {
            val h = head.text.toString().toIntOrNull(); val t = tail.text.toString().toIntOrNull()
            if (h == null || h !in 1..100) reject(head, "首部行数须在 1–100 之间")
            else if (t == null || t !in 1..100) reject(tail, "尾部行数须在 1–100 之间")
            else { val candidate = copy(); val updated = candidate.optJSONObject("terminal_reading") ?: JSONObject(); updated.put("head_lines", h).put("tail_lines", t); candidate.put("terminal_reading", updated); save(candidate) }
        })
    } }
    private fun bindings() { with(activity) {
        val config = copy(); val models = config.getJSONObject("models")
        val ids = models.keys().asSequence().toList()
        val fields = column(16)
        val scopes = listOf("global" to "Global 默认", "session-default" to "Session 默认") + if (session.isNotEmpty()) listOf("session/$session" to "当前终端覆盖") else emptyList()
        val choices = scopes.map { (scope, title) ->
            val values = listOf("") + ids
            val labels = listOf(if (scope.startsWith("session/")) "继承 Session 默认" else "未绑定") + ids.map { "${models.getJSONObject(it).optString("model")} · $it" }
            scope to spinner(values, config.getJSONObject("bindings").optJSONObject(scope)?.optString("model_id").orEmpty(), labels).also { fields.labelled(title, it) }
        }
        push("作用域绑定", fields, save = {
            val candidate = copy(); val bindings = candidate.getJSONObject("bindings")
            choices.forEach { (scope, input) ->
                val id = input.selectedItem.toString()
                if (id.isEmpty()) bindings.remove(scope)
                else if (bindings.optJSONObject(scope)?.optString("model_id") != id) bindings.put(scope, JSONObject().put("model_id", id))
            }
            save(candidate)
        })
    } }
    private val protocols = listOf("openai_responses", "openai_chat", "anthropic", "gemini", "azure_openai", "ollama")
    private fun protocolName(value: String) = mapOf("openai_responses" to "OpenAI Responses", "openai_chat" to "OpenAI Chat Completions", "anthropic" to "Anthropic", "gemini" to "Gemini", "azure_openai" to "Azure OpenAI", "ollama" to "Ollama")[value] ?: value
    private fun provider(id: String?) { with(activity) {
        val old = id?.let { copy().getJSONObject("providers").getJSONObject(it) }
        val fields = column(16)
        val name = field("供应商 ID").apply { setText(id.orEmpty()); isEnabled = id == null }
        val protocol = spinner(protocols, old?.getJSONObject("connection")?.optString("protocol").orEmpty(), protocols.map(::protocolName))
        val endpoint = field("API 地址").apply { setText(old?.getJSONObject("connection")?.optString("endpoint") ?: "https://api.openai.com/v1") }
        val apiVersion = field("Azure API version").apply { setText(old?.getJSONObject("connection")?.opt("api_version")?.takeUnless { it == JSONObject.NULL }?.toString().orEmpty()) }
        val key = field("API 密钥（留空保留）", true).apply { isSaveEnabled = false; if (android.os.Build.VERSION.SDK_INT >= 26) importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS }
        val enabled = CheckBox(activity).apply { buttonTintList = android.content.res.ColorStateList.valueOf(Palette.accent); text = "启用供应商"; setTextColor(Palette.text); isChecked = old?.optBoolean("enabled", true) ?: true }
        fields.labelled("供应商 ID *", name); fields.labelled("连接协议", protocol); fields.labelled("API 地址 *", endpoint)
        val azure = fields.labelled("Azure API version *", apiVersion)
        fun azureVisibility() { azure.visibility = if (protocol.selectedItem == "azure_openai") View.VISIBLE else View.GONE }
        azureVisibility()
        protocol.onItemSelectedListener = selected { azureVisibility() }
        fields.labelled("API 密钥", key); fields.addView(label("留空保留现有密钥。密钥仅在此编辑页面的内存中保留，离开即清除。", 12f, Palette.muted)); fields.gap(); fields.addView(enabled)
        push(if (id == null) "添加供应商" else "编辑供应商", fields, clear = { key.setText("") }, error = { message ->
            when {
                "endpoint" in message -> reject(endpoint, message)
                "api_version" in message -> reject(apiVersion, message)
                "id" in message -> reject(name, message)
            }
        }, save = saveForm@{
            if (!validId(name, "providers", id != null)) return@saveForm
            if (!SettingsValidation.endpoint(endpoint.text.toString())) { reject(endpoint, "请输入无用户名密码的 HTTP / HTTPS 地址"); return@saveForm }
            if (protocol.selectedItem == "azure_openai" && apiVersion.text.isBlank()) { reject(apiVersion, "Azure 需要 API version"); return@saveForm }
            val config = copy(); val pid = name.text.toString()
            val provider = config.getJSONObject("providers").optJSONObject(pid)?.let { JSONObject(it.toString()) } ?: JSONObject().put("id", pid).put("name", pid).put("credential_revision", 0)
            val connection = provider.optJSONObject("connection") ?: JSONObject()
            connection.put("protocol", protocol.selectedItem.toString()).put("endpoint", endpoint.text.toString()).put("api_version", apiVersion.text.toString().takeIf { protocol.selectedItem == "azure_openai" && it.isNotEmpty() })
            provider.put("connection", connection).put("enabled", enabled.isChecked); config.getJSONObject("providers").put(pid, provider)
            val secrets = JSONObject(); if (key.text.isNotBlank()) secrets.put(pid, key.text.toString())
            save(config, secrets)
        })
    } }
    private fun selected(action: () -> Unit) = object : AdapterView.OnItemSelectedListener {
        override fun onNothingSelected(parent: AdapterView<*>?) {}
        override fun onItemSelected(parent: AdapterView<*>?, v: View?, position: Int, id: Long) { action() }
    }
    private fun model(id: String?) { with(activity) {
        val config = copy(); val providers = config.getJSONObject("providers").keys().asSequence().toList()
        if (providers.isEmpty()) { status.text = "请先添加供应商"; return }
        val old = id?.let { config.getJSONObject("models").getJSONObject(it) }
        val fields = column(16); val name = field("配置 ID").apply { setText(id.orEmpty()); isEnabled = id == null }
        val provider = spinner(providers, old?.optString("provider_id").orEmpty())
        val model = field("模型 ID / Azure deployment").apply { setText(old?.optString("model").orEmpty()) }
        val context = field("上下文窗口").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText((old?.optLong("context_window") ?: 128000).toString()) }
        val output = field("最大输出 tokens").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText((old?.optLong("max_tokens") ?: 4096).toString()) }
        val temperature = field("温度（可留空）").apply { inputType=InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL;setText(old?.opt("temperature")?.takeUnless {it==JSONObject.NULL}?.toString().orEmpty()) }
        val topP = field("Top P（可留空）").apply { inputType=InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL;setText(old?.opt("top_p")?.takeUnless {it==JSONObject.NULL}?.toString().orEmpty()) }
        var caps = old?.optJSONObject("capabilities")?.let { JSONObject(it.toString()) } ?: JSONObject()
        var capabilitiesFromSnapshot = old != null
        val tools = CheckBox(activity).apply { buttonTintList = android.content.res.ColorStateList.valueOf(Palette.accent); text = "已确认模型支持工具"; isChecked = caps.optBoolean("tools") }
        val vision = CheckBox(activity).apply { buttonTintList = android.content.res.ColorStateList.valueOf(Palette.accent); text = "已确认模型支持图片"; isChecked = caps.optBoolean("vision") }
        val levels = field("供应商声明的思考等级（逗号分隔）").apply { setText(caps.optJSONArray("reasoning_levels")?.let { a -> (0 until a.length()).joinToString(",") { a.getString(it) } }.orEmpty()) }
        val modes = spinner(listOf("provider_default", "level", "budget", "adaptive", "disabled"), old?.optJSONObject("reasoning")?.optString("mode").orEmpty(), listOf("跟随供应商", "指定等级", "Token 预算", "自适应", "关闭思考"))
        val strength = field("思考等级 / token 预算").apply { setText(old?.optJSONObject("reasoning")?.let { it.optString("level").ifEmpty { it.optString("tokens") } }.orEmpty()) }
        val budgetMin = field("支持的预算下限").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText(caps.optJSONArray("reasoning_budget")?.optLong(0)?.toString().orEmpty()) }
        val budgetMax = field("支持的预算上限").apply { inputType = InputType.TYPE_CLASS_NUMBER; setText(caps.optJSONArray("reasoning_budget")?.optLong(1)?.toString().orEmpty()) }
        val adaptive = CheckBox(activity).apply { buttonTintList = android.content.res.ColorStateList.valueOf(Palette.accent); text = "供应商支持 adaptive"; isChecked = caps.optBoolean("reasoning_adaptive") }
        val disabled = CheckBox(activity).apply { buttonTintList = android.content.res.ColorStateList.valueOf(Palette.accent); text = "供应商支持关闭思考"; isChecked = caps.optBoolean("reasoning_disabled") }
        model.addTextChangedListener(watcher {
            capabilitiesFromSnapshot = false
            caps = JSONObject();temperature.setText("");topP.setText("");modes.setSelection(0);strength.setText("");levels.setText("");budgetMin.setText("");budgetMax.setText("");tools.isChecked=false;vision.isChecked=false;adaptive.isChecked=false;disabled.isChecked=false
        })
        val binding = spinner(listOf("不改默认", "全局默认", "终端默认") + if (session.isNotEmpty()) listOf("当前终端") else emptyList())
        fields.labelled("配置 ID *", name); fields.labelled("供应商", provider); fields.labelled("模型 ID / Azure deployment *", model)
        fields.addView(actionButton("搜索供应商模型目录") {
            catalog(provider.selectedItem.toString()) { selected ->
                model.setText(selected.getString("id"))
                selected.optLong("context_window").takeIf { it > 0 }?.let { context.setText(it.toString()) }
                selected.optLong("max_output_tokens").takeIf { it > 0 }?.let { output.setText(it.toString()) }
                capabilitiesFromSnapshot = false
                caps = selected.optJSONObject("capabilities")?.let { JSONObject(it.toString()) } ?: JSONObject()
                tools.isChecked = caps.optBoolean("tools"); vision.isChecked = caps.optBoolean("vision")
                levels.setText(caps.optJSONArray("reasoning_levels")?.let { a -> (0 until a.length()).joinToString(",") { a.getString(it) } }.orEmpty())
                val budget = caps.optJSONArray("reasoning_budget")
                budgetMin.setText(budget?.optLong(0)?.toString().orEmpty()); budgetMax.setText(budget?.optLong(1)?.toString().orEmpty())
                adaptive.isChecked = caps.optBoolean("reasoning_adaptive"); disabled.isChecked = caps.optBoolean("reasoning_disabled")
            }
        })
        val advanced = column().apply { visibility = View.GONE }
        fields.gap(20)
        fields.addView(actionButton("高级参数与思考能力") { advanced.visibility = if (advanced.visibility == View.GONE) View.VISIBLE else View.GONE })
        listOf(context, output, temperature, topP, tools, vision, levels, adaptive, disabled, budgetMin, budgetMax).forEach {
            if (it is EditText) advanced.labelled(it.hint.toString(), it) else { (it as CheckBox).setTextColor(Palette.text); advanced.addView(it) }
        }
        fields.addView(advanced); fields.gap(20); fields.labelled("思考模式", modes)
        val modeError = label("", 12f, Palette.danger).apply { visibility = View.GONE }
        fields.addView(modeError)
        val strengthGroup = fields.labelled("思考等级 / token 预算", strength)
        fun showStrength() { strengthGroup.visibility = if (modes.selectedItem in listOf("level", "budget")) View.VISIBLE else View.GONE }
        showStrength(); modes.onItemSelectedListener = selected { showStrength(); modeError.visibility = View.GONE }
        fields.labelled("保存后绑定", binding)
        var previousProvider = provider.selectedItemPosition
        provider.onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
            override fun onNothingSelected(parent: AdapterView<*>?) {}
            override fun onItemSelected(parent: AdapterView<*>?, v: android.view.View?, position: Int, id: Long) {
                if (position != previousProvider) { previousProvider = position; model.setText("") }
            }
        }
        push(if (id == null) "添加模型" else "编辑模型", fields, error = { message ->
            advanced.visibility = View.VISIBLE
            when {
                "context" in message -> reject(output, message)
                "temperature" in message -> reject(temperature, message)
                "top_p" in message -> reject(topP, message)
                "reasoning" in message -> { modeError.text = message; modeError.visibility = View.VISIBLE }
                "model" in message -> reject(model, message)
            }
        }, save = saveForm@{
            if (!validId(name, "models", id != null)) return@saveForm
            try {
                val candidate = copy(); val mid = name.text.toString(); val m = candidate.getJSONObject("models").optJSONObject(mid)?.let { JSONObject(it.toString()) } ?: JSONObject().put("id", mid).put("name", mid)
                if (model.text.isEmpty() || model.text.toString().toByteArray().size > 256) { reject(model, "模型 ID 须为 1–256 字节"); return@saveForm }
                val ctx = context.text.toString().toLongOrNull()
                if (ctx == null || ctx !in 4096L..4000000L) { advanced.visibility = View.VISIBLE; reject(context, "上下文须在 4096–4000000 之间"); return@saveForm }
                val max = output.text.toString().toLongOrNull()
                if (max == null || max <= 0 || max >= ctx - 2048) { advanced.visibility = View.VISIBLE; reject(output, "输出须大于 0 且小于上下文减 2048"); return@saveForm }
                // Only untouched declarations inherit refreshed hidden fields. A catalog selection
                // (even the same ID), or an identity reset, owns its new/unknown capabilities.
                if (capabilitiesFromSnapshot) {
                    val latest = m.optJSONObject("capabilities")
                    listOf("streaming", "temperature", "top_p").forEach { key ->
                        if (latest?.has(key) == true) caps.put(key, latest.get(key)) else caps.remove(key)
                    }
                }
                fun sampling(field: EditText, top: Boolean): Boolean {
                    if (field.text.isEmpty()) return true
                    val value = field.text.toString().toDoubleOrNull()
                    if (value == null || !value.isFinite() || (if (top) value <= 0 || value > 1 else value < 0 || value > 2) || caps.opt(if (top) "top_p" else "temperature") == false) {
                        advanced.visibility = View.VISIBLE; return reject(field, if (top) "Top P 须大于 0 且不超过 1，并受模型支持" else "温度须在 0–2 之间，并受模型支持")
                    }
                    return true
                }
                if (!sampling(temperature, false) || !sampling(topP, true)) return@saveForm
                val lower = budgetMin.text.toString().toLongOrNull(); val upper = budgetMax.text.toString().toLongOrNull()
                if ((budgetMin.text.isNotEmpty() || budgetMax.text.isNotEmpty()) && (lower == null || upper == null || lower < 0 || upper < lower)) {
                    advanced.visibility = View.VISIBLE; reject(budgetMin, "预算上下限须为非负整数，且下限不超过上限"); return@saveForm
                }
                caps.put("tools", tools.isChecked).put("vision", vision.isChecked).put("source", "user_declared").put("reasoning_levels", JSONArray(levels.text.toString().split(',').map { it.trim() }.filter { it.isNotEmpty() })).put("reasoning_adaptive", adaptive.isChecked).put("reasoning_disabled", disabled.isChecked)
                if (budgetMin.text.isNotEmpty() && budgetMax.text.isNotEmpty()) caps.put("reasoning_budget", JSONArray(listOf(budgetMin.text.toString().toLong(), budgetMax.text.toString().toLong()))) else caps.remove("reasoning_budget")
                val reasoning = JSONObject().put("mode", modes.selectedItem.toString())
                if (modes.selectedItem == "level") reasoning.put("level", strength.text.toString())
                if (modes.selectedItem == "budget") {
                    val tokens = strength.text.toString().toLongOrNull()
                    if (tokens == null || tokens < 0) { reject(strength, "请输入有效的 token 预算"); return@saveForm }
                    reasoning.put("tokens", tokens)
                }
                val protocol = candidate.getJSONObject("providers").getJSONObject(provider.selectedItem.toString()).getJSONObject("connection").getString("protocol")
                val error = SettingsValidation.reasoning(protocol, reasoning, caps, max, temperature.text.isNotEmpty() || topP.text.isNotEmpty())
                if (error != null) { modeError.text = error; modeError.visibility = View.VISIBLE; status.text = error; if (strengthGroup.visibility == View.VISIBLE) reject(strength, error); return@saveForm }
                val identityChanged = old != null && (m.optString("model") != model.text.toString() || m.optString("provider_id") != provider.selectedItem.toString())
                m.put("provider_id", provider.selectedItem.toString()).put("model", model.text.toString()).put("context_window", context.text.toString().toLong()).put("max_tokens", output.text.toString().toLong()).put("capabilities", caps).put("read_only", !tools.isChecked || m.optBoolean("read_only", false)).put("reasoning", reasoning)
                m.put("temperature",temperature.text.toString().takeIf{it.isNotEmpty()}?.toDouble()).put("top_p",topP.text.toString().takeIf{it.isNotEmpty()}?.toDouble())
                candidate.getJSONObject("models").put(mid, m)
                if (identityChanged) {
                    val bindings=candidate.getJSONObject("bindings");bindings.keys().forEach { scope -> val value=bindings.getJSONObject(scope);if(value.optString("model_id")==mid)value.remove("reasoning") }
                }
                val key = when (binding.selectedItemPosition) { 1 -> "global"; 2 -> "session-default"; 3 -> "session/$session"; else -> null }
                key?.let { candidate.getJSONObject("bindings").put(it, JSONObject().put("model_id", mid).put("reasoning", reasoning)) }; save(candidate)
            } catch (e: Exception) { status.text = "参数无效：${e.message}" }
        })
    } }
    private fun catalog(provider: String, choose: (JSONObject) -> Unit) { with(activity) {
        val fields = column(16); val query = field("搜索模型")
        val results = column(); var cursor: String? = null
        val next = actionButton("下一页") {}.apply { visibility = View.GONE }
        fields.labelled("搜索模型", query)
        fun discover(token: String? = null) {
            val command = JSONObject().put("action", "discover").put("provider", provider).put("search", query.text.toString()).put("refresh", token == null)
            token?.let { command.put("cursor", it) }
            run(command) { result ->
                results.removeAllViews()
                val models = result.getJSONArray("models")
                if (models.length() == 0) results.addView(label("未找到模型", 14f, Palette.muted))
                for (index in 0 until models.length()) {
                    val item = models.getJSONObject(index)
                    results.addView(settingsRow(item.getString("id"), "${item.optLong("context_window").takeIf { it > 0 }?.toString() ?: "未知"} 上下文") {
                        back(); choose(item)
                    }); results.settingsDivider()
                }
                cursor = result.optString("cursor").takeUnless { it.isEmpty() || it == "null" }
                next.visibility = if (cursor != null) View.VISIBLE else View.GONE
            }
        }
        fields.addView(actionButton("搜索", true) { discover() }); fields.gap(); fields.addView(results); fields.gap(); fields.addView(next)
        next.setOnClickListener { cursor?.let { discover(it) } }
        // A changed query starts a new first page, never reuses a cursor for another search.
        query.addTextChangedListener(watcher { cursor = null; next.visibility = View.GONE })
        push("模型目录 · $provider", fields); discover()
    } }
    private fun mcp() { with(activity) {
        val fields = column(16)
        val input = field("{\"mcpServers\": {...}}").apply { isSingleLine = false; minLines = 8 }
        fields.labelled("MCP JSON", input); fields.addView(label("合并 mcpServers 中的用户服务；同名服务将更新。凭据引用保持原协议。", 12f, Palette.muted))
        push("导入 MCP JSON", fields, error = { reject(input, it) }, save = {
            try {
                val servers = JSONObject(input.text.toString()).getJSONObject("mcpServers")
                require(servers.length() > 0) { "mcpServers 不能为空" }
                val config = copy()
                servers.keys().forEach { id -> validateMcp(id, servers.getJSONObject(id)); config.getJSONObject("mcp").put(id, servers.get(id)) }
                save(config)
            } catch (e: Exception) { reject(input, "JSON 无效：${e.message}") }
        })
    } }
    private fun validateMcp(id: String, value: JSONObject) {
        require(!id.startsWith("builtin") && id.removePrefix("user/").matches(Regex("[A-Za-z0-9_-]{1,128}"))) { "无效的用户 MCP ID" }
        val command = value.opt("command")?.takeUnless { it == JSONObject.NULL }?.toString()
        val url = value.opt("url")?.takeUnless { it == JSONObject.NULL }?.toString()
        val transport = value.optString("transport").takeUnless { it.isEmpty() || it == "null" } ?: if (command != null && url == null) "stdio" else if (url != null && command == null) "streamable_http" else ""
        require(if (transport == "stdio") !command.isNullOrEmpty() && url == null else transport == "streamable_http" && command == null && url != null && SettingsValidation.endpoint(url)) { "检查 transport、command 和 url" }
        val args = value.optJSONArray("args") ?: JSONArray()
        val env = value.optJSONObject("env") ?: JSONObject(); val envRefs = value.optJSONObject("envSecretRefs") ?: JSONObject()
        val headers = value.optJSONObject("headers") ?: JSONObject(); val headerRefs = value.optJSONObject("headerSecretRefs") ?: JSONObject()
        require(args.length() <= 128 && (0 until args.length()).sumOf { args.getString(it).toByteArray().size } <= 16000 && env.length() + envRefs.length() <= 128 && headers.length() + headerRefs.length() <= 32) { "MCP 参数或凭据引用数量超过限制" }
        (env.keys().asSequence() + envRefs.keys().asSequence()).forEach { require(it.isNotEmpty() && '=' !in it && '\u0000' !in it) { "环境变量名无效" } }
        require(value.optLong("startup_timeout_ms", 10000) in 100..60000 && value.optLong("call_timeout_ms", 30000) in 100..300000) { "超时参数超出支持范围" }
    }
    private fun skill() { with(activity) {
        val fields = column(16); val id = field("Skill ID"); val path = field("Desktop 上的绝对目录")
        fields.labelled("Skill ID *", id); fields.labelled("Desktop 上的绝对目录 *", path)
        fields.addView(label("安装 Desktop 中的完整 Skill 文件夹，包括 SKILL.md 和所引用的资源。", 12f, Palette.muted))
        push("安装 Codex Skill", fields, saveTitle = "安装", error = { reject(path, it) }, save = {
            if (validId(id, "skills")) {
                if (!SettingsValidation.desktopAbsolutePath(path.text.toString())) reject(path, "请输入 Desktop 上的绝对路径")
                else run(JSONObject().put("action", "skill_install").put("id", id.text.toString()).put("path", path.text.toString()).put("expected_revision", view.getLong("revision")), true) { render() }
            }
        })
    } }
    private fun upload(tree: android.net.Uri) {
        val fields = activity.column(16); val id = activity.field("Skill ID")
        fields.labelled("Skill ID *", id)
        fields.addView(activity.label("上传完整文件夹：最多 256 个文件、8 层目录、单文件 2 MiB、总计 8 MiB。", 12f, Palette.muted))
        push("导入 Skill 文件夹", fields, saveTitle = "上传", save = uploadForm@{
            if (!validId(id, "skills")) return@uploadForm
            val origin = current ?: return@uploadForm
            val revision = view.getLong("revision"); val skillId = id.text.toString()
            busy(origin, true)
            worker.execute {
                try {
                    val resolver = activity.contentResolver
                    val document = android.provider.DocumentsContract.getTreeDocumentId(tree)
                    val start = JSONObject(request(JSONObject().put("action", "skill_upload_begin").put("id", skillId).put("expected_revision", revision).toString()))
                    val token = start.getString("upload_id"); var total = 0L; var files = 0
                    fun walk(parent: String, prefix: String, depth: Int) {
                        check(depth <= 8) { "文件夹层级超过限制" }
                        val children = android.provider.DocumentsContract.buildChildDocumentsUriUsingTree(tree, parent)
                        val cursor = resolver.query(children, arrayOf("document_id", "_display_name", "mime_type"), null, null, null) ?: error("无法读取文件夹")
                        cursor.use {
                            while (it.moveToNext()) {
                                val child = it.getString(0); val name = it.getString(1); val path = prefix + name
                                check(!name.contains('/') && name != ".." && name != ".") { "无效文件名" }
                                if (it.getString(2) == android.provider.DocumentsContract.Document.MIME_TYPE_DIR) walk(child, "$path/", depth + 1)
                                else {
                                    files++; check(files <= 256) { "文件数量超过限制" }
                                    val source = resolver.openInputStream(android.provider.DocumentsContract.buildDocumentUriUsingTree(tree, child)) ?: error("无法读取 $path")
                                    source.use { input ->
                                        var offset = 0L; val buffer = ByteArray(49152)
                                        while (true) {
                                            val size = input.read(buffer); if (size < 0 && offset > 0) break
                                            val count = size.coerceAtLeast(0); total += count
                                            check(total <= 8388608 && offset + count <= 2097152) { "Skill 文件过大" }
                                            val data = android.util.Base64.encodeToString(buffer, 0, count, android.util.Base64.NO_WRAP)
                                            request(JSONObject().put("action", "skill_upload_chunk").put("upload_id", token).put("path", path).put("offset", offset).put("data", data).toString())
                                            offset += count; if (size < 0) break
                                        }
                                    }
                                }
                            }
                        }
                    }
                    walk(document, "", 0)
                    val fresh = JSONObject(request(JSONObject().put("action", "skill_upload_commit").put("upload_id", token).toString()))
                    activity.runOnUiThread { if (!closed && current === origin) { busy(origin, false); view = fresh; render() } }
                } catch (e: Exception) {
                    val conflict = e.message.orEmpty().contains("revision", true)
                    val fresh = if (conflict) runCatching { JSONObject(request(JSONObject().put("action", "show").toString())) }.getOrNull() else null
                    activity.runOnUiThread { if (!closed && current === origin) {
                        busy(origin, false); fresh?.let { view = it }
                        status.text = if (conflict) "配置已变化，请检查后重新上传。" else "导入失败：${e.message}"
                    } }
                }
            }
        })
    }
    private fun extensionValue(kind: String, id: String, config: JSONObject = view.getJSONObject("config")): JSONObject? {
        val value = config.getJSONObject(kind).optJSONObject(id)
        if (value == null) status.text = "此扩展已删除，请返回列表重新检查"
        return value
    }
    private fun extension(kind: String, id: String) { with(activity) {
        val value = extensionValue(kind, id) ?: return
        val fields = column(16)
        fields.addView(settingsRow(value.optString("name", id), "$id · ${if (value.optBoolean("enabled", true)) "已启用" else "已停用"}", if (kind == "mcp") R.drawable.ic_plug else R.drawable.ic_book_open))
        fields.gap(20)
        fields.addView(label(if (kind == "skills") value.optString("description") else value.optString("command").takeUnless { it == "null" }.orEmpty().ifEmpty { value.optString("url") }, 14f, Palette.muted))
        fields.addView(actionButton("查看 / 编辑") readExtension@{
            val item = extensionValue(kind, id) ?: return@readExtension
            if (kind == "skills") run(JSONObject().put("action", "skill_read").put("id", id)) { result -> editExtension(kind, id, result.optString("body")) }
            else editExtension(kind, id, item.toString(2))
        }); fields.gap()
        // Retained labels keep their original intent after a revision-conflict refresh.
        val targetEnabled = !value.optBoolean("enabled", true)
        fields.addView(actionButton(if (targetEnabled) "启用" else "停用") toggleExtension@{
            val candidate = copy(); val item = extensionValue(kind, id, candidate) ?: return@toggleExtension
            item.put("enabled", targetEnabled)
            save(candidate) { render(); extension(kind, id) }
        }); fields.gap(24)
        fields.addView(actionButton("删除") deleteExtension@{
            if (extensionValue(kind, id) == null) return@deleteExtension
            val origin = current
            AlertDialog.Builder(activity).setTitle("删除 $id？").setMessage("从配置中移除此扩展，下次任务将不再使用。")
                .setNegativeButton("取消", null).setPositiveButton("删除") { _, _ -> if (!closed && current === origin) { val candidate = copy(); if (extensionValue(kind, id, candidate) != null) { candidate.getJSONObject(kind).remove(id); save(candidate) } } }.show()
        }.apply { setTextColor(Palette.danger) })
        push(if (kind == "mcp") "MCP 详情" else "Skill 详情", fields)
    } }
    private fun editExtension(kind: String, id: String, text: String) { with(activity) {
        if (extensionValue(kind, id) == null) return
        val fields = column(16)
        val editor = field(if (kind == "skills") "SKILL.md" else "MCP JSON").apply { setText(text); isSingleLine = false; minLines = 12 }
        fields.labelled(editor.hint.toString(), editor)
        push(if (kind == "skills") "编辑用户 SKILL.md" else "编辑 MCP", fields, error = { reject(editor, it) }, save = editForm@{
            if (extensionValue(kind, id) == null) return@editForm
            if (kind == "skills" && editor.text.toString().toByteArray().size > 512 * 1024) { reject(editor, "SKILL.md 不能超过 512 KiB"); return@editForm }
            if (kind == "skills") run(JSONObject().put("action", "skill_edit").put("id", id).put("path", "SKILL.md").put("body", editor.text.toString()).put("expected_revision", view.getLong("revision")), true) { render(); extension(kind, id) }
            else try { val item = JSONObject(editor.text.toString()); validateMcp(id, item); val candidate = copy(); candidate.getJSONObject(kind).put(id, item); save(candidate) { render(); extension(kind, id) } }
            catch (e: Exception) { reject(editor, "JSON 无效：${e.message}") }
        })
    } }
}
