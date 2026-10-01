package com.yxf.aterminal

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.res.Configuration
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
import android.view.KeyEvent
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
    private var terminalScrollback: TerminalScrollback? = null
    private var historyStatus: String? = null
    private var historyClose: (() -> Unit)? = null
    private val ui = Handler(Looper.getMainLooper())
    private var active = false
    @Volatile private var generation = 0
    @Volatile private var accountEpoch = 0
    private val accountPersistenceLock = Any()
    private var selected: String? = null
    private var currentTerminalPath: String? = null
    private var controlled = false
    private var desktopAttached = false
    private var sessionExited = false
    private var connected = false
    private var accountName = ""
    private var lastHeartbeatAt = 0L
    private var lastSessionRefreshAt = 0L
    private val terminalKeyUps = mutableSetOf<Int>()
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
    private lateinit var workspaceHeader: LinearLayout
    private lateinit var workspaceFooter: LinearLayout
    private lateinit var headerLine: View
    private lateinit var footerLine: View
    private lateinit var sideWorkspace: View
    private lateinit var sideAccount: View
    private lateinit var toolRail: LinearLayout
    private var keyboardOpen = false
    private lateinit var store: PairingStore
    private lateinit var display: DisplayPreferences
    private var terminal: TerminalView? = null
    private var overlay: FrameLayout? = null
    private var overlayPanel: View? = null
    private var settingsEditor: AgentSettingsPanel? = null
    private var accountFromSettings = false
    private var assistant: AssistantPanel? = null
    private var agentPanel: AgentPanel? = null
    private var globalPanel: GlobalConversationPanel? = null
    private var imagePicker: ((List<android.net.Uri>) -> Unit)? = null
    private var skillPicker: ((android.net.Uri) -> Unit)? = null
    private var chatStore: ChatStore? = null
    private var agentArchives:List<Conversation> = emptyList()
    private var archiveLoading=false
    private var loginBusy = false
    private var heartbeatBusy = false
    private var entryPending = false
    private lateinit var entryScreen: LinearLayout
    private lateinit var entryStatus: TextView
    private lateinit var resetLogin: () -> Unit
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
                refreshSessions(); loadAgentArchives()
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
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE or WindowManager.LayoutParams.SOFT_INPUT_STATE_HIDDEN)
        store = PairingStore(this, if (terminalTest) "terminal-input-account" else if (localDebugAutoLogin) "account" else if (testMode || isolatedUi) "acceptance-account" else "account")
        display = DisplayPreferences(this, if (terminalTest) "terminal-input-display" else if (localDebugAutoLogin) "display" else if (testMode || isolatedUi) "acceptance-display" else "display")
        serverUrl = if (isolatedUi) BuildConfig.DEFAULT_SERVER_URL else connectionPreferences.getString("server", null) ?: BuildConfig.DEFAULT_SERVER_URL
        root = FrameLayout(this).apply {
            isFocusableInTouchMode = true
            setBackgroundColor(Palette.background)
            setOnApplyWindowInsetsListener { view, insets ->
                if (Build.VERSION.SDK_INT >= 30) keyboardOpen = insets.isVisible(android.view.WindowInsets.Type.ime())
                view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                    insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
                insets.consumeSystemWindowInsets()
            }
        }
        buildWorkspace()
        buildLogin()
        entryScreen = column(24).apply {
            gravity = Gravity.CENTER
            setBackgroundColor(Palette.background)
            addView(ProgressBar(this@MainActivity))
            entryStatus = label("正在恢复工作空间…", 16f).apply { gravity = Gravity.CENTER }
            addView(entryStatus)
            visibility = View.GONE
        }
        root.addView(entryScreen, FrameLayout.LayoutParams(-1, -1))
        setContentView(root)
        applyWorkspaceLayout()
        if (BuildConfig.TERMINAL_DEBUG && intent.getBooleanExtra("render_fixture", false)) {
            (loginBox.parent as View).visibility = View.GONE; workspace.visibility = View.VISIBLE
            val replica = TerminalReplica()
            replica.applySnapshot(assets.open("screen.pb").use { it.readBytes() })
            replica.frame()?.let { show(it) }; replica.close()
        } else if (localDebugAutoLogin) {
            autoLoginLocalDebug()
        } else if (!isolatedUi) {
            loginBusy = true
            beginEntry("正在恢复工作空间…")
            val epoch = accountEpoch
            work {
                try { store.load()?.let { value ->
                    account.restore(value)
                    val name = account.username()
                    val exported = JSONObject(value)
                    val restoredServer = exported.optString("server", serverUrl)
                    post { if (epoch == accountEpoch) signedIn(name, restoredServer) }
                    loadDevices(epoch)
                } ?: post { if (epoch == accountEpoch) finishEntry() } } finally { post { if (epoch == accountEpoch) loginBusy = false } }
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
                    persist(epoch)
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
            addView(label("   aTerminal", 14f))
        }
        loginBox.addView(brand); loginBox.gap(24)
        loginBox.addView(label("YOUR WORKSPACE, CONNECTED.", 12f, Palette.muted))
        loginBox.addView(label("aTerminal", 32f).apply { setTypeface(typeface, Typeface.BOLD) })
        loginBox.addView(label("登录，回到你的工作现场。", 16f, Palette.muted)); loginBox.gap(20)
        val server = field("服务器 https://…").apply { setText(serverUrl); inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_URI }
        val username = field("账号").apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) setAutofillHints(View.AUTOFILL_HINT_USERNAME) }
        val password = field("密码", true).apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) setAutofillHints(View.AUTOFILL_HINT_PASSWORD) }
        val serverAddress = label(serverUrl.ifBlank { "未配置服务器" }, 14f, Palette.muted).apply { tag = "login.server.address" }
        server.visibility = View.GONE
        val editServer = actionButton("修改服务器地址") {}.apply {
            textSize = 12f; setTextColor(Palette.accent); background = null; gravity = Gravity.START or Gravity.CENTER_VERTICAL
            setOnClickListener {
                if (server.visibility == View.GONE) {
                    server.visibility = View.VISIBLE; serverAddress.visibility = View.GONE; text = "保存服务器地址"
                    server.requestFocus()
                } else try {
                    val value = ChatStore.canonicalServer(server.text.toString())
                    connectionPreferences.edit().putString("server", value).apply()
                    serverUrl = value; server.setText(value); serverAddress.text = value
                    server.visibility = View.GONE; serverAddress.visibility = View.VISIBLE; text = "修改服务器地址"; hideKeyboard()
                } catch (e: Exception) { server.error = e.message ?: "请输入有效的服务地址" }
            }
        }
        loginBox.addView(label("登录地址", 12f, Palette.muted)); loginBox.addView(serverAddress)
        loginBox.addView(server); loginBox.addView(editServer); loginBox.gap(16)
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
                            } else BuildConfig.DEFAULT_SERVER_CA_PEM
                            account.login(url, name, secret, android.os.Build.MODEL, "android", testCa)
                            persist(epoch)
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
                            post { if (epoch == accountEpoch) resetLogin() }
                        }
                    }
                } catch (e: Exception) { error.visibility = View.VISIBLE; error.text = e.message }
            }
        }
        resetLogin = { loginBusy = false; submit.isEnabled = true; submit.text = "登录"; progress.visibility = View.GONE }
        loginBox.addView(submit, LinearLayout.LayoutParams(-1, dp(52)))
        loginBox.gap(32)
        loginBox.addView(label("桌面工作，随身连接", 12f, Palette.muted))
        root.addView(scroll(loginBox), FrameLayout.LayoutParams(-1, -1))
    }

    private fun buildWorkspace() {
        workspace = column().apply { visibility = View.GONE }
        val header = column(4)
        workspaceHeader = header
        sessionTitle = heading("aTerminal").apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END }
        sessionMeta = label("选择 Desktop", 12f, Palette.muted).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END }
        connection = label("未连接", 12f, Palette.muted)
        header.addView(row().apply {
            addView(iconButton(R.drawable.ic_panel_left, "打开工作空间") { openDrawer() })
            fill(column(8).apply { addView(sessionTitle); addView(sessionMeta) })
            addView(iconButton(R.drawable.ic_user_round, "账号与设备") { accountPanel() })
        })
        headerLine = divider()
        workspace.addView(header); workspace.addView(headerLine)
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
        region.addView(TerminalScrollView(this).apply {
            isFillViewport = true
            addView(surface)
            canReadHistory = { selected != null && connected && this@MainActivity.overlay == null }
            isReading = { terminalScrollback?.reading == true }
            lineHeight = { terminal?.rowHeight ?: 20f }
            scrollHistory = { terminalScrollback?.scroll(it) }
            stopFollowing = { terminal?.stopFollowingCursor() }
        }, FrameLayout.LayoutParams(-1, -1))
        empty = column(24).apply {
            gravity = Gravity.CENTER
            addView(label("尚未连接终端", 18f)); addView(actionButton("选择设备", true) { accountPanel() })
        }
        region.addView(empty, FrameLayout.LayoutParams(-1, -1))
        toolRail = column(2).apply {
            background = shape(panelColor(), true)
            sideWorkspace = iconButton(R.drawable.ic_panel_left, "打开工作空间") { openDrawer() }
            sideAccount = iconButton(R.drawable.ic_user_round, "账号与设备") { accountPanel() }
            addView(sideWorkspace); addView(sideAccount)
            addView(iconButton(R.drawable.ic_sliders_horizontal, "设置") { settingsPanel() })
            addView(iconButton(R.drawable.ic_message_circle, "AI 对话") { openChat() })
            addView(iconButton(R.drawable.ic_globe, "全局AI助手") { openGlobalList() })
            addView(iconButton(R.drawable.ic_keyboard, "特殊按键") { specialKeys() })
            for (i in 0 until childCount) getChildAt(i).background = null
        }
        region.addView(scroll(toolRail).apply { isFillViewport = false }, FrameLayout.LayoutParams(-2, -2, Gravity.END or Gravity.CENTER_VERTICAL).apply { marginEnd = dp(6) })
        workspace.grow(region)
        footerLine = divider(); workspace.addView(footerLine)
        val footer = column(6)
        workspaceFooter = footer
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
    private fun work(action: () -> Unit) {
        val epoch = accountEpoch
        worker.execute { try { action() } catch (e: Exception) { post { if (epoch == accountEpoch) notice(e.message ?: "操作失败") } } }
    }
    private fun beginEntry(message: String = "正在连接终端…") {
        entryPending = true; entryStatus.text = message
        (loginBox.parent as View).visibility = View.GONE; workspace.visibility = View.GONE; entryScreen.visibility = View.VISIBLE
    }
    private fun finishEntry() {
        entryPending = false; entryScreen.visibility = View.GONE
        val signedIn = accountName.isNotEmpty()
        (loginBox.parent as View).visibility = if (signedIn) View.GONE else View.VISIBLE
        workspace.visibility = if (signedIn) View.VISIBLE else View.GONE
    }
    private fun notice(message: String) {
        if (entryPending) finishEntry()
        status.text = message; toast?.cancel()
        toast = Toast.makeText(this, message, Toast.LENGTH_LONG).also { it.show() }
    }
    private fun persist(epoch: Int) {
        // Export can wait for network work inside Account; never hold the UI logout lock there.
        val value = account.export()
        synchronized(accountPersistenceLock) {
            if (epoch != accountEpoch) return
            if (value.isEmpty()) store.clear() else store.save(value)
        }
    }
    private fun signedIn(name: String, server: String) {
        accountName = name; serverUrl = server; chatStore = null
        memory = WorkspaceMemory(this, server, name, if (terminalTest) "terminal-input" else ""); restorePending = true
        agentArchives = memory?.archives().orEmpty()
        beginEntry()
        status.text = "$name · 选择在线 Desktop"
    }
    private fun loadDevices(epoch: Int) {
        if (epoch != accountEpoch) return
        try {
            val list = account.devices()
            post { if (epoch == accountEpoch) {
                devices = list; deviceBusy = false
                if (!connected) status.text = "${accountName} · ${list.count { it.platform == "desktop" && it.online }} 台 Desktop 在线"
                if (overlay?.tag == "account") accountPanel()
                if (localDebugAutoLogin && !connected && !connecting) {
                    val desktop = list.firstOrNull { it.platform == "desktop" && it.online && it.name == localDebugDesktop }
                    if (desktop == null) { finishEntry(); localDebugStatus("error", "$localDebugDesktop 未在线") } else if (active) connectDevice(desktop.id)
                } else {
                    val desktop = list.filter { it.online && !it.current && it.platform == "desktop" }.singleOrNull()
                    if (active && !connected && !connecting && desktop != null) connectDevice(desktop.id)
                    else {
                        restoreLastSession()
                        if (active && !connecting && entryPending) finishEntry()
                    }
                }
            } }
        } finally { persist(epoch) }
    }
    private fun connectDevice(id: String, resumeSession: String? = null, resumeChat: Boolean = false) {
        beginEntry()
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
                    connecting = false; connected = true; sessions = list; uncertainSessions.removeAll { it.first == id }; memory?.record(id, list.associate { it.id to it.exited }); rememberSessions(id, list); connection.text = "已连接"; connection.setTextColor(Palette.green)
                    val target = list.firstOrNull { it.id == resumeSession && !it.exited } ?: if (resumeSession == null) list.firstOrNull { !it.exited } else null
                    if (target != null) select(target.id, true, resumeChat)
                    else {
                        status.text = if (resumeSession == null) "已连接，暂无运行中的终端" else "原会话已关闭，历史仍可查阅"
                        finishEntry()
                        if (localDebugAutoLogin) localDebugStatus("ready", "已连接 $deviceName，暂无运行中的终端")
                    }
                } }
            } catch (e: Exception) { post { if (generation == version) { connecting = false; connection.text = "未连接"; if (localDebugAutoLogin) localDebugStatus("error", e.message ?: "连接失败"); notice(e.message ?: "连接失败") } } }
            finally { persist(epoch) }
        }
    }
    private fun refreshSessions(manual: Boolean = false) {
        if (!connected) { if (manual) notice("请先连接 Desktop"); return }
        if (sessionRefreshBusy) return
        sessionRefreshBusy = true
        val version = generation; val device = deviceId; val pathSession = selected
        worker.execute {
            try {
                val list = remote.sessions()
                val cwd = pathSession?.let { id -> runCatching { JSONObject(remote.agent(id, JSONObject().put("version", 1).put("action", "context").toString())).optString("cwd").takeUnless { it.isBlank() || it == "null" } }.getOrNull() }
                post {
                    sessionRefreshBusy = false
                    if (generation == version && connected && deviceId == device) {
                        sessions = list
                        if (selected == pathSession) currentTerminalPath = cwd
                        uncertainSessions.removeAll { it.first == device }
                        memory?.record(device, list.associate { it.id to it.exited }); rememberSessions(device, list)
                        updateSessionHeader()
                        refreshDrawer?.invoke()
                        if (selected == null && !sessionBusy && overlay == null) list.firstOrNull { !it.exited }?.let { select(it.id, true) }
                    }
                }
            } catch (e: Exception) { post { sessionRefreshBusy = false; if (manual && generation == version) notice(e.message ?: "刷新会话失败") } }
        }
    }
    private fun rememberSessions(device: String, list: List<RemoteSession>) {
        memory?.archive(device, list.map { Conversation(device, it.id, it.cwd) }); agentArchives = memory?.archives().orEmpty()
    }
    private fun updateSessionHeader() {
        val session = sessions.firstOrNull { it.id == selected } ?: return
        sessionTitle.text = session.cwd.substringAfterLast('/').ifEmpty { session.cwd }
        sessionMeta.text = "$deviceName · ${session.cwd}"
    }
    private fun select(id: String, control: Boolean, chat: Boolean = false) {
        val reopenKeyboard = control && keyboardOpen
        beginEntry("正在打开终端…")
        terminalScrollback?.live(); terminalScrollback = null
        closeOverlay(); toggleInput(false); currentTerminalPath = null; generation++; val version = generation
        selected = null; controlled = false; desktopAttached = false; sessionExited = false
        surface.removeAllViews(); terminal = null
        status.text = "打开会话…"
        work {
            if (generation == version) {
                val frame = remote.select(id, control); val hasControl = remote.hasControl()
                val attached = remote.desktopAttached(); val exited = remote.sessionExited()
                post { if (generation == version && active) {
                    selected = id; controlled = hasControl; desktopAttached = attached; sessionExited = exited
                    memory?.remember(deviceId, id)
                    updateSessionHeader()
                    if (sessions.none { it.id == id }) { sessionTitle.text = "终端"; sessionMeta.text = "$deviceName · $id" }
                    show(frame)
                    finishEntry()
                    if (localDebugAutoLogin) localDebugStatus("ready", "已打开 $deviceName 的终端")
                    if (reopenKeyboard) terminal?.focusKeyboard()
                    if (chat) openChat()
                } }
            }
        }
    }
    private fun show(frame: RenderFrame) {
        terminalScrollback?.live()
        val version = generation
        terminalScrollback = TerminalScrollback(remote, historyWorker, ui,
            { active && generation == version && selected != null && connected },
            { terminal?.showHistory(it) }, { historyStatus = it; if (it != null) status.text = it }, { notice(it) })
        empty.visibility = View.GONE
        if (terminal == null) { terminal = TerminalView(this, frame).apply {
            beforeInput = { terminalScrollback?.live() }
            historyInvalidated = { terminalScrollback?.live() }
            canType = { selected != null && controlled && (this@MainActivity.overlay == null || this@MainActivity.overlay?.tag == "keys") }
            sendText = { text -> enqueue { remote.typeText(text) } }
            sendPaste = { text -> enqueue { remote.sendText(text, false) } }
            sendKey = { key -> enqueue { remote.sendKey(key) } }
            keyboardOpened = { keyboardOpen = true }
            readOnlyTapped = { notice(readOnlyReason()) }
            zoom(this@MainActivity.display.fontSize / 15f)
        }; surface.addView(terminal) }
        else terminal!!.update(frame)
        dimensions.text = "${frame.cols} 列 × ${frame.rows} 行 · UTF-8"
    }
    private fun enqueue(action: () -> Unit): Boolean {
        terminalScrollback?.live()
        if (!controlled || selected == null) { notice(readOnlyReason()); return false }
        return try { action(); true } catch (e: Exception) { notice(e.message ?: "输入失败"); false }
    }
    private fun readOnlyReason() = when {
        selected == null -> "请先选择会话"
        sessionExited -> "会话已结束，只能查看历史"
        !desktopAttached -> "Desktop 已离开；在桌面重新 --attach 后可输入"
        else -> "当前设备只有只读权限"
    }
    private fun toggleInput(show: Boolean = !keyboardOpen) {
        if (show) terminal?.focusKeyboard() else hideKeyboard()
    }
    private fun hideKeyboard() {
        keyboardOpen = false
        (getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(root.windowToken, 0)
        root.requestFocus(); terminal?.clearFocus()
    }
    private fun specialKeys() {
        if (overlay?.tag == "keys") { closeOverlay(hideIme = false); return }
        val body = panel("特殊按键")
        overlay?.tag = "keys"
        val grid = GridLayout(this).apply { columnCount = 4; setPadding(dp(8), dp(8), dp(8), dp(8)) }
        fun key(icon: Int, label: String, action: () -> Unit) {
            grid.addView(iconButton(icon, label) { closeOverlay(hideIme = false); action() }.apply {
                layoutParams = GridLayout.LayoutParams().apply { width = dp(48); height = dp(48) }
                setPadding(dp(15), dp(15), dp(15), dp(15)); background = null
            })
        }
        for ((icon, label, value) in listOf(
            Triple(R.drawable.ic_key_enter, "回车", "enter"), Triple(R.drawable.ic_key_tab, "Tab", "tab"),
            Triple(R.drawable.ic_key_backspace, "退格", "backspace"), Triple(R.drawable.ic_key_escape, "Esc", "escape"),
            Triple(R.drawable.ic_arrow_left, "向左", "left"), Triple(R.drawable.ic_arrow_up, "向上", "up"),
            Triple(R.drawable.ic_arrow_down, "向下", "down"), Triple(R.drawable.ic_arrow_right, "向右", "right"),
            Triple(R.drawable.ic_key_stop, "Ctrl-C", "ctrl_c"))) {
            key(icon, label) { enqueue { remote.sendKey(value) } }
        }
        key(R.drawable.ic_clipboard, "粘贴") { terminal?.pasteClipboard() }
        key(R.drawable.ic_history, "终端历史") { terminalHistory() }
        key(R.drawable.ic_keyboard, if (keyboardOpen) "收起系统键盘" else "显示系统键盘") { toggleInput() }
        body.grow(scroll(grid))
    }
    private fun applyWorkspaceLayout() {
        val landscape = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
        if (Build.VERSION.SDK_INT >= 30) {
            // Hide status bars without FLAG_FULLSCREEN, which disables IME resize.
            window.clearFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)
            window.insetsController?.let { controller ->
                controller.systemBarsBehavior = android.view.WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                if (landscape) controller.hide(android.view.WindowInsets.Type.statusBars())
                else controller.show(android.view.WindowInsets.Type.statusBars())
            }
        } else if (landscape) window.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)
        else window.clearFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)
        listOf(workspaceHeader, workspaceFooter, headerLine, footerLine).forEach { it.visibility = if (landscape) View.GONE else View.VISIBLE }
        listOf(sideWorkspace, sideAccount).forEach { it.visibility = if (landscape) View.VISIBLE else View.GONE }
        root.requestApplyInsets()
    }
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        applyWorkspaceLayout()
    }

    private var createDialog: AlertDialog? = null

    private fun createSession() {
        if (createDialog?.isShowing == true) return
        if (!connected) { accountPanel(); return }
        val version = generation; val desktop = deviceId
        val cwd = field("Desktop 工作目录（留空使用默认）")
        val recent = column()
        val loading = label("正在读取最近目录…", 12f, Palette.muted)
        var submitting = false
        val content = column(16).apply {
            addView(label("工作目录", 14f, Palette.secondary)); gap(6); addView(cwd); gap()
            addView(settingsRow("默认目录", "使用 Desktop 的默认工作目录", R.drawable.ic_terminal) {
                if (!submitting) { cwd.setText(""); cwd.error = null }
            }); gap(16)
            addView(label("最近使用", 12f, Palette.muted)); addView(loading); addView(recent)
        }
        val dialog = AlertDialog.Builder(this).setTitle("新建会话").setView(scroll(content))
            .setNegativeButton("取消", null).setPositiveButton("创建", null).create()
        fun current() = active && dialog.isShowing && generation == version && deviceId == desktop && connected
        createDialog = dialog
        dialog.setOnDismissListener { if (createDialog === dialog) createDialog = null }
        dialog.setOnShowListener {
            dialog.window?.setBackgroundDrawable(shape(Palette.surface, true))
            dialog.window?.setLayout(-1, (resources.displayMetrics.heightPixels * 0.8).toInt())
            dialog.window?.setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
                if (!current() || submitting) return@setOnClickListener
                // Keep the exact path: spaces and shell metacharacters are valid directory names.
                val path = cwd.text.toString()
                submitting = true; cwd.isEnabled = false; dialog.setCancelable(false)
                dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = false
                dialog.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled = false
                worker.execute {
                    try {
                        if (generation != version || deviceId != desktop) { post { dialog.dismiss() }; return@execute }
                        val created = remote.createSession(path)
                        // A failed refresh must never turn successful creation into a retryable failure.
                        val list = runCatching { remote.sessions() }.getOrNull()
                        post { if (current()) {
                            dialog.dismiss(); sessions = list ?: (listOf(created) + sessions); select(created.id, true)
                        } else { dialog.dismiss() } }
                    } catch (e: Exception) { post {
                        if (current()) {
                            submitting = false; cwd.isEnabled = true; cwd.error = e.message
                            dialog.setCancelable(true)
                            dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = true
                            dialog.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled = true
                        } else { dialog.dismiss() }
                    } }
                }
            }
            worker.execute {
                try {
                    if (generation != version || deviceId != desktop) return@execute
                    val paths = remote.recentDirectories()
                    post { if (current()) {
                        loading.text = if (paths.isEmpty()) "暂无最近目录，可输入目录或使用默认目录" else "选择后可编辑，再点击创建"
                        paths.forEach { path ->
                            recent.addView(settingsRow(path.substringAfterLast('/').ifEmpty { path }, path, R.drawable.ic_terminal) {
                                if (!submitting) { cwd.setText(path); cwd.setSelection(path.length); cwd.error = null }
                            }); recent.settingsDivider()
                        }
                    } }
                } catch (e: Exception) { post { if (current()) { loading.text = "最近目录读取失败，可输入目录或使用默认目录" } } }
            }
        }
        dialog.show()
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
        terminalScrollback?.live()
        if (selected == null) { notice("请先选择会话"); return }
        val version = generation
        val body = panel("终端历史")
        val summary = label("正在读取历史", 12f, Palette.muted)
        val more = actionButton("加载更早记录") {}
        val refresh = actionButton("读取最新历史") { terminalHistory() }
        val text = label("", display.fontSize.toFloat()).apply {
            setBackgroundColor(Palette.surface); typeface = Typeface.MONOSPACE
            setTextIsSelectable(true); setPadding(dp(16), dp(16), dp(16), dp(16))
        }
        body.addView(summary); body.addView(more); body.addView(refresh)
        val viewport = scroll(text)
        body.grow(viewport)
        var cursor: TerminalHistoryCursor? = null
        val lines = mutableListOf<String>()
        var closed = false
        var loading = false
        fun release(value: TerminalHistoryCursor?) {
            if (value != null && !historyWorker.isShutdown) historyWorker.execute { runCatching { remote.releaseHistory(value) } }
        }
        historyClose = { closed = true; release(cursor); cursor = null }
        fun load() {
            if (closed || loading) return
            loading = true; more.isEnabled = false
            val previous = cursor
            historyWorker.execute {
                try {
                    val page = remote.readHistoryPage(previous)
                    post {
                        if (!active || generation != version || closed) { release(page.cursor); return@post }
                        val oldHeight = text.height; val oldY = viewport.scrollY
                        cursor = page.cursor; lines.addAll(0, page.lines)
                        text.text = lines.joinToString("\n").ifEmpty { "暂无终端历史" }
                        summary.text = "已加载 ${page.cursor.offset} / ${page.total} 行" + if (page.truncated) " · 更早记录已超出保留范围" else ""
                        more.visibility = if (page.hasMore) View.VISIBLE else View.GONE
                        more.isEnabled = page.hasMore; loading = false
                        if (previous != null) text.post { if (!closed) viewport.scrollTo(0, oldY + text.height - oldHeight) }
                        else viewport.post { if (!closed) viewport.fullScroll(View.FOCUS_DOWN) }
                    }
                } catch (e: Exception) {
                    post { if (active && generation == version && !closed) {
                        loading = false; more.isEnabled = false
                        summary.text = "历史读取失败，请读取最新历史：${e.message}"
                    } }
                }
            }
        }
        more.setOnClickListener { load() }
        load()
    }

    private fun openDrawer() {
        val body = panel("工作空间", drawer = true); overlay?.tag = "drawer"
        fun symbol(icon: Int, color: Int = Palette.muted, size: Int = 20) = ImageView(this).apply {
            setImageResource(icon); imageTintList = ColorStateList.valueOf(color)
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            layoutParams = LinearLayout.LayoutParams(dp(size), dp(size))
        }
        val count = label("", 12f, Palette.muted)
        val add = iconButton(R.drawable.ic_plus, "新建会话") { createSession() }.apply { background = null; isEnabled = connected }
        val header = body.getChildAt(0) as LinearLayout
        header.removeAllViews()
        header.apply {
            setPadding(dp(20), dp(16), dp(12), dp(12))
            fill(column().apply { addView(heading("工作空间").apply { textSize = 20f }); addView(count) })
            addView(add); addView(iconButton(R.drawable.ic_x, "关闭工作空间") { closeOverlay() }.apply { background = null })
        }
        body.removeViewAt(1)
        val query = field("搜索终端或对话").apply {
            background = null; minHeight = dp(46); textSize = 14f
            setPadding(dp(12), dp(8), dp(8), dp(8))
        }
        body.addView(column().apply {
            setPadding(dp(16), dp(8), dp(16), dp(8))
            addView(row().apply {
                background = shape(Palette.background, true); setPadding(dp(12), 0, dp(4), 0)
                addView(symbol(R.drawable.ic_search)); fill(query)
            })
        })
        body.addView(row().apply {
            setPadding(dp(16), 0, dp(16), 0); fill(label("所有会话", 12f, Palette.muted))
            addView(iconButton(R.drawable.ic_refresh_cw, "刷新会话") { refreshSessions(manual = true); loadAgentArchives() }.apply { background = null })
        })
        val list = column().apply { setPadding(dp(8), 0, dp(8), 0) }
        val viewport = scroll(list); body.grow(viewport)
        fun render() {
            val oldY = viewport.scrollY
            list.removeAllViews()
            add.isEnabled = connected
            val search = query.text.toString()
            val merged = linkedMapOf<String, Conversation>()
            agentArchives.filter { it.sessionId.isNotEmpty() }.forEach { merged[it.deviceId + ":" + it.sessionId] = it }
            sessions.forEach { merged[deviceId + ":" + it.id] = Conversation(deviceId, it.id, it.cwd) }
            val matches = merged.values.filter { it.title.contains(search, true) || it.sessionId.contains(search, true) || agentHistoryMatches(it.deviceId, it.sessionId, search) }
                .sortedBy { chat -> when {
                    deviceId == chat.deviceId && selected == chat.sessionId -> 0
                    connected && deviceId == chat.deviceId && sessions.any { it.id == chat.sessionId && !it.exited && it.desktopAttached } -> 1
                    else -> 2
                } }
            count.text = "${matches.size} 个会话"
            if (matches.isEmpty()) list.addView(label(if (connected) "暂无匹配会话" else "连接 Desktop 后查看终端", 14f, Palette.muted).apply { setPadding(dp(8), dp(12), dp(8), dp(12)) })
            for (chat in matches) {
                val session = sessions.firstOrNull { deviceId == chat.deviceId && it.id == chat.sessionId }
                val available = connected && session != null && !session.exited && session.desktopAttached
                val state = sessionAvailability(chat.deviceId, chat.sessionId).removeSuffix(" · 只读")
                val current = deviceId == chat.deviceId && selected == chat.sessionId
                val device = devices.firstOrNull { it.id == chat.deviceId }?.name
                    ?: deviceName.takeIf { chat.deviceId == deviceId && it.isNotEmpty() } ?: "Desktop"
                val path = session?.cwd ?: chat.title
                val title = path.trimEnd('/').substringAfterLast('/').ifEmpty { path }
                list.addView(row().apply {
                    background = shape(if (current) Palette.control else Color.TRANSPARENT, current)
                    fill(row().apply {
                        tag = chat.sessionId; isSelected = current; isClickable = true; isFocusable = true
                        contentDescription = "$title，$path，$state，$device" + if (current) "，当前会话" else ""
                        accessibilityDelegate = object : View.AccessibilityDelegate() {
                            override fun onInitializeAccessibilityNodeInfo(host: View, info: android.view.accessibility.AccessibilityNodeInfo) {
                                super.onInitializeAccessibilityNodeInfo(host, info); info.className = Button::class.java.name
                            }
                        }
                        gravity = Gravity.TOP; setPadding(dp(12), dp(14), 0, dp(14))
                        setOnClickListener { if (available) select(chat.sessionId, true) else openChat(chat) }
                        addView(symbol(if (available) R.drawable.ic_terminal else R.drawable.ic_archive, if (current) Palette.accent else Palette.muted, 24).apply {
                            layoutParams = LinearLayout.LayoutParams(dp(28), dp(28)).apply { topMargin = dp(2); marginEnd = dp(12) }
                            setPadding(dp(5), dp(5), dp(5), dp(5))
                        })
                        fill(column().apply {
                            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
                            addView(row().apply {
                                fill(label(title, 14f).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END })
                                if (current) addView(symbol(R.drawable.ic_check, Palette.accent, 14))
                            })
                            addView(label(path, 11f, Palette.muted).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.MIDDLE })
                            addView(row().apply {
                                addView(View(this@MainActivity).apply { background = shape(if (available) Palette.green else Palette.muted) }, LinearLayout.LayoutParams(dp(6), dp(6)).apply { marginEnd = dp(6) })
                                addView(label(state, 11f, Palette.text))
                                fill(label(" · $device", 10f, Palette.muted).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END })
                            })
                        })
                    })
                    if (available && session != null) addView(iconButton(R.drawable.ic_x, "关闭 ${session.cwd} (${session.id})") { closeSession(session) }.apply {
                        tag = "close-${session.id}"; background = null
                        setPadding(dp(14), dp(14), dp(14), dp(14))
                        isEnabled = !sessionBusy && (deviceId to session.id) !in uncertainSessions
                    }) else addView(View(this@MainActivity), LinearLayout.LayoutParams(dp(44), 1))
                }); list.gap(8)
            }
            viewport.post { if (overlay?.tag == "drawer") viewport.scrollTo(0, oldY) }
        }
        query.addTextChangedListener(watcher { viewport.scrollTo(0, 0); render() }); render()
        body.addView(divider())
        body.addView(row().apply {
            setPadding(dp(16), dp(12), dp(16), dp(12))
            addView(label(accountName.take(1).uppercase().ifEmpty { "D" }, 16f, Palette.accent).apply {
                gravity = Gravity.CENTER; background = shape(Palette.control, true); importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            }, LinearLayout.LayoutParams(dp(32), dp(32)))
            fill(column().apply {
                setPadding(dp(12), 0, 0, 0)
                addView(label(accountName.ifEmpty { "旧版配对" }, 14f).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END })
                addView(label("账号与设备", 12f, Palette.muted))
            })
            addView(iconButton(R.drawable.ic_monitor_smartphone, "账号与设备") { accountPanel() }.apply { background = null })
        })
        refreshDrawer = { if (overlay?.tag == "drawer") render() }
        if (connected) {
            loadAgentArchives(); refreshSessions()
            ui.removeCallbacks(drawerRefresh); ui.postDelayed(drawerRefresh, 3000)
        }
    }

    private fun agentHistoryMatches(device: String, session: String, query: String): Boolean {
        if (query.isBlank()) return true
        return AgentDraftStore(this, listOf(serverUrl, accountName, device)).search(session, query)
    }
    private fun loadAgentArchives() {
        if(!connected||archiveLoading)return
        archiveLoading=true;val version=generation;val device=deviceId
        historyWorker.execute{try {
            val rows=JSONObject(remote.agent("",JSONObject().put("version",1).put("action","list").toString())).getJSONArray("agents")
            val archives=(0 until rows.length()).mapNotNull { val scope=rows.getJSONObject(it).getJSONObject("scope");val id=scope.optString("session").takeUnless {it.isEmpty() || it=="null"};id?.let { Conversation(device,it,"终端 "+it.take(8)) } }
            post {
                archiveLoading = false
                if (version == generation && device == deviceId) {
                    val known = agentArchives.associateBy { it.deviceId to it.sessionId }
                    val resolved = archives.map { known[it.deviceId to it.sessionId] ?: it }
                    memory?.archive(device, resolved)
                    agentArchives = memory?.archives() ?: (agentArchives.filter { it.deviceId != device } + resolved)
                    refreshDrawer?.invoke()
                }
            }
        }catch(_:Exception){post{archiveLoading=false}}}
    }
    private fun accountPanel() = accountPanel(overlay?.tag == "account" && accountFromSettings)
    private fun accountPanel(fromSettings: Boolean) {
        accountFromSettings = fromSettings
        val body = panel("账号与设备"); overlay?.tag = "account"
        (body.getChildAt(0) as LinearLayout).addView(iconButton(R.drawable.ic_arrow_left, "返回上一页") { if (accountFromSettings) settingsPanel() else closeOverlay() }, 0)
        accountBar = column(16)
        accountBar.addView(settingsRow(accountName.ifEmpty { "旧版配对" }, serverUrl, R.drawable.ic_user_round)); accountBar.gap(24)
        accountBar.addView(row().apply {
            fill(label("在线设备", 14f, Palette.muted))
            addView(iconButton(R.drawable.ic_refresh_cw, if (deviceBusy) "刷新中…" else "刷新设备") {
                if (!deviceBusy) { deviceBusy = true; accountPanel(); val epoch = accountEpoch; work { try { loadDevices(epoch) } finally { post { if (epoch == accountEpoch) { deviceBusy = false; if (this@MainActivity.overlay?.tag == "account") accountPanel() } } } } }
            }.apply { isEnabled = !deviceBusy && accountName.isNotEmpty() })

        }); accountBar.gap(16)
        val onlineDevices = devices.filter { it.online && !it.current }
        if (onlineDevices.isEmpty()) accountBar.addView(label("暂无在线设备", 14f, Palette.muted))
        onlineDevices.forEach { device ->
            accountBar.addView(row().apply {
                background = shape(Palette.surface); setPadding(dp(12), dp(12), dp(8), dp(12))
                fill(column().apply { addView(label(device.name, 16f)); addView(label("${device.platform} · 在线", 12f, Palette.green)) })
                addView(iconButton(R.drawable.ic_link, "连接 ${device.name}") { connectDevice(device.id) }.apply { background = null; isEnabled = device.platform == "desktop" })
                addView(iconButton(R.drawable.ic_unlink, "移除 ${device.name}") {
                    AlertDialog.Builder(this@MainActivity).setTitle("移除 ${device.name}？").setMessage("该设备的登录和连接将失效，桌面 Shell 会保留。")
                        .setNegativeButton("取消", null).setPositiveButton("移除") { _, _ ->
                            disconnect(); val epoch = accountEpoch
                            work { try { account.revoke(device.id); if (account.username().isEmpty()) post { if (epoch == accountEpoch) signedOut() } else loadDevices(epoch) } finally { persist(epoch) } }
                        }.show()
                }.apply { background = null })
            }); accountBar.gap(8)
        }
        accountBar.gap(16)
        accountBar.addView(settingsRow("修改密码", "更新当前账号密码", R.drawable.ic_key_round) { if (accountName.isNotEmpty()) passwordDialog() }); accountBar.gap()
        accountBar.addView(actionButton("退出登录") { logout() }.apply { setCompoundDrawablesWithIntrinsicBounds(R.drawable.ic_log_out, 0, 0, 0) })
        if (accountName.isEmpty()) accountBar.addView(actionButton("高级：旧版配对") { legacyPairing() })
        body.grow(scroll(accountBar))
    }
    private fun logout() {
        AlertDialog.Builder(this).setTitle("退出登录？").setNegativeButton("取消", null).setPositiveButton("退出") { _, _ ->
            try {
                synchronized(accountPersistenceLock) { store.clear(); accountEpoch++ }
                disconnect(); signedOut()
                work { account.logout() }
            } catch (e: Exception) { notice(e.message ?: "无法清除登录信息，请重试") }
        }.show()
    }
    private fun signedOut() {
        resetLogin(); deviceBusy = false; entryPending = false; entryScreen.visibility = View.GONE
        closeOverlay(); chatStore = null; agentArchives=emptyList(); memory = null; uncertainSessions.clear(); restorePending = false; accountName = ""; devices = emptyList(); deviceId = ""; deviceName = ""
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
            val epoch = accountEpoch
            worker.execute {
                try { account.changePassword(current, password); persist(epoch); post { if (epoch == accountEpoch) { dialog.dismiss(); accountEpoch++; disconnect(); signedOut(); notice("密码已更新，请重新登录") } } }
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
        val body = panel("设置"); overlay?.tag = "settings"
        val settings = column(16)
        settings.addView(label("显示", 12f, Palette.muted)); settings.gap(8)
        val displayCard = column(12).apply { background = shape(Palette.surface) }
        settings.addView(displayCard)
        fun slider(title: String, min: Int, max: Int, current: Int, suffix: String, change: (Int) -> Unit) {
            val value = label("$current$suffix", 14f, Palette.accent)
            displayCard.addView(row().apply { fill(label(title, 16f)); addView(value) })
            displayCard.addView(SeekBar(this).apply {
                this.max = max - min; progress = current - min; contentDescription = title
                progressTintList = ColorStateList.valueOf(Palette.accent); thumbTintList = ColorStateList.valueOf(Palette.accent)
                setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                    override fun onProgressChanged(bar: SeekBar?, progress: Int, fromUser: Boolean) { val number = progress + min; value.text = "$number$suffix"; change(number) }
                    override fun onStartTrackingTouch(bar: SeekBar?) {}
                    override fun onStopTrackingTouch(bar: SeekBar?) {}
                })
            }, LinearLayout.LayoutParams(-1, dp(48)))
            displayCard.addView(row().apply { fill(label("$min$suffix", 12f, Palette.muted)); addView(label("$max$suffix", 12f, Palette.muted)) }); displayCard.gap(16)
        }
        slider("文字大小", 6, 24, display.fontSize, " sp") { display.fontSize = it; terminal?.zoom(it / 15f) }
        slider("浮层不透明度", 0, 100, display.opacity, "%") { display.opacity = it; (overlayPanel?.background as? android.graphics.drawable.GradientDrawable)?.setColor(panelColor()); toolRail.background = shape(panelColor(), true) }
        settings.gap(24); settings.addView(label("AI 与工具", 12f, Palette.muted)); settings.gap(8)
        val llm = settingsRow("LLM 大模型", if (connected) "正在读取模型配置…" else "连接 Desktop 后配置", R.drawable.ic_cpu) { agentSettings("llm") }
        settings.addView(llm); settings.settingsDivider()
        settings.addView(settingsRow("终端读取", "首尾锚点与读取范围", R.drawable.ic_search) { agentSettings("reading") }); settings.settingsDivider()
        settings.addView(settingsRow("MCP", "外部工具与服务", R.drawable.ic_plug) { agentSettings("mcp") }); settings.settingsDivider()
        settings.addView(settingsRow("Skills", "可复用的 Agent 能力", R.drawable.ic_book_open) { agentSettings("skills") })
        settings.gap(24); settings.addView(label("工作空间", 12f, Palette.muted)); settings.gap(8)
        settings.addView(settingsRow("账号与设备", "管理账号与在线设备", R.drawable.ic_monitor_smartphone) { accountPanel(true) })
        body.grow(scroll(settings)); body.addView(divider())
        body.addView(row().apply {
            setPadding(dp(12), dp(8), dp(16), dp(8)); setBackgroundColor(Palette.surface)
            fill(actionButton("恢复显示默认值") { this@MainActivity.display.reset(); terminal?.zoom(16 / 15f); toolRail.background = shape(panelColor(), true); settingsPanel() }.apply {
                background = null; gravity = Gravity.START; setCompoundDrawablesWithIntrinsicBounds(R.drawable.ic_rotate_ccw, 0, 0, 0)
            }); addView(label("自动保存", 12f, Palette.muted))
        })
        if (connected) {
            val device = deviceId; val owner = accountName; val version = generation; val session = selected
            work {
                val summary = runCatching {
                    check(device == deviceId && owner == accountName && version == generation)
                    val config = JSONObject(remote.configuration(JSONObject().put("action", "show").toString())).getJSONObject("config")
                    val bindings = config.getJSONObject("bindings")
                    val binding = if (session != null) bindings.optJSONObject("session/$session") ?: bindings.optJSONObject("session-default") else bindings.optJSONObject("global")
                    val model = binding?.optString("model_id")?.let { config.getJSONObject("models").optJSONObject(it) }
                    val provider = model?.optString("provider_id")?.let { config.getJSONObject("providers").optJSONObject(it) }
                    (model?.optString("model") ?: "尚未绑定模型") + if (provider?.optBoolean("enabled", true) == false) " · 供应商已停用" else ""
                }.getOrElse { "无法读取配置" }
                post { if (body.isAttachedToWindow && device == deviceId && owner == accountName && version == generation) ((llm.getChildAt(1) as LinearLayout).getChildAt(1) as TextView).text = summary }
            }
        }
    }
    private fun panelColor() = Color.argb(display.opacity * 255 / 100, 16, 22, 35)
    private fun panel(title: String, drawer: Boolean = false): LinearLayout {
        val special = title == "特殊按键"
        closeOverlay(hideIme = !special)
        workspace.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        toolRail.visibility = View.INVISIBLE
        val floating = special
        val layer = FrameLayout(this).apply { setBackgroundColor(if (drawer) 0x66000000 else Color.TRANSPARENT); isClickable = true }
        val body = column().apply { background = shape(if (drawer) Palette.surface else panelColor(), floating).apply { if (!floating) cornerRadius = 0f }; isClickable = true; clipToOutline = floating }
        body.addView(row().apply {
            setPadding(dp(16), dp(10), dp(12), dp(10)); setBackgroundColor(Palette.surface)
            fill(heading(title)); addView(iconButton(R.drawable.ic_x, "关闭$title") { closeOverlay(hideIme = !special) })
        }); body.addView(divider())
        fun layoutPanel(width: Int, height: Int) = FrameLayout.LayoutParams(
            if (special) (width - dp(24)).coerceAtMost(dp(232)) else if (drawer) (width * .92f).toInt().coerceAtMost(dp(420)) else if (floating) (width - dp(24)).coerceAtMost(dp(520)) else -1,
            if (special) (height - dp(24)).coerceAtMost(dp(224)).coerceAtLeast(dp(80)) else if (floating) (height * .72f).toInt() else -1,
            if (special) Gravity.CENTER_VERTICAL or Gravity.END else if (floating) Gravity.BOTTOM or Gravity.END else Gravity.START
        ).apply { if (floating) { marginStart = dp(12); marginEnd = dp(12); bottomMargin = dp(12) } }
        layer.addView(body, layoutPanel(root.width, root.height))
        layer.addOnLayoutChangeListener { _, left, top, right, bottom, oldLeft, oldTop, oldRight, oldBottom ->
            if (right - left != oldRight - oldLeft || bottom - top != oldBottom - oldTop) {
                body.layoutParams = layoutPanel(right - left, bottom - top)
            }
        }
        layer.setOnClickListener { closeOverlay(hideIme = !special) }
        root.addView(layer, FrameLayout.LayoutParams(-1, -1)); overlay = layer; overlayPanel = body
        body.announceForAccessibility(title)
        return body
    }
    private fun closeOverlay(hideIme: Boolean = true) {
        settingsEditor?.close(); settingsEditor = null
        historyClose?.invoke(); historyClose = null
        assistant?.close(); assistant = null
        agentPanel?.close(); agentPanel = null
        globalPanel?.close(); globalPanel = null
        refreshDrawer = null; ui.removeCallbacks(drawerRefresh)
        overlay?.let { root.removeView(it) }; overlay = null; overlayPanel = null
        workspace.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_AUTO
        toolRail.visibility = View.VISIBLE
        if (hideIme) hideKeyboard()
        if (controlled && keyboardOpen) terminal?.requestFocus() else root.requestFocus()
    }
    private fun openGlobalList() {
        val device = deviceId; val owner = accountName; val server = serverUrl
        val body = panel("全局AI助手"); overlay?.tag = "global-list"
        globalPanel = GlobalConversationPanel(this, body, listOf(server, owner, device), deviceName.ifEmpty { "Desktop" },
            { connected && deviceId == device && accountName == owner && serverUrl == server },
            { json ->
                check(connected && deviceId == device && accountName == owner && serverUrl == server) { "连接已变化" }
                remote.agent("", json)
            }, { closeOverlay() }, { row -> openChat(global = row) })
    }
    private fun openChat(history: Conversation? = null, global: JSONObject? = null) {
        if (global == null && (history?.sessionId ?: selected).isNullOrEmpty()) { notice("请先选择终端会话"); return }
        val body = panel(if (global != null) "全局AI助手" else "AI Agent")
        if (global != null) overlay?.tag = "global-chat"
        val device = history?.deviceId ?: deviceId; val account = accountName; val version = generation; val server = serverUrl
        agentPanel = AgentPanel(this, body, listOf(serverUrl, accountName, device), history?.sessionId ?: selected.orEmpty(),
            { connected && deviceId == device && accountName == account && serverUrl == server && (global != null || generation == version) },
            { session, json ->
                check(connected && deviceId == device && accountName == account && serverUrl == server && (global != null || generation == version)) { "连接已变化" }
                remote.agent(session, json)
            }, {}, history != null, workingPath = {
                if (global != null) deviceName.ifEmpty { "Desktop" }
                else if (history != null && history.sessionId != selected) history.title
                else currentTerminalPath ?: if (selected == null) "未选择终端" else "路径不可用"
            }, writeReason = { target ->
                when {
                    (!connected || deviceId != device) -> "设备离线 · 只读缓存"
                    global != null -> null
                    target.isEmpty() -> null
                    sessions.none { it.id == target } -> "会话已关闭 · 只读"
                    sessions.first { it.id == target }.exited -> "会话已结束 · 只读"
                    !sessions.first { it.id == target }.desktopAttached -> "Desktop 已离开 · 只读"
                    else -> null
                }
            }, pickImages = { callback ->
                imagePicker = callback
                val picker = if (Build.VERSION.SDK_INT >= 33) android.content.Intent(android.provider.MediaStore.ACTION_PICK_IMAGES).putExtra(android.provider.MediaStore.EXTRA_PICK_IMAGES_MAX, 4)
                    else android.content.Intent(android.content.Intent.ACTION_OPEN_DOCUMENT).addCategory(android.content.Intent.CATEGORY_OPENABLE).putExtra(android.content.Intent.EXTRA_ALLOW_MULTIPLE, true)
                picker.type = "image/*"
                startActivityForResult(picker, 831)
            }, globalConversation = global, back = { openGlobalList() })
    }
    private fun agentSettings(page: String = "llm") {
        val title = mapOf("llm" to "LLM 大模型", "reading" to "终端读取", "mcp" to "MCP", "skills" to "Skills")[page] ?: "LLM 大模型"
        val body = panel(title); overlay?.tag = "setting-detail"
        val header = body.getChildAt(0) as LinearLayout
        header.addView(iconButton(R.drawable.ic_arrow_left, "返回上一页") { if (settingsEditor?.back() != true) settingsPanel() }, 0)
        body.addView(label("$accountName · $deviceName",12f,Palette.muted))
        val device=deviceId; val account=accountName;val version=generation
        settingsEditor = AgentSettingsPanel(this, body, { json ->
            check(connected && device==deviceId && account==accountName && version==generation) { "连接已变化" }
            val result = remote.configuration(json)
            check(connected && device==deviceId && account==accountName && version==generation) { "连接已变化" }
            result
        }, selected.orEmpty(), { callback ->
            skillPicker=callback
            startActivityForResult(android.content.Intent(android.content.Intent.ACTION_OPEN_DOCUMENT_TREE),830)
        }, page, { heading -> (header.getChildAt(1) as TextView).text = heading })
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
        if (accountName.isNotEmpty() && !heartbeatBusy && !entryPending && !connecting && now - lastHeartbeatAt >= 10_000) {
            lastHeartbeatAt = now; heartbeatBusy = true
            val epoch = accountEpoch
            val discover = !connected && !deviceBusy
            if (discover) deviceBusy = true
            worker.execute {
                try {
                    if (epoch != accountEpoch) return@execute
                    try { account.heartbeat() } catch (_: Exception) { /* Discovery may still recover. */ }
                    if (discover) loadDevices(epoch)
                } catch (_: Exception) { /* The next tick retries without queuing more work. */ }
                finally {
                    try { persist(epoch) } finally { post {
                        heartbeatBusy = false
                        if (epoch == accountEpoch) {
                            if (discover) deviceBusy = false
                            if (connected && selected == null && !entryPending && !sessionBusy) refreshSessions()
                        }
                    } }
                }
            }
        }
        if (connected && now - lastSessionRefreshAt >= 3000) { lastSessionRefreshAt = now; refreshSessions() }
        if (selected != null) try {
            remote.pollDisplay()?.let { batch ->
                batch.update?.let { terminal?.apply(it); dimensions.text = "${it.cols} 列 × ${it.rows} 行 · UTF-8" }
                val lostControl = controlled && !batch.controlled
                controlled = batch.controlled; desktopAttached = batch.desktopAttached; sessionExited = batch.exited
                if (lostControl) toggleInput(false)
                val next = (if (batch.path == "direct") "直连" else "中转") + " · " + when {
                    sessionExited -> "会话已结束 · 只读历史"
                    !desktopAttached -> "Desktop 已离开 · 只读历史"
                    controlled -> "可输入"
                    else -> "只读权限"
                }
                val shownStatus = historyStatus ?: next
                if (status.text.toString() != shownStatus) status.text = shownStatus
            }
        } catch (e: Exception) { disconnect(); notice(e.message ?: "显示同步失败，请重新连接") }
        Choreographer.getInstance().postFrameCallback(this)
    }
    private fun disconnect() {
        createDialog?.dismiss()
        terminalScrollback?.live(); terminalScrollback = null
        generation++; selected = null; currentTerminalPath = null; controlled = false; desktopAttached = false; sessionExited = false; connected = false; connecting = false; sessions = emptyList()
        sessionRefreshBusy = false
        aiStatus.visibility = View.GONE
        connection.text = "未连接"; connection.setTextColor(Palette.muted)
        toggleInput(false); surface.removeAllViews(); terminal = null; empty.visibility = View.VISIBLE
        sessionTitle.text = "aTerminal"; sessionMeta.text = deviceName.ifEmpty { "选择 Desktop" }
        work { remote.disconnect() }
    }
    override fun onStart() {
        super.onStart(); agentPanel?.resume(); globalPanel?.resume(); active = true; Choreographer.getInstance().postFrameCallback(this)
        if (accountName.isNotEmpty() && !connected) { beginEntry(); restorePending = true; val epoch = accountEpoch; work { loadDevices(epoch) } }
    }
    override fun onStop() {
        terminalScrollback?.live(); terminalScrollback = null
        active = false; terminalKeyUps.clear(); toast?.cancel(); agentPanel?.pause(); globalPanel?.pause(); Choreographer.getInstance().removeFrameCallback(this)
        if (imagePicker == null && skillPicker == null) { closeOverlay(); disconnect() }; super.onStop()
    }
    override fun onDestroy() {
        synchronized(accountPersistenceLock) { accountEpoch++ }
        agentPanel?.close(); globalPanel?.close(); assistant?.close(); worker.execute { remote.close(); account.close() }; worker.shutdown(); historyWorker.shutdown()
        super.onDestroy()
    }
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (event.action == KeyEvent.ACTION_UP && terminalKeyUps.remove(event.keyCode)) return true
        val view = terminal
        if ((event.action == KeyEvent.ACTION_DOWN || event.action == KeyEvent.ACTION_MULTIPLE) && selected != null && view != null &&
            (overlay == null || overlay?.tag == "keys") && workspace.visibility == View.VISIBLE && currentFocus !is EditText &&
            view.handleHardwareKey(event)) {
            if (event.action == KeyEvent.ACTION_DOWN) terminalKeyUps.add(event.keyCode)
            return true
        }
        return super.dispatchKeyEvent(event)
    }
    override fun onBackPressed() { if (settingsEditor?.back() == true) return; if (overlay?.tag == "account" && accountFromSettings) { settingsPanel(); return }; if (agentPanel?.closeDetails() == true) return; if (overlay?.tag == "global-chat") openGlobalList() else if (overlay?.tag == "setting-detail") settingsPanel() else if (overlay != null) closeOverlay() else if (keyboardOpen) toggleInput(false) else super.onBackPressed() }
    @Deprecated("Legacy activity result bridge")
    override fun onActivityResult(requestCode:Int,resultCode:Int,data:android.content.Intent?) {
        super.onActivityResult(requestCode,resultCode,data)
        if (requestCode == 831) {
            val callback = imagePicker; imagePicker = null
            if (resultCode == RESULT_OK && data != null) {
                val uris = data.clipData?.let { clip -> (0 until clip.itemCount).map { clip.getItemAt(it).uri } } ?: listOfNotNull(data.data)
                callback?.invoke(uris)
            }
        }
        if(requestCode==830) {val callback=skillPicker;skillPicker=null;if(resultCode==RESULT_OK)data?.data?.let{callback?.invoke(it)}}
    }
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        assistant?.permissionResult(requestCode, grantResults)
        agentPanel?.permissionResult(requestCode, grantResults)
    }
}

fun watcher(action: () -> Unit) = object : TextWatcher {
    override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
    override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) { action() }
    override fun afterTextChanged(s: Editable?) {}
}
