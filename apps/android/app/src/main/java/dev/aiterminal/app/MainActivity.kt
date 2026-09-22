package dev.aiterminal.app

import android.app.Activity
import android.os.Bundle
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.RenderNode
import android.os.Build
import android.content.Context
import android.view.View
import android.view.Choreographer
import android.widget.*
import uniffi.ai_terminal_mobile.*

class MainActivity : Activity(), Choreographer.FrameCallback {
    private val remote = RemoteTerminal()
    private val account = Account()
    private val worker = java.util.concurrent.Executors.newSingleThreadExecutor()
    private val historyWorker = java.util.concurrent.Executors.newSingleThreadExecutor()
    private val ui = android.os.Handler(android.os.Looper.getMainLooper())
    private var active = false
    @Volatile private var generation = 0
    private var selected: String? = null
    private var controlled = false
    private var accountName = ""
    private lateinit var status: TextView
    private lateinit var loginBox: LinearLayout
    private lateinit var accountBar: LinearLayout
    private lateinit var devicesBox: LinearLayout
    private lateinit var sessionsBox: LinearLayout
    private lateinit var surface: HorizontalScrollView
    private lateinit var takeControl: CheckBox
    private lateinit var store: PairingStore
    private var terminal: TerminalView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        store = PairingStore(this, "account")
        val column = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(16, 24, 16, 16) }
        column.addView(TextView(this).apply { text = "AI Terminal"; textSize = 22f })
        status = TextView(this).apply { text = "登录后选择 Desktop" }; column.addView(status)
        loginBox = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val server = field("服务器 https://…"); val username = field("账号"); val password = field("密码", true)
        loginBox.addView(server); loginBox.addView(username); loginBox.addView(password)
        loginBox.addView(button("登录") {
            val url = server.text.toString(); val name = username.text.toString(); val secret = password.text.toString(); password.setText("")
            work { account.login(url, name, secret, android.os.Build.MODEL, "android", ""); persist(); loadDevices() }
        })
        loginBox.addView(button("旧版配对（迁移用）") {
            val invitation = field("配对邀请", true)
            android.app.AlertDialog.Builder(this).setTitle("旧版配对").setView(invitation).setNegativeButton("取消", null).setPositiveButton("连接") { _, _ ->
                val value = invitation.text.toString(); val version = generation
                work { if (generation == version) { check(account.username().isEmpty()); remote.connect(value); val sessions = remote.sessions(); ui.post { if (generation == version && active) showSessions(sessions) } } }
            }.show()
        })
        column.addView(loginBox)
        accountBar = LinearLayout(this).apply { visibility = View.GONE }
        accountBar.addView(button("刷新设备") { work { loadDevices() } })
        accountBar.addView(button("改密码") { passwordDialog() })
        accountBar.addView(button("退出") { disconnect(); work { try { account.logout() } finally { persist(); ui.post { signedOut() } } } })
        column.addView(accountBar)
        devicesBox = LinearLayout(this)
        column.addView(HorizontalScrollView(this).apply { addView(devicesBox) })
        val actions = LinearLayout(this)
        actions.addView(button("新建") { val cwd = field("桌面工作目录"); android.app.AlertDialog.Builder(this).setTitle("新建会话").setView(cwd).setNegativeButton("取消", null).setPositiveButton("创建") { _, _ -> val path = cwd.text.toString(); work { remote.createSession(path); val sessions = remote.sessions(); ui.post { showSessions(sessions) } } }.show() })
        actions.addView(button("历史") {
            val version = generation
            historyWorker.execute {
                try {
                    val text = remote.readHistory().joinToString("\n")
                    ui.post { if (active && generation == version) {
                        val content = TextView(this).apply { this.text = text; typeface = Typeface.MONOSPACE; setTextIsSelectable(true) }
                        android.app.AlertDialog.Builder(this).setTitle("历史").setView(ScrollView(this).apply { addView(content) }).setPositiveButton("关闭", null).show()
                    } }
                } catch (e: Exception) { ui.post { if (active && generation == version) status.text = e.message ?: "读取历史失败" } }
            }
        })
        takeControl = CheckBox(this).apply { text = "接管输入" }
        takeControl.setOnClickListener { selected?.let { select(it, takeControl.isChecked) } }
        actions.addView(takeControl); column.addView(actions)
        sessionsBox = LinearLayout(this); column.addView(HorizontalScrollView(this).apply { addView(sessionsBox) })
        surface = HorizontalScrollView(this)
        column.addView(ScrollView(this).apply { addView(surface) }, LinearLayout.LayoutParams(-1, 0, 1f))
        column.addView(SeekBar(this).apply { max = 120; progress = 40; contentDescription = "终端字号"; setOnSeekBarChangeListener(object: SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(bar: SeekBar?, value: Int, fromUser: Boolean) { terminal?.zoom(0.6f + value / 100f) }
            override fun onStartTrackingTouch(bar: SeekBar?) {} ; override fun onStopTrackingTouch(bar: SeekBar?) {}
        }) })
        val input = field("输入文字"); column.addView(input)
        val keys = LinearLayout(this)
        keys.addView(button("发送") { enqueue { remote.sendText(input.text.toString(), false); input.setText("") } })
        for ((label, key) in listOf("回车" to "enter", "Ctrl-C" to "ctrl_c", "Tab" to "tab", "Esc" to "escape", "↑" to "up", "↓" to "down")) keys.addView(button(label) { enqueue { remote.sendKey(key) } })
        column.addView(HorizontalScrollView(this).apply { addView(keys) })
        setContentView(column)
        if (BuildConfig.TERMINAL_DEBUG && intent.getBooleanExtra("render_fixture", false)) {
            val replica = TerminalReplica(); replica.applySnapshot(assets.open("screen.pb").use { it.readBytes() }); replica.frame()?.let { show(it) }; replica.close()
        } else work { store.load()?.let { account.restore(it); loadDevices() } }
    }
    private fun field(hint: String, secret: Boolean = false) = EditText(this).apply { this.hint = hint; maxLines = 1; inputType = android.text.InputType.TYPE_CLASS_TEXT or if (secret) android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD else android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS }
    private fun button(label: String, action: () -> Unit) = Button(this).apply { text = label; setOnClickListener { action() } }
    private fun failure(e: Exception) { ui.post { status.text = e.message ?: "操作失败" } }
    private fun work(action: () -> Unit) { worker.execute { try { action() } catch (e: Exception) { failure(e) } } }
    private fun persist() { val value = account.export(); if (value.isEmpty()) store.clear() else store.save(value) }
    private fun loadDevices() {
        val restoredName = account.username()
        ui.post { accountName = restoredName; loginBox.visibility = if (restoredName.isEmpty()) View.VISIBLE else View.GONE; accountBar.visibility = if (restoredName.isEmpty()) View.GONE else View.VISIBLE }
        try { val devices = account.devices(); val name = account.username(); ui.post { accountName = name; loginBox.visibility = View.GONE; accountBar.visibility = View.VISIBLE; devicesBox.removeAllViews(); status.text = "$name · 选择在线 Desktop"
            for (device in devices) {
                val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
                box.addView(button(device.name + if (device.current) " · 本机" else if (device.online) " · 在线" else " · 离线") { connectDevice(device.id) }.apply { isEnabled = device.platform == "desktop" && device.online })
                box.addView(button("移除") { android.app.AlertDialog.Builder(this).setTitle("移除 ${device.name}？").setMessage("该设备的登录和远程连接将失效，桌面 Shell 会保留。").setNegativeButton("取消", null).setPositiveButton("移除") { _, _ -> disconnect(); work { try { account.revoke(device.id); if (account.username().isEmpty()) ui.post { signedOut() } else loadDevices() } finally { persist() } } }.show() })
                devicesBox.addView(box)
            }
        } } finally { persist() }
    }
    private fun connectDevice(id: String) {
        disconnect(); val version = generation; status.text = "连接中…"
        work { try { if (generation == version) { account.connect(id, remote); val sessions = remote.sessions(); ui.post { if (generation == version && active) showSessions(sessions) } } } finally { persist() } }
    }
    private fun showSessions(sessions: List<RemoteSession>) { sessionsBox.removeAllViews(); for (s in sessions) sessionsBox.addView(button(s.cwd + if (s.exited) " · 已结束" else "") { select(s.id, controlled) }); status.text = "已连接 · 选择会话" }
    private fun select(id: String, control: Boolean) { generation++; val version = generation; selected = null; controlled = false; work { if (generation == version) { val frame = remote.select(id, control); val hasControl = remote.hasControl(); ui.post { if (generation == version && active) { selected = id; controlled = hasControl; takeControl.isChecked = controlled; show(frame) } } } } }
    private fun show(frame: RenderFrame) { if (terminal == null) { terminal = TerminalView(this, frame); surface.addView(terminal) } else terminal!!.update(frame) }
    private fun enqueue(action: () -> Unit) { if (!controlled) { status.text = "请先接管输入"; return }; try { action() } catch (e: Exception) { failure(e) } }
    override fun doFrame(time: Long) {
        if (!active) return
        if (selected != null) try {
            // A busy replica returns no batch without consuming its dirty state. The next
            // display frame receives every accumulated cell change and coherent control status.
            remote.pollDisplay()?.let { batch ->
                batch.update?.let { terminal?.apply(it) }
                controlled = batch.controlled
                if (takeControl.isChecked != controlled) takeControl.isChecked = controlled
                val next = (if (batch.path == "direct") "直连" else "中转") + if (controlled) " · 可输入" else " · 只读"
                if (status.text.toString() != next) status.text = next
            }
        } catch (e: Exception) {
            disconnect()
            status.text = e.message ?: "显示同步失败，请重新连接"
        }
        Choreographer.getInstance().postFrameCallback(this)
    }
    private fun disconnect() { generation++; selected = null; controlled = false; takeControl.isChecked = false; sessionsBox.removeAllViews(); work { remote.disconnect() } }
    private fun signedOut() { accountName = ""; loginBox.visibility = View.VISIBLE; accountBar.visibility = View.GONE; devicesBox.removeAllViews(); sessionsBox.removeAllViews(); surface.removeAllViews(); terminal = null; status.text = "已退出登录" }
    private fun passwordDialog() {
        val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }; val old = field("当前密码", true); val next = field("新密码（至少 12 字节）", true); box.addView(old); box.addView(next)
        android.app.AlertDialog.Builder(this).setTitle("修改密码").setView(box).setNegativeButton("取消", null).setPositiveButton("保存") { _, _ -> val current = old.text.toString(); val password = next.text.toString(); disconnect(); work { try { account.changePassword(current, password); ui.post { signedOut(); status.text = "密码已更新，请重新登录" } } finally { persist() } } }.show()
    }
    override fun onStart() { super.onStart(); active = true; Choreographer.getInstance().postFrameCallback(this) }
    override fun onStop() { active = false; Choreographer.getInstance().removeFrameCallback(this); disconnect(); super.onStop() }
    override fun onDestroy() { worker.shutdown(); historyWorker.shutdown(); super.onDestroy() }
}
class TerminalView(context: Context, private var frame: RenderFrame) : View(context) {
    private var cells = frame.cells.toMutableList()
    private var sourceEpoch: ULong? = null
    private var sourceGeneration: ULong? = null
    init {
        frame = frame.copy(cells = cells)
        contentDescription = "终端屏幕"
        setOnLongClickListener {
            val text = frame.cells.chunked(frame.cols.toInt()).joinToString("\n") { row -> row.filter { it.width > 0u }.joinToString("") { it.text } }
            (context.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager)
                .setPrimaryClip(android.content.ClipData.newPlainText("Terminal", text))
            true
        }
    }
    private val rowNodes = mutableMapOf<Int, RenderNode>()
    private val changedRows = mutableSetOf<Int>()
    fun zoom(scale: Float) { rowNodes.clear(); paint.textSize = 15f * resources.displayMetrics.scaledDensity * scale; metrics.textSize = paint.textSize; cellWidth = metrics.measureText("M"); cellHeight = metrics.fontSpacing; baseline = -metrics.fontMetrics.top; requestLayout(); invalidate() }
    fun update(next: RenderFrame) { rowNodes.clear(); changedRows.clear(); val resized = next.rows != frame.rows || next.cols != frame.cols; cells = next.cells.toMutableList(); frame = next.copy(cells = cells); sourceEpoch = null; sourceGeneration = null; if (resized) requestLayout(); invalidate() }
    fun apply(update: RenderUpdate) {
        val resized = frame.rows != update.rows || frame.cols != update.cols
        val count = update.rows.toLong() * update.cols.toLong()
        require(update.rows > 0u && update.cols > 0u && count <= 100_000 && update.cursorRow < update.rows && update.cursorCol < update.cols) { "Invalid display dimensions" }
        require(sourceEpoch == null || (sourceEpoch == update.epoch && sourceGeneration == update.generation)) { "Display session changed without an initial frame" }
        require(update.full || (!resized && sourceEpoch != null)) { "Display delta has no baseline" }
        if (update.revision < frame.revision) return
        if (update.revision == frame.revision && sourceEpoch != null) return
        // Validate the entire batch before changing the owned buffer; malformed updates cannot
        // partially overwrite an otherwise valid screen. Rust already verifies the state hash.
        var previous = -1L
        for (patch in update.patches) {
            val index = patch.index.toLong()
            require(index > previous && index < count && (!update.full || index == previous + 1)) { "Invalid display patch order or index" }
            previous = index
        }
        require(!update.full || update.patches.size.toLong() == count) { "Incomplete display snapshot" }
        val dirty = mutableSetOf(frame.cursorRow.toInt(), update.cursorRow.toInt())
        if (update.full) cells = update.patches.mapTo(ArrayList(update.patches.size)) { it.cell }
        else for (patch in update.patches) { val i = patch.index.toInt(); cells[i] = patch.cell; dirty.add(i / update.cols.toInt()) }
        frame = RenderFrame(update.rows, update.cols, update.revision, cells, update.cursorRow, update.cursorCol, update.cursorVisible, update.cursorShape)
        sourceEpoch = update.epoch; sourceGeneration = update.generation
        if (update.full || resized) { rowNodes.clear(); changedRows.clear() } else changedRows.addAll(dirty)
        if (resized) requestLayout()
        if (update.full) invalidate() else for (row in dirty) invalidate(0, (row * cellHeight).toInt(), width, kotlin.math.ceil((row + 1) * cellHeight.toDouble()).toInt())
    }
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { typeface = Typeface.MONOSPACE; textSize = 15f * resources.displayMetrics.scaledDensity; fontFeatureSettings = "'liga' 0" }
    private val metrics = Paint(paint)
    private var cellWidth = metrics.measureText("M")
    private var cellHeight = metrics.fontSpacing
    private var baseline = -metrics.fontMetrics.top
    override fun onMeasure(width: Int, height: Int) {
        setMeasuredDimension((frame.cols.toInt() * cellWidth).toInt(), (frame.rows.toInt() * cellHeight).toInt())
    }
    private fun drawRow(canvas: Canvas, row: Int, y: Float) {
        val cols = frame.cols.toInt()
        var col = 0
        paint.style = Paint.Style.FILL; paint.isAntiAlias = false; paint.alpha = 255
        while (col < cols) {
            val start = col; val background = frame.cells[row * cols + col].background
            while (col < cols && frame.cells[row * cols + col].background == background) col++
            paint.color = background.toInt() or 0xff000000.toInt()
            canvas.drawRect(start * cellWidth, y, col * cellWidth, y + cellHeight, paint)
        }
        paint.isAntiAlias = true; col = 0
        while (col < cols) {
            val cell = frame.cells[row * cols + col]
            if (cell.width == 0u) { col++; continue }
            val start = col; col += cell.width.toInt()
            val text = StringBuilder(cell.text)
            if (cell.width == 1u && cell.text.length == 1 && cell.text[0].code in 32..126) {
                while (col < cols) {
                    val next = frame.cells[row * cols + col]
                    if (next.width != 1u || next.text.length != 1 || next.text[0].code !in 32..126 || next.foreground != cell.foreground || next.style != cell.style) break
                    text.append(next.text); col++
                }
            }
            if (cell.style and 12u == 0u && text.all { it == ' ' }) continue
            val save = canvas.save()
            canvas.clipRect(start * cellWidth, y, col * cellWidth, y + cellHeight)
            paint.color = cell.foreground.toInt() or 0xff000000.toInt()
            paint.isFakeBoldText = cell.style and 1u != 0u
            paint.textSkewX = if (cell.style and 2u != 0u) -0.25f else 0f
            paint.isUnderlineText = cell.style and 4u != 0u
            paint.isStrikeThruText = cell.style and 8u != 0u
            paint.alpha = if (cell.style and 16u != 0u) 170 else 255
            canvas.drawText(text.toString(), start * cellWidth, y + baseline, paint)
            canvas.restoreToCount(save)
        }
    }
    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val clip = canvas.clipBounds
        val firstRow = (clip.top / cellHeight).toInt().coerceAtLeast(0)
        val endRow = kotlin.math.ceil(clip.bottom / cellHeight.toDouble()).toInt().coerceAtMost(frame.rows.toInt())
        for (row in firstRow until endRow) {
            if (Build.VERSION.SDK_INT >= 29 && canvas.isHardwareAccelerated) {
                var node = rowNodes[row]
                if (node == null || changedRows.remove(row)) {
                    node = node ?: RenderNode("terminal-row-$row").also { rowNodes[row] = it }
                    node.setPosition(0, (row * cellHeight).toInt(), width, kotlin.math.ceil((row + 1) * cellHeight.toDouble()).toInt())
                    val recording = node.beginRecording(width, kotlin.math.ceil(cellHeight.toDouble()).toInt())
                    try { drawRow(recording, row, 0f) } finally { node.endRecording() }
                }
                canvas.drawRenderNode(node)
            } else { drawRow(canvas, row, row * cellHeight) }
        }
        paint.alpha = 255
        if (frame.cursorVisible) {
            paint.color = 0xffeeeeee.toInt()
            paint.style = if (frame.cursorShape == 3u) Paint.Style.STROKE else Paint.Style.FILL
            val x = frame.cursorCol.toInt() * cellWidth
            val y = frame.cursorRow.toInt() * cellHeight
            when (frame.cursorShape) {
                1u -> canvas.drawRect(x, y, x + 2f, y + cellHeight, paint)
                2u -> canvas.drawRect(x, y + cellHeight - 2f, x + cellWidth, y + cellHeight, paint)
                else -> { paint.alpha = 120; canvas.drawRect(x, y, x + cellWidth, y + cellHeight, paint) }
            }
            paint.alpha = 255
            paint.style = Paint.Style.FILL
        }
    }
}
