package com.yxf.aterminal

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.graphics.RenderNode
import android.os.Build
import android.text.InputType
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import android.widget.HorizontalScrollView
import android.widget.ScrollView
import uniffi.ai_terminal_mobile.*

class TerminalView(context: Context, private var frame: RenderFrame) : View(context) {
    var canType: () -> Boolean = { false }
    var sendText: (String) -> Boolean = { false }
    var sendPaste: (String) -> Boolean = { false }
    var sendKey: (String) -> Boolean = { false }
    var keyboardOpened: () -> Unit = {}
    var readOnlyTapped: () -> Unit = {}
    private var cells = frame.cells.toMutableList()
    private var sourceEpoch: ULong? = null
    private var sourceGeneration: ULong? = null
    private var followCursor = true
    init {
        frame = frame.copy(cells = cells)
        contentDescription = "终端屏幕，点击输入"
        isFocusable = true
        isFocusableInTouchMode = true
        setOnClickListener { focusKeyboard() }
        setOnLongClickListener {
            val text = frame.cells.chunked(frame.cols.toInt()).joinToString("\n") { row -> row.filter { it.width > 0u }.joinToString("") { it.text } }
            (context.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager)
                .setPrimaryClip(android.content.ClipData.newPlainText("Terminal", text))
            true
        }
    }
    fun focusKeyboard(): Boolean {
        if (!canType()) { readOnlyTapped(); return false }
        followCursor = true
        requestFocus()
        (context.getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager)
            .showSoftInput(this, InputMethodManager.SHOW_IMPLICIT)
        keyboardOpened()
        revealCursor()
        return true
    }
    fun stopFollowingCursor() { followCursor = false }
    private fun revealCursor() {
        if (!followCursor || !frame.cursorVisible) return
        post {
            if (!followCursor || !frame.cursorVisible) return@post
            val horizontal = parent as? HorizontalScrollView ?: return@post
            val x = (frame.cursorCol.toInt() * cellWidth).toInt()
            val y = (frame.cursorRow.toInt() * cellHeight).toInt()
            val margin = (12 * resources.displayMetrics.density).toInt().coerceAtLeast((6 * cellWidth).toInt())
            if (horizontal.width > 0) {
                val right = x + cellWidth.toInt() + margin
                val left = (x - margin).coerceAtLeast(0)
                if (left < horizontal.scrollX) horizontal.scrollTo(left, horizontal.scrollY)
                else if (right > horizontal.scrollX + horizontal.width) horizontal.scrollTo(right - horizontal.width, horizontal.scrollY)
            }
            val vertical = horizontal.parent as? ScrollView
            if (vertical != null && vertical.height > 0) {
                val verticalMargin = cellHeight.toInt()
                val bottom = y + cellHeight.toInt() + verticalMargin
                val top = (y - verticalMargin).coerceAtLeast(0)
                if (top < vertical.scrollY) vertical.scrollTo(vertical.scrollX, top)
                else if (bottom > vertical.scrollY + vertical.height) vertical.scrollTo(vertical.scrollX, bottom - vertical.height)
            }
        }
    }
    override fun onCheckIsTextEditor() = true
    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_ACTION_NONE
        return object : BaseInputConnection(this@TerminalView, true) {
            private var composing = ""
            private var committing = false
            override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
                composing = text?.toString().orEmpty()
                return super.setComposingText(text, newCursorPosition)
            }
            override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                committing = true
                val updated = super.commitText(text, newCursorPosition)
                committing = false
                composing = ""
                editable?.clear()
                text?.toString()?.let(::committedText)
                return updated
            }
            override fun finishComposingText(): Boolean {
                val pending = composing
                val updated = super.finishComposingText()
                if (!committing && pending.isNotEmpty()) {
                    composing = ""
                    editable?.clear()
                    committedText(pending)
                }
                return updated
            }
            override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
                if (composing.isNotEmpty()) {
                    val updated = super.deleteSurroundingText(beforeLength, afterLength)
                    composing = editable?.toString().orEmpty()
                    return updated
                }
                return sendKey("backspace")
            }
            override fun performEditorAction(actionCode: Int) = sendKey("enter")
            override fun performContextMenuAction(id: Int): Boolean =
                if (id == android.R.id.paste || id == android.R.id.pasteAsPlainText) pasteClipboard()
                else super.performContextMenuAction(id)
            override fun sendKeyEvent(event: KeyEvent): Boolean =
                if ((event.action == KeyEvent.ACTION_DOWN || event.action == KeyEvent.ACTION_MULTIPLE) && terminalKey(event)) true
                else event.action == KeyEvent.ACTION_UP || super.sendKeyEvent(event)
        }
    }
    private fun committedText(value: String) {
        // IMEs do not identify clipboard commits. Only a standalone control is a
        // key; preserve mixed/batched text as one paste instead of executing it.
        when {
            value == "\t" -> sendKey("tab")
            value == "\n" || value == "\r" || value == "\r\n" -> sendKey("enter")
            value.any { it == '\t' || it == '\n' || it == '\r' } -> sendPaste(value)
            value.isNotEmpty() -> sendText(value)
        }
    }
    fun pasteClipboard(): Boolean {
        if (!canType()) { readOnlyTapped(); return false }
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        val clip = clipboard.primaryClip ?: return false
        if (clip.itemCount == 0) return false
        val text = clip.getItemAt(0).coerceToText(context)?.toString() ?: return false
        return text.isNotEmpty() && sendPaste(text)
    }
    private fun terminalKey(event: KeyEvent): Boolean {
        if (event.keyCode == KeyEvent.KEYCODE_V && event.isCtrlPressed) { pasteClipboard(); return true }
        if (event.action == KeyEvent.ACTION_MULTIPLE && event.keyCode == KeyEvent.KEYCODE_UNKNOWN) {
            event.characters?.let { committedText(it); return true }
        }
        val key = when (event.keyCode) {
            KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> "enter"
            KeyEvent.KEYCODE_TAB -> "tab"
            KeyEvent.KEYCODE_DEL, KeyEvent.KEYCODE_FORWARD_DEL -> "backspace"
            KeyEvent.KEYCODE_ESCAPE -> "escape"
            KeyEvent.KEYCODE_DPAD_UP -> "up"
            KeyEvent.KEYCODE_DPAD_DOWN -> "down"
            KeyEvent.KEYCODE_DPAD_LEFT -> "left"
            KeyEvent.KEYCODE_DPAD_RIGHT -> "right"
            KeyEvent.KEYCODE_C -> if (event.isCtrlPressed) "ctrl_c" else null
            else -> null
        }
        if (key != null) { sendKey(key); return true }
        if (event.isCtrlPressed || event.isAltPressed) return false
        val codePoint = event.unicodeChar
        if (codePoint > 0) { committedText(String(Character.toChars(codePoint))); return true }
        return false
    }
    fun handleHardwareKey(event: KeyEvent): Boolean = terminalKey(event)
    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean = terminalKey(event) || super.onKeyDown(keyCode, event)
    override fun onKeyMultiple(keyCode: Int, repeatCount: Int, event: KeyEvent): Boolean = terminalKey(event) || super.onKeyMultiple(keyCode, repeatCount, event)
    private val rowNodes = mutableMapOf<Int, RenderNode>()
    private val changedRows = mutableSetOf<Int>()
    fun zoom(scale: Float) { rowNodes.clear(); paint.textSize = 15f * resources.displayMetrics.scaledDensity * scale; metrics.textSize = paint.textSize; cellWidth = metrics.measureText("M"); cellHeight = metrics.fontSpacing; baseline = -metrics.fontMetrics.top; requestLayout(); invalidate() }
    fun update(next: RenderFrame) { rowNodes.clear(); changedRows.clear(); val resized = next.rows != frame.rows || next.cols != frame.cols; cells = next.cells.toMutableList(); frame = next.copy(cells = cells); sourceEpoch = null; sourceGeneration = null; if (resized) requestLayout(); invalidate(); revealCursor() }
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
        revealCursor()
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
            val x = frame.cursorCol.toInt() * cellWidth
            val y = frame.cursorRow.toInt() * cellHeight
            val stroke = (2f * resources.displayMetrics.density).coerceAtLeast(2f)
            paint.color = 0xffe5e5e5.toInt(); paint.style = Paint.Style.FILL; paint.alpha = 255
            when (frame.cursorShape) {
                0u, 1u -> canvas.drawRect(x, y, x + stroke, y + cellHeight, paint)
                2u -> canvas.drawRect(x, y + cellHeight - stroke, x + cellWidth, y + cellHeight, paint)
                3u -> { paint.style = Paint.Style.STROKE; paint.strokeWidth = stroke; canvas.drawRect(x + stroke / 2, y + stroke / 2, x + cellWidth - stroke / 2, y + cellHeight - stroke / 2, paint) }
                else -> {
                    canvas.drawRect(x, y, x + cellWidth, y + cellHeight, paint)
                    val cell = frame.cells[frame.cursorRow.toInt() * frame.cols.toInt() + frame.cursorCol.toInt()]
                    if (cell.width > 0u && cell.text.isNotBlank()) {
                        val saved = canvas.save(); canvas.clipRect(x, y, x + cellWidth, y + cellHeight)
                        paint.color = 0xff101014.toInt(); paint.isFakeBoldText = false; paint.isUnderlineText = false; paint.isStrikeThruText = false; paint.textSkewX = 0f
                        canvas.drawText(cell.text, x, y + baseline, paint); canvas.restoreToCount(saved)
                    }
                }
            }
            paint.alpha = 255
            paint.style = Paint.Style.FILL
        }
    }
}
