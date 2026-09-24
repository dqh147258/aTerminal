package dev.aiterminal.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.graphics.RenderNode
import android.os.Build
import android.view.View
import uniffi.ai_terminal_mobile.*

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
