package dev.aiterminal.app

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.Typeface
import android.os.Bundle
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.TextWatcher
import android.text.method.PasswordTransformationMethod
import android.view.Choreographer
import android.view.Gravity
import android.view.View
import android.view.MotionEvent
import android.view.WindowManager
import android.view.inputmethod.InputMethodManager
import android.widget.*
import org.json.JSONObject
import uniffi.ai_terminal_mobile.*
import java.util.concurrent.Executors

class MainActivity : Activity(), Choreographer.FrameCallback {
    private val remote = RemoteTerminal()
    private val account = Account()
    private val worker = Executors.newSingleThreadExecutor()
    private val historyWorker = Executors.newSingleThreadExecutor()
    private val ui = Handler(Looper.getMainLooper())
    private var active = false
    @Volatile private var generation = 0
    @Volatile private var accountEpoch = 0
    private var selected: String? = null
    private var controlled = false
    private var desktopAttached = false
    private var sessionExited = false
    private var connected = false
    private var accountName = ""
    private var lastHeartbeatAt = 0L
    private var serverUrl = ""
    private var deviceId = ""
    private var deviceName = ""
    private var devices = emptyList<AccountDevice>()
    private var sessions = emptyList<RemoteSession>()
    private lateinit var root: FrameLayout
    private lateinit var loginBox: LinearLayout
    private lateinit var workspace: LinearLayout
    private lateinit var accountBar: LinearLayout
    private lateinit var status: TextView
    private lateinit var sessionTitle: TextView
    private lateinit var sessionMeta: TextView
    private lateinit var connection: TextView
    private lateinit var dimensions: TextView
    private lateinit var empty: LinearLayout
    private lateinit var surface: HorizontalScrollView
    private lateinit var inputBox: LinearLayout
    private lateinit var keysBar: HorizontalScrollView
    private lateinit var store: PairingStore
    private lateinit var display: DisplayPreferences
    private var terminal: TerminalView? = null
    private var overlay: FrameLayout? = null
    private var overlayPanel: View? = null
    private var assistant: AssistantPanel? = null
    private var chatStore: ChatStore? = null
    private var drawerTab = false
    private var loginBusy = false
    private var deviceBusy = false
    private var sessionBusy = false
    private val uncertainSessions = mutableSetOf<Pair<String, String>>()
    private var toast: Toast? = null
    private var memory: WorkspaceMemory? = null
    private var restorePending = false
    private var connecting = false
    private var aiBusy = false
    private var sessionRefreshBusy = false
    private var refreshDrawer: (() -> Unit)? = null
    private val drawerRefresh = object : Runnable {
        override fun run() {
            if (active && connected && overlay?.tag == "drawer") {
                if (!drawerTab) refreshSessions()
                ui.postDelayed(this, 3000)
            }
        }
    }
    private lateinit var aiStatus: TextView
    private var edgeStartX = 0f
    private var edgeStartY = 0f
    private val terminalTest get() = BuildConfig.TERMINAL_DEBUG && intent.getBooleanExtra("terminal_input_test", false)
    private val localDebugAutoLogin get() = BuildConfig.TERMINAL_DEBUG && intent.getBooleanExtra("local_debug_autologin", false)
    private val localDebugDesktop get() = intent.getStringExtra("local_debug_desktop") ?: "Local Desktop 新版"
    private val testMode get() = BuildConfig.TERMINAL_DEBUG && (intent.getBooleanExtra("acceptance_test", false) || terminalTest || localDebugAutoLogin)
    private val isolatedUi get() = BuildConfig.TERMINAL_DEBUG && intent.getBooleanExtra("isolated_ui", false)
    private val connectionPreferences get() = getSharedPreferences(if (terminalTest) "terminal-input-connection" else if (localDebugAutoLogin) "connection" else if (testMode || isolatedUi) "acceptance-connection" else "connection", MODE_PRIVATE)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.statusBarColor = Palette.background
        window.navigationBarColor = Palette.background
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
        store = PairingStore(this, if (terminalTest) "terminal-input-account" else if (localDebugAutoLogin) "account" else if (testMode || isolatedUi) "acceptance-account" else "account")
        display = DisplayPreferences(this, if (terminalTest) "terminal-input-display" else if (localDebugAutoLogin) "display" else if (testMode || isolatedUi) "acceptance-display" else "display")
        serverUrl = if (isolatedUi) "" else connectionPreferences.getString("server", "") ?: ""
        root = FrameLayout(this).apply {
            isFocusableInTouchMode = true
            setBackgroundColor(Palette.background)
            setOnApplyWindowInsetsListener { view, insets ->
                view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                    insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
                insets.consumeSystemWindowInsets()
            }
        }
        buildWorkspace()
        buildLogin()
        setContentView(root)
        if (BuildConfig.TERMINAL_DEBUG && intent.getBooleanExtra("render_fixture", false)) {
            (loginBox.parent as View).visibility = View.GONE; workspace.visibility = View.VISIBLE
            val replica = TerminalReplica()
            replica.applySnapshot(assets.open("screen.pb").use { it.readBytes() })
            replica.frame()?.let { show(it) }; replica.close()
        } else if (localDebugAutoLogin) {
            autoLoginLocalDebug()
        } else if (!isolatedUi) {
            val epoch = accountEpoch
            work {
                store.load()?.let { value ->
                    account.restore(value)
                    val name = account.username()
                    val exported = JSONObject(value)
                    val restoredServer = exported.optString("server", serverUrl)
                    post { if (epoch == accountEpoch) signedIn(name, restoredServer) }
                    loadDevices(epoch)
                }
            }
        }
    }

    private fun localDebugStatus(state: String, message: String = "") {
        if (!localDebugAutoLogin) return
        val data = JSONObject().put("state", state).put("message", message.take(200)).toString()
        val pending = java.io.File(filesDir, "local-debug-status.tmp")
        pending.writeText(data)
        check(pending.renameTo(java.io.File(filesDir, "local-debug-status.json")))
    }
    private fun autoLoginLocalDebug() {
        val fixture = java.io.File(filesDir, "local-debug-login.json")
        try {
            val config = JSONObject(fixture.readText())
            val server = ChatStore.canonicalServer(config.getString("server"))
            require(server == "https://192.168.0.36:7200") { "本地调试服务地址不匹配" }
            val username = config.getString("username")
            val password = config.getString("password")
            require(username.isNotBlank() && password.isNotEmpty()) { "本地测试账号不完整" }
            val epoch = ++accountEpoch
            localDebugStatus("starting")
            worker.execute {
                try {
                    val saved = store.load()
                    if (saved == null) {
                        val ca = java.io.File(filesDir, "acceptance-ca.pem").readText()
                        account.login(server, username, password, Build.MODEL, "android", ca)
                    } else {
                        val previous = JSONObject(saved)
                        check(previous.optString("server") == server && previous.optJSONObject("tokens")?.optString("username") == username) {
                            "设备已登录其他账号，请先在 App 中退出"
                        }
                        account.restore(saved)
                    }
                    persist()
                    connectionPreferences.edit().putString("server", server).apply()
                    post { if (epoch == accountEpoch) signedIn(username, server) }
                    loadDevices(epoch)
                } catch (error: Exception) {
                    localDebugStatus("error", error.message ?: "本地设备登录失败")
                    post { notice(error.message ?: "本地设备登录失败") }
                }
            }
        } catch (error: Exception) {
            localDebugStatus("error", error.message ?: "本地调试凭据无效")
            notice(error.message ?: "本地调试凭据无效")
        } finally { fixture.delete() }
    }

    private fun buildLogin() {
        loginBox = column(24)
        val brand = row().apply {
            addView(ImageView(this@MainActivity).apply {
                setImageResource(R.drawable.ic_terminal); imageTintList = ColorStateList.valueOf(Palette.background)
                background = shape(Palette.accent); setPadding(dp(6), dp(6), dp(6), dp(6))
                layoutParams = LinearLayout.LayoutParams(dp(36), dp(36)); importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            })
            addView(label("   AI TERMINAL", 14f))
        }
        loginBox.addView(brand); loginBox.gap(24)
        loginBox.addView(label("YOUR WORKSPACE, CONNECTED.", 12f, Palette.muted))
        loginBox.addView(label("AI Terminal", 32f).apply { setTypeface(typeface, Typeface.BOLD) })
        loginBox.addView(label("登录，回到你的工作现场。", 16f, Palette.muted)); loginBox.gap(20)
        val server = field("服务器 https://…").apply { setText(serverUrl); inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_URI; if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) setAutofillHints(View.AUTOFILL_HINT_USERNAME) }
        val username = field("账号").apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) setAutofillHints(View.AUTOFILL_HINT_USERNAME) }
        val password = field("密码", true).apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) setAutofillHints(View.AUTOFILL_HINT_PASSWORD) }
        loginBox.addView(label("服务地址")); loginBox.addView(server); loginBox.gap(16)
        loginBox.addView(label("账号")); loginBox.addView(username); loginBox.gap(16)
        loginBox.addView(label("密码"))
        loginBox.addView(row().apply {
            fill(password)
            addView(iconButton(R.drawable.ic_eye, "显示密码") {
                val visible = password.transformationMethod is PasswordTransformationMethod
                password.transformationMethod = if (visible) null else PasswordTransformationMethod.getInstance()
                password.setSelection(password.length())
                getChildAt(1).contentDescription = if (visible) "隐藏密码" else "显示密码"
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) getChildAt(1).tooltipText = getChildAt(1).contentDescription
                (getChildAt(1) as ImageButton).setImageResource(if (visible) R.drawable.ic_eye_off else R.drawable.ic_eye)
            })
        })
        val error = label("", 14f, Palette.danger).apply { visibility = View.GONE; accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE }
        loginBox.addView(error)
        val progress = ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal).apply { isIndeterminate = true; visibility = View.GONE }
        loginBox.addView(progress); loginBox.gap(12)
        lateinit var submit: Button
        submit = actionButton("登录", true) {
            if (!loginBusy) {
                try {
                    val url = ChatStore.canonicalServer(server.text.toString())
                    val name = username.text.toString().trim()
                    val secret = password.text.toString()
                    require(name.isNotEmpty()) { "请输入账号" }; require(secret.isNotEmpty()) { "请输入密码" }
                    loginBusy = true; error.text = ""; error.visibility = View.GONE; submit.isEnabled = false; submit.text = "正在连接工作空间"; progress.visibility = View.VISIBLE
                    hideKeyboard(); val epoch = ++accountEpoch
                    worker.execute {
                        try {
                            // The local deployment test pins its private CA without changing normal login trust.
                            val testCa = if (testMode) {
                                java.io.File(filesDir, "acceptance-ca.pem").takeIf { it.isFile }?.readText().orEmpty()
                            } else ""
                            account.login(url, name, secret, android.os.Build.MODEL, "android", testCa)
                            persist()
                            connectionPreferences.edit().putString("server", url).apply()
                            val actualName = account.username()
                            post { if (epoch == accountEpoch) { password.setText(""); signedIn(actualName, url) } }
                            loadDevices(epoch)
                        } catch (e: Exception) {
                            post { if (epoch == accountEpoch) {
                                if (accountName.isNotEmpty()) notice(e.message ?: "读取设备失败，请刷新")
                                else { error.visibility = View.VISIBLE; error.text = e.message ?: "登录失败，请重试" }
                            } }
                        } finally {
                            post { loginBusy = false; submit.isEnabled = true; submit.text = "登录"; progress.visibility = View.GONE }
                        }
                    }
                } catch (e: Exception) { error.visibility = View.VISIBLE; error.text = e.message }
            }
        }
        loginBox.addView(submit, LinearLayout.LayoutParams(-1, dp(52)))
        loginBox.gap(32)
        loginBox.addView(label("桌面工作，随身连接", 12f, Palette.muted))
        root.addView(scroll(loginBox), FrameLayout.LayoutParams(-1, -1))
    }

    private fun buildWorkspace() {
        workspace = column().apply { visibility = View.GONE }
        val header = column(4)
        sessionTitle = heading("AI Terminal").apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END }
        sessionMeta = label("选择 Desktop", 12f, Palette.muted).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END }
        connection = label("未连接", 12f, Palette.muted)
        header.addView(row().apply {
            addView(iconButton(R.drawable.ic_panel_left, "打开工作空间") { openDrawer() })
            fill(column(8).apply { addView(sessionTitle); addView(sessionMeta) })
            addView(iconButton(R.drawable.ic_user_round, "账号与设备") { accountPanel() })
        })
        workspace.addView(header); workspace.addView(divider())
        val region = FrameLayout(this)
        var scrollStartX = 0f
        surface = HorizontalScrollView(this).apply {
            isFillViewport = true; contentDescription = "远程终端，可横向滚动"
            setOnTouchListener { _, event ->
                if (event.actionMasked == MotionEvent.ACTION_DOWN) scrollStartX = event.x
                if (event.actionMasked == MotionEvent.ACTION_MOVE && kotlin.math.abs(event.x - scrollStartX) > dp(8)) terminal?.stopFollowingCursor()
                false
            }
        }
        var scrollStartY = 0f
        region.addView(scroll(surface).apply {
            setOnTouchListener { _, event ->
                if (event.actionMasked == MotionEvent.ACTION_DOWN) scrollStartY = event.y
                if (event.actionMasked == MotionEvent.ACTION_MOVE && kotlin.math.abs(event.y - scrollStartY) > dp(8)) terminal?.stopFollowingCursor()
                false
            }
        }, FrameLayout.LayoutParams(-1, -1))
        empty = column(24).apply {
            gravity = Gravity.CENTER
            addView(label("尚未连接终端", 18f)); addView(actionButton("选择设备", true) { accountPanel() })
        }
        region.addView(empty, FrameLayout.LayoutParams(-1, -1))
        val tools = column(4).apply {
            background = shape(Palette.surface, true)
            addView(iconButton(R.drawable.ic_sliders_horizontal, "终端设置") { settingsPanel() }); gap(6)
            addView(iconButton(R.drawable.ic_message_circle, "AI 对话") { openChat() }); gap(6)
            addView(iconButton(R.drawable.ic_keyboard, "显示或隐藏终端键盘") { toggleInput() })
        }
        region.addView(tools, FrameLayout.LayoutParams(-2, -2, Gravity.END or Gravity.CENTER_VERTICAL).apply { marginEnd = dp(12) })
        workspace.grow(region)
        inputBox = column(8).apply { visibility = View.GONE; setBackgroundColor(Palette.surface) }
        inputBox.addView(row().apply {
            addView(actionButton("历史") { terminalHistory() })
            addView(iconButton(R.drawable.ic_x, "隐藏终端键盘") { toggleInput(false) })
        })
        val keys = row()
        for ((name, key) in listOf("回车" to "enter", "Tab" to "tab", "退格" to "backspace", "Ctrl-C" to "ctrl_c", "Esc" to "escape", "↑" to "up", "↓" to "down", "←" to "left", "→" to "right")) {
            keys.addView(actionButton(name) { enqueue { remote.sendKey(key) } })
        }
        keysBar = HorizontalScrollView(this).apply { addView(keys); visibility = View.GONE }
        inputBox.addView(keysBar)
        workspace.addView(inputBox)
        workspace.addView(divider())
        val footer = column(6)
        status = label("登录后选择 Desktop", 12f, Palette.muted).apply { accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE }
        dimensions = label("UTF-8", 12f, Palette.muted)
        footer.addView(row().apply { fill(status); addView(connection) })
        aiStatus = label("", 12f, Palette.accent).apply { visibility = View.GONE; setOnClickListener { openChat() } }
        footer.addView(row().apply { fill(aiStatus); addView(dimensions) })
        workspace.addView(footer)
        root.addView(workspace, FrameLayout.LayoutParams(-1, -1))
    }

    private fun divider() = View(this).apply { setBackgroundColor(Palette.line); layoutParams = LinearLayout.LayoutParams(-1, dp(1)) }
    private fun post(action: () -> Unit) { ui.post { if (!isDestroyed) action() } }
    private fun work(action: () -> Unit) { worker.execute { try { action() } catch (e: Exception) { post { notice(e.message ?: "操作失败") } } } }
    private fun notice(message: String) {
        status.text = message; toast?.cancel()
        toast = Toast.makeText(this, message, Toast.LENGTH_LONG).also { it.show() }
    }
    private fun persist() { val value = account.export(); if (value.isEmpty()) store.clear() else store.save(value) }
    private fun signedIn(name: String, server: String) {
        accountName = name; serverUrl = server; chatStore = ChatStore(this, server, name, if (terminalTest) "terminal-input" else "")
        memory = WorkspaceMemory(this, server, name, if (terminalTest) "terminal-input" else ""); restorePending = true
        (loginBox.parent as View).visibility = View.GONE; workspace.visibility = View.VISIBLE
        status.text = "$name · 选择在线 Desktop"
    }
    private fun loadDevices(epoch: Int = accountEpoch) {
        try {
            val list = account.devices()
            post { if (epoch == accountEpoch) {
                devices = list; deviceBusy = false
                if (!connected) status.text = "${accountName} · ${list.count { it.platform == "desktop" && it.online }} 台 Desktop 在线"
                if (overlay?.tag == "account") accountPanel()
                if (localDebugAutoLogin && !connected && !connecting) {
                    val desktop = list.firstOrNull { it.platform == "desktop" && it.online && it.name == localDebugDesktop }
                    if (desktop == null) localDebugStatus("error", "$localDebugDesktop 未在线") else connectDevice(desktop.id)
                } else restoreLastSession()
            } }
        } finally { persist() }
    }
    private fun connectDevice(id: String, resumeSession: String? = null, resumeChat: Boolean = false) {
        disconnect(); closeOverlay()
        deviceId = id; deviceName = devices.firstOrNull { it.id == id }?.name ?: "Desktop"
        connecting = true; restorePending = false
        val version = generation; val epoch = accountEpoch
        status.text = "连接 $deviceName…"; connection.text = "连接中"
        worker.execute {
            try {
                if (generation != version || epoch != accountEpoch) return@execute
                account.connect(id, remote)
                val list = remote.sessions()
                post { if (active && generation == version && epoch == accountEpoch) {
                    connecting = false; connected = true; sessions = list; uncertainSessions.removeAll { it.first == id }; memory?.record(id, list.associate { it.id to it.exited }); connection.text = "已连接"; connection.setTextColor(Palette.green)
                    if (localDebugAutoLogin) localDebugStatus("ready", "已连接 $deviceName")
                    val target = list.firstOrNull { it.id == resumeSession && !it.exited } ?: if (resumeSession == null) list.firstOrNull { !it.exited } else null
                    if (target != null) select(target.id, true, resumeChat)
                    else { status.text = if (resumeSession == null) "已连接，创建或选择会话" else "原会话已关闭，历史仍可查阅"; openDrawer() }
                } }
            } catch (e: Exception) { post { if (generation == version) { connecting = false; connection.text = "未连接"; if (localDebugAutoLogin) localDebugStatus("error", e.message ?: "连接失败"); notice(e.message ?: "连接失败") } } }
            finally { persist() }
        }
    }
    private fun refreshSessions(manual: Boolean = false) {
        if (!connected) { if (manual) notice("请先连接 Desktop"); return }
        if (sessionRefreshBusy) return
        sessionRefreshBusy = true
        val version = generation; val device = deviceId
        worker.execute {
            try {
                val list = remote.sessions()
                post {
                    sessionRefreshBusy = false
                    if (generation == version && connected && deviceId == device) {
                        sessions = list
                        uncertainSessions.removeAll { it.first == device }
                        memory?.record(device, list.associate { it.id to it.exited })
                        updateSessionHeader()
                        refreshDrawer?.invoke()
                    }
                }
            } catch (e: Exception) { post { sessionRefreshBusy = false; if (manual && generation == version) notice(e.message ?: "刷新会话失败") } }
        }
    }
    private fun updateSessionHeader() {
        val session = sessions.firstOrNull { it.id == selected } ?: return
        sessionTitle.text = session.cwd.substringAfterLast('/').ifEmpty { session.cwd }
        sessionMeta.text = "$deviceName · ${session.cwd}"
    }
    private fun select(id: String, control: Boolean, chat: Boolean = false) {
        val reopenKeyboard = control && inputBox.visibility == View.VISIBLE
        closeOverlay(); toggleInput(false); generation++; val version = generation
        selected = null; controlled = false; desktopAttached = false; sessionExited = false
        surface.removeAllViews(); terminal = null
        status.text = "打开会话…"
        work {
            if (generation == version) {
                val frame = remote.select(id, control); val hasControl = remote.hasControl()
                val attached = remote.desktopAttached(); val exited = remote.sessionExited()
                post { if (generation == version && active) {
                    selected = id; controlled = hasControl; desktopAttached = attached; sessionExited = exited
                    keysBar.visibility = if (controlled) View.VISIBLE else View.GONE
                    memory?.remember(deviceId, id)
                    updateSessionHeader()
                    if (sessions.none { it.id == id }) { sessionTitle.text = "终端"; sessionMeta.text = "$deviceName · $id" }
                    show(frame)
                    if (reopenKeyboard) terminal?.focusKeyboard()
                    if (chat) openChat()
                } }
            }
        }
    }
    private fun show(frame: RenderFrame) {
        empty.visibility = View.GONE
        if (terminal == null) { terminal = TerminalView(this, frame).apply {
            canType = { selected != null && controlled }
            sendText = { text -> enqueue { remote.sendText(text, false) } }
            sendKey = { key -> enqueue { remote.sendKey(key) } }
            keyboardOpened = { inputBox.visibility = View.VISIBLE }
            readOnlyTapped = { notice(readOnlyReason()) }
            zoom(this@MainActivity.display.fontSize / 15f)
        }; surface.addView(terminal) }
        else terminal!!.update(frame)
        dimensions.text = "${frame.cols} 列 × ${frame.rows} 行 · UTF-8"
    }
    private fun enqueue(action: () -> Unit): Boolean {
        if (!controlled || selected == null) { notice(readOnlyReason()); return false }
        return try { action(); true } catch (e: Exception) { notice(e.message ?: "输入失败"); false }
    }
    private fun readOnlyReason() = when {
        selected == null -> "请先选择会话"
        sessionExited -> "会话已结束，只能查看历史"
        !desktopAttached -> "Desktop 已离开；在桌面重新 --attach 后可输入"
        else -> "当前设备只有只读权限"
    }
    private fun toggleInput(show: Boolean = inputBox.visibility != View.VISIBLE) {
        inputBox.visibility = if (show) View.VISIBLE else View.GONE
        if (show) terminal?.focusKeyboard() else hideKeyboard()
    }
    private fun hideKeyboard() { (getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(root.windowToken, 0); root.requestFocus(); terminal?.clearFocus() }

    private fun createSession() {
        if (!connected) { accountPanel(); return }
        val cwd = field("桌面工作目录")
        val dialog = AlertDialog.Builder(this).setTitle("新建会话").setView(column(16).apply { addView(cwd) })
            .setNegativeButton("取消", null).setPositiveButton("创建", null).create()
        dialog.setOnShowListener { dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
            val path = cwd.text.toString().trim()
            if (path.isEmpty()) { cwd.error = "请输入工作目录"; return@setOnClickListener }
            val version = generation; dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = false
            worker.execute {
                try { if (generation == version) {
                    val created = remote.createSession(path); val list = remote.sessions()
                    post { if (generation == version) { dialog.dismiss(); sessions = list; select(created.id, true) } }
                } } catch (e: Exception) { post { cwd.error = e.message; dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = true } }
            }
        } }; dialog.show()
    }
    private fun closeSession(session: RemoteSession) {
        AlertDialog.Builder(this).setTitle("关闭会话？").setMessage(session.cwd + "\n终端进程将结束，AI 历史保留。")
            .setNegativeButton("取消", null).setPositiveButton("关闭会话") { _, _ ->
                if (sessionBusy) return@setPositiveButton
                sessionBusy = true; val version = ++generation
                val closingDevice = deviceId
                uncertainSessions.add(closingDevice to session.id)
                selected = null; controlled = false; desktopAttached = false; sessionExited = false; toggleInput(false)
                closeOverlay(); surface.removeAllViews(); terminal = null; empty.visibility = View.VISIBLE
                status.text = "正在关闭会话"
                worker.execute {
                    var confirmed = false
                    try { if (generation == version) {
                        remote.select(session.id, true); remote.closeSelected(); confirmed = true
                        post { if (generation == version) {
                            uncertainSessions.remove(closingDevice to session.id)
                            sessions = sessions.filterNot { it.id == session.id }
                            memory?.record(closingDevice, sessions.associate { it.id to it.exited })
                            status.text = "会话已关闭"; openDrawer()
                        } }
                        val list = remote.sessions()
                        post { if (generation == version) { sessions = list; uncertainSessions.removeAll { it.first == closingDevice }; memory?.record(closingDevice, list.associate { it.id to it.exited }) } }
                    } } catch (e: Exception) {
                        val message = (if (confirmed) "会话已关闭，刷新列表失败" else "关闭结果待确认") + "：" + (e.message ?: "连接错误")
                        post { if (generation == version) { notice(message); openDrawer() } }
                    } finally { post { sessionBusy = false; if (generation == version && overlay?.tag == "drawer") openDrawer() } }
                }
            }.show()
    }
    private fun terminalHistory() {
        if (selected == null) { notice("请先选择会话"); return }
        val version = generation
        historyWorker.execute {
            try {
                val text = remote.readHistory().joinToString("\n")
                post { if (active && generation == version) {
                    val body = panel("终端历史")
                    body.grow(scroll(label(text.ifEmpty { "暂无终端历史" }, display.fontSize.toFloat()).apply { setBackgroundColor(Palette.surface); typeface = Typeface.MONOSPACE; setTextIsSelectable(true); setPadding(dp(16), dp(16), dp(16), dp(16)) }))
                } }
            } catch (e: Exception) { post { if (active && generation == version) notice(e.message ?: "读取失败") } }
        }
    }

    private fun openDrawer() {
        val body = panel("工作空间", drawer = true); overlay?.tag = "drawer"
        val tabs = RadioGroup(this).apply { orientation = RadioGroup.HORIZONTAL; setPadding(dp(12), dp(12), dp(12), 0) }
        fun tab(title: String) = RadioButton(this).apply {
            id = View.generateViewId(); text = title; buttonDrawable = null; gravity = Gravity.CENTER; minHeight = dp(44)
            setTextColor(ColorStateList(arrayOf(intArrayOf(android.R.attr.state_checked), intArrayOf()), intArrayOf(Palette.text, Palette.muted)))
            background = android.graphics.drawable.StateListDrawable().apply {
                addState(intArrayOf(android.R.attr.state_checked), shape(Palette.control, true))
                addState(intArrayOf(), shape(Color.TRANSPARENT, true))
            }
        }
        val terminalsTab = tab("终端"); val historyTab = tab("AI 历史")
        tabs.addView(terminalsTab, RadioGroup.LayoutParams(0, -2, 1f).apply { marginEnd = dp(4) })
        tabs.addView(historyTab, RadioGroup.LayoutParams(0, -2, 1f).apply { marginStart = dp(4) })
        tabs.check(if (drawerTab) historyTab.id else terminalsTab.id); body.addView(tabs)
        val query = field("搜索终端或对话"); body.addView(column(12).apply { addView(query) })
        val list = column(12); body.grow(scroll(list))
        fun render() {
            list.removeAllViews()
            val search = query.text.toString()
            if (drawerTab) {
                val history = chatStore?.list(search).orEmpty()
                if (history.isEmpty()) list.addView(label("暂无匹配的对话", 14f, Palette.muted))
                for (chat in history) {
                    list.addView(actionButton(chat.title + " · " + sessionAvailability(chat.deviceId, chat.sessionId) + "\n" + chat.messages.last().content.take(100)) { openChat(chat) }.apply { gravity = Gravity.START; maxLines = 4 })
                    list.gap(8)
                }
            } else {
                list.addView(row().apply {
                    fill(label(if (connected) deviceName else "尚未连接设备", 12f, Palette.muted))
                    addView(iconButton(R.drawable.ic_refresh_cw, "刷新会话") { refreshSessions(manual = true) })
                    addView(iconButton(R.drawable.ic_plus, "新建会话") { createSession() })
                })
                val matches = sessions.filter { it.cwd.contains(search, true) || it.id.contains(search, true) }
                if (matches.isEmpty()) list.addView(label(if (connected) "暂无匹配会话" else "连接 Desktop 后查看终端", 14f, Palette.muted))
                for (session in matches) {
                    list.addView(row().apply {
                        fill(actionButton(session.cwd + "\n" + sessionAvailability(deviceId, session.id) + " · ${session.id.take(8)}" + if (selected == session.id) " · 当前会话" else "") { select(session.id, true) }.apply { tag = session.id; gravity = Gravity.START; maxLines = 4; isEnabled = (deviceId to session.id) !in uncertainSessions })
                        addView(iconButton(R.drawable.ic_x, "关闭 ${session.cwd} (${session.id})") { closeSession(session) }.apply { tag = "close-${session.id}"; isEnabled = !session.exited && session.desktopAttached && !sessionBusy && (deviceId to session.id) !in uncertainSessions })
                    }); list.gap(8)
                }
            }
        }
        tabs.setOnCheckedChangeListener { _, id -> drawerTab = id == historyTab.id; render() }
        query.addTextChangedListener(watcher { render() }); render()
        body.addView(divider())
        body.addView(row().apply {
            setPadding(dp(12), dp(8), dp(12), dp(8)); fill(label(accountName.ifEmpty { "旧版配对" }))
            addView(iconButton(R.drawable.ic_user_round, "账号与设备") { accountPanel() })
        })
        refreshDrawer = { if (overlay?.tag == "drawer") render() }
        if (connected) {
            if (!drawerTab) refreshSessions()
            ui.removeCallbacks(drawerRefresh)
            ui.postDelayed(drawerRefresh, 3000)
        }
    }

    private fun accountPanel() {
        val body = panel("账号与设备"); overlay?.tag = "account"
        accountBar = column(16)
        accountBar.addView(heading(accountName.ifEmpty { "旧版配对" })); accountBar.addView(label(serverUrl, 12f, Palette.muted))
        accountBar.addView(row().apply {
            fill(actionButton(if (deviceBusy) "刷新中…" else "刷新设备") {
                if (!deviceBusy) { deviceBusy = true; accountPanel(); work { try { loadDevices() } finally { post { deviceBusy = false; if (this@MainActivity.overlay?.tag == "account") accountPanel() } } } }
            }.apply { isEnabled = !deviceBusy && accountName.isNotEmpty() })
            addView(actionButton("改密码") { passwordDialog() }.apply { isEnabled = accountName.isNotEmpty() })
        }); accountBar.gap(16)
        val onlineDevices = devices.filter { it.online }
        if (onlineDevices.isEmpty()) accountBar.addView(label("暂无在线设备", 14f, Palette.muted))
        onlineDevices.forEach { device ->
            accountBar.addView(label(device.name, 16f))
            accountBar.addView(label("${device.platform} · " + if (device.current) "本机" else "在线", 12f, Palette.green))
            accountBar.addView(row().apply {
                fill(actionButton("连接") { connectDevice(device.id) }.apply { contentDescription = "连接 ${device.name}"; isEnabled = device.platform == "desktop" })
                addView(actionButton("移除") {
                    AlertDialog.Builder(this@MainActivity).setTitle("移除 ${device.name}？").setMessage("该设备的登录和连接将失效，桌面 Shell 会保留。")
                        .setNegativeButton("取消", null).setPositiveButton("移除") { _, _ ->
                            disconnect(); val epoch = accountEpoch
                            work { try { account.revoke(device.id); if (account.username().isEmpty()) post { if (epoch == accountEpoch) signedOut() } else loadDevices(epoch) } finally { persist() } }
                        }.show()
                })
            }); accountBar.gap(16)
        }
        accountBar.addView(actionButton("退出登录") { logout() }.apply { setCompoundDrawablesWithIntrinsicBounds(R.drawable.ic_log_out, 0, 0, 0) })
        if (accountName.isEmpty()) accountBar.addView(actionButton("高级：旧版配对") { legacyPairing() })
        body.grow(scroll(accountBar))
    }
    private fun logout() {
        AlertDialog.Builder(this).setTitle("退出登录？").setNegativeButton("取消", null).setPositiveButton("退出") { _, _ ->
            accountEpoch++; disconnect(); signedOut()
            work { try { account.logout() } finally { store.clear() } }
        }.show()
    }
    private fun signedOut() {
        closeOverlay(); chatStore = null; memory = null; uncertainSessions.clear(); restorePending = false; accountName = ""; devices = emptyList(); deviceId = ""; deviceName = ""
        workspace.visibility = View.GONE; (loginBox.parent as View).visibility = View.VISIBLE
        surface.removeAllViews(); terminal = null; toggleInput(false); status.text = "已退出登录"
    }
    private fun passwordDialog() {
        val box = column(16); val old = field("当前密码", true); val next = field("新密码（至少 12 字节）", true)
        box.addView(old); box.gap(); box.addView(next)
        val dialog = AlertDialog.Builder(this).setTitle("修改密码").setView(box).setNegativeButton("取消", null).setPositiveButton("保存", null).create()
        dialog.setOnShowListener { dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
            val current = old.text.toString(); val password = next.text.toString()
            if (password.toByteArray().size < 12 || current.isEmpty()) { next.error = "请输入当前密码，新密码至少 12 字节"; return@setOnClickListener }
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = false
            worker.execute {
                try { account.changePassword(current, password); persist(); post { dialog.dismiss(); accountEpoch++; disconnect(); signedOut(); notice("密码已更新，请重新登录") } }
                catch (e: Exception) { post { next.error = e.message; dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = true } }
            }
        } }; dialog.show()
    }
    private fun legacyPairing() {
        val invitation = field("配对邀请", true)
        AlertDialog.Builder(this).setTitle("旧版配对").setView(invitation).setNegativeButton("取消", null).setPositiveButton("连接") { _, _ ->
            val value = invitation.text.toString(); val version = generation
            work { check(account.username().isEmpty()); remote.connect(value); val list = remote.sessions(); post { if (active && generation == version) {
                (loginBox.parent as View).visibility = View.GONE; workspace.visibility = View.VISIBLE; connected = true; sessions = list; deviceName = "旧版 Desktop"
                openDrawer()
            } } }
        }.show()
    }

    private fun settingsPanel() {
        val body = panel("终端设置")
        val settings = column(20)
        fun slider(title: String, min: Int, max: Int, current: Int, suffix: String, change: (Int) -> Unit) {
            val value = label("$title  $current$suffix", 16f).apply { setBackgroundColor(Palette.surface); setPadding(dp(12), dp(8), dp(12), dp(8)) }
            settings.addView(value)
            settings.addView(SeekBar(this).apply {
                this.max = max - min; progress = current - min; contentDescription = title
                progressTintList = ColorStateList.valueOf(Palette.accent); thumbTintList = ColorStateList.valueOf(Palette.accent)
                setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                    override fun onProgressChanged(bar: SeekBar?, progress: Int, fromUser: Boolean) { val number = progress + min; value.text = "$title  $number$suffix"; change(number) }
                    override fun onStartTrackingTouch(bar: SeekBar?) {}
                    override fun onStopTrackingTouch(bar: SeekBar?) {}
                })
            }, LinearLayout.LayoutParams(-1, dp(48)))
            settings.addView(row().apply { fill(label("$min$suffix", 12f, Palette.muted).apply { setBackgroundColor(Palette.surface) }); addView(label("$max$suffix", 12f, Palette.muted).apply { setBackgroundColor(Palette.surface) }) }); settings.gap(24)
        }
        slider("文字大小", 12, 24, display.fontSize, " sp") { display.fontSize = it; terminal?.zoom(it / 15f) }
        slider("浮层不透明度", 60, 96, display.opacity, "%") { display.opacity = it; (overlayPanel?.background as? android.graphics.drawable.GradientDrawable)?.setColor(panelColor()) }
        settings.addView(actionButton("恢复默认") { display.reset(); terminal?.zoom(16 / 15f); settingsPanel() }.apply { setCompoundDrawablesWithIntrinsicBounds(R.drawable.ic_rotate_ccw, 0, 0, 0) })
        body.grow(scroll(settings))
    }
    private fun panelColor() = Color.argb(display.opacity * 255 / 100, 26, 29, 32)
    private fun panel(title: String, drawer: Boolean = false): LinearLayout {
        closeOverlay(); hideKeyboard()
        workspace.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        val floating = !drawer && title in setOf("终端设置", "AI Agent", "对话记录")
        val layer = FrameLayout(this).apply { setBackgroundColor(if (drawer) 0x66000000 else Color.TRANSPARENT); isClickable = true }
        val body = column().apply { background = shape(if (drawer) Palette.surface else panelColor(), floating); isClickable = true; clipToOutline = floating }
        body.addView(row().apply {
            setPadding(dp(16), dp(10), dp(12), dp(10)); setBackgroundColor(Palette.surface)
            fill(heading(title)); addView(iconButton(R.drawable.ic_x, "关闭$title") { closeOverlay() })
        }); body.addView(divider())
        fun layoutPanel(width: Int, height: Int) = FrameLayout.LayoutParams(
            if (drawer) (width * .88f).toInt().coerceAtMost(dp(420)) else if (floating) (width - dp(24)).coerceAtMost(dp(520)) else -1,
            if (floating) (height * .72f).toInt() else -1,
            if (floating) Gravity.BOTTOM or Gravity.END else Gravity.START
        ).apply { if (floating) { marginStart = dp(12); marginEnd = dp(12); bottomMargin = dp(12) } }
        layer.addView(body, layoutPanel(root.width, root.height))
        layer.addOnLayoutChangeListener { _, left, top, right, bottom, oldLeft, oldTop, oldRight, oldBottom ->
            if (right - left != oldRight - oldLeft || bottom - top != oldBottom - oldTop) {
                body.layoutParams = layoutPanel(right - left, bottom - top)
            }
        }
        layer.setOnClickListener { closeOverlay() }
        root.addView(layer, FrameLayout.LayoutParams(-1, -1)); overlay = layer; overlayPanel = body
        body.announceForAccessibility(title)
        return body
    }
    private fun closeOverlay() {
        assistant?.close(); assistant = null
        refreshDrawer = null; ui.removeCallbacks(drawerRefresh)
        overlay?.let { root.removeView(it) }; overlay = null; overlayPanel = null
        workspace.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_AUTO
        hideKeyboard()
    }
    private fun openChat(history: Conversation? = null) {
        val body = panel(if (history == null) "AI Agent" else "对话记录")
        val content = column(16)
        content.addView(label("AI 助手即将开放", 16f, Palette.accent))
        content.addView(label("当前版本专注终端操作，AI 对话与语音输入稍后提供。", 14f, Palette.muted))
        if (history != null) {
            content.addView(label(history.title + " · " + sessionAvailability(history.deviceId, history.sessionId), 12f, Palette.muted))
            history.messages.forEach { message ->
                content.addView(label((if (message.role == "user") "你" else "AI Agent") + "\n" + message.content).apply { setTextIsSelectable(true) })
                content.gap(12)
            }
        }
        body.grow(scroll(content))
        body.addView(column(12).apply {
            setBackgroundColor(Palette.surface)
            addView(field("AI 对话暂未开放").apply { isEnabled = false })
            addView(actionButton("即将开放", true) {}.apply { isEnabled = false })
        })
    }

    private fun restoreLastSession() {
        if (!active || !restorePending || connecting || connected) return
        restorePending = false
        val target = memory?.last() ?: return
        if (memory?.closed(target.first, target.second) == true) { status.text = "上次会话已关闭"; return }
        val device = devices.firstOrNull { it.id == target.first && it.platform == "desktop" }
        if (device?.online == true) connectDevice(target.first, target.second)
        else status.text = "上次 Desktop 当前离线"
    }
    private fun sessionAvailability(device: String, session: String): String {
        if (memory?.closed(device, session) == true) return "已关闭"
        if ((device to session) in uncertainSessions) return "待确认"
        if (connected && deviceId == device) return sessions.firstOrNull { it.id == session }?.let {
            if (it.exited) "已结束 · 只读" else if (it.desktopAttached) "在线" else "桌面已离开 · 只读"
        } ?: "已关闭"
        if (devices.any { it.id == device && !it.online }) return "离线"
        return "待确认"
    }
    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) { edgeStartX = event.x; edgeStartY = event.y }
        if (event.actionMasked == MotionEvent.ACTION_MOVE && overlay == null && workspace.visibility == View.VISIBLE && edgeStartX < dp(40) && event.x - edgeStartX > dp(64) && kotlin.math.abs(event.y - edgeStartY) < dp(40)) {
            val cancel = MotionEvent.obtain(event).apply { action = MotionEvent.ACTION_CANCEL }
            super.dispatchTouchEvent(cancel); cancel.recycle(); edgeStartX = Float.MAX_VALUE; openDrawer(); return true
        }
        return super.dispatchTouchEvent(event)
    }

    override fun doFrame(frameTimeNanos: Long) {
        if (!active) return
        val now = android.os.SystemClock.elapsedRealtime()
        if (accountName.isNotEmpty() && now - lastHeartbeatAt >= 10_000) {
            lastHeartbeatAt = now
            worker.execute {
                try { account.heartbeat() } catch (_: Exception) { /* The next tick retries. */ }
                finally { persist() }
            }
        }
        if (selected != null) try {
            remote.pollDisplay()?.let { batch ->
                batch.update?.let { terminal?.apply(it); dimensions.text = "${it.cols} 列 × ${it.rows} 行 · UTF-8" }
                val lostControl = controlled && !batch.controlled
                controlled = batch.controlled; desktopAttached = batch.desktopAttached; sessionExited = batch.exited
                keysBar.visibility = if (controlled) View.VISIBLE else View.GONE
                if (lostControl) toggleInput(false)
                val next = (if (batch.path == "direct") "直连" else "中转") + " · " + when {
                    sessionExited -> "会话已结束 · 只读历史"
                    !desktopAttached -> "Desktop 已离开 · 只读历史"
                    controlled -> "可输入"
                    else -> "只读权限"
                }
                if (status.text.toString() != next) status.text = next
            }
        } catch (e: Exception) { disconnect(); notice(e.message ?: "显示同步失败，请重新连接") }
        Choreographer.getInstance().postFrameCallback(this)
    }
    private fun disconnect() {
        generation++; selected = null; controlled = false; desktopAttached = false; sessionExited = false; connected = false; connecting = false; sessions = emptyList()
        sessionRefreshBusy = false
        aiStatus.visibility = View.GONE
        connection.text = "未连接"; connection.setTextColor(Palette.muted)
        toggleInput(false); surface.removeAllViews(); terminal = null; empty.visibility = View.VISIBLE
        sessionTitle.text = "AI Terminal"; sessionMeta.text = deviceName.ifEmpty { "选择 Desktop" }
        work { remote.disconnect() }
    }
    override fun onStart() {
        super.onStart(); active = true; Choreographer.getInstance().postFrameCallback(this)
        if (accountName.isNotEmpty() && !connected) { restorePending = true; work { loadDevices() } }
    }
    override fun onStop() {
        active = false; toast?.cancel(); closeOverlay(); Choreographer.getInstance().removeFrameCallback(this)
        disconnect(); super.onStop()
    }
    override fun onDestroy() {
        assistant?.close(); worker.execute { remote.close(); account.close() }; worker.shutdown(); historyWorker.shutdown()
        super.onDestroy()
    }
    override fun onBackPressed() { if (overlay != null) closeOverlay() else if (inputBox.visibility == View.VISIBLE) toggleInput(false) else super.onBackPressed() }
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        assistant?.permissionResult(requestCode, grantResults)
    }
}

fun watcher(action: () -> Unit) = object : TextWatcher {
    override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
    override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) { action() }
    override fun afterTextChanged(s: Editable?) {}
}
