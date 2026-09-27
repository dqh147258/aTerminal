package com.yxf.aterminal

import android.content.Context
import android.view.MotionEvent
import android.view.ViewConfiguration
import android.widget.ScrollView
import kotlin.math.abs

/** Scrolls the oversized live grid first; pulling past its top reads retained history. */
class TerminalScrollView(context: Context) : ScrollView(context) {
    var canReadHistory: () -> Boolean = { false }
    var isReading: () -> Boolean = { false }
    var lineHeight: () -> Float = { 20f }
    var scrollHistory: (Int) -> Unit = {}
    var stopFollowing: () -> Unit = {}
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    private var startX = 0f
    private var startY = 0f
    private var lastY = 0f
    private var remainder = 0f
    private var readingGesture = false
    private fun begin(event: MotionEvent) {
        startX = event.x; startY = event.y; lastY = event.y
        readingGesture = false; remainder = 0f
    }
    private fun startReading(event: MotionEvent): Boolean {
        if (event.actionMasked != MotionEvent.ACTION_MOVE) return false
        val dy = event.y - startY
        if (abs(dy) <= slop || abs(dy) <= abs(event.x - startX)) { lastY = event.y; return false }
        stopFollowing()
        if (!canReadHistory() || (!isReading() && (dy <= 0 || canScrollVertically(-1)))) { lastY = event.y; return false }
        readingGesture = true
        parent?.requestDisallowInterceptTouchEvent(true)
        return true
    }
    override fun onInterceptTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                begin(event)
            }
            MotionEvent.ACTION_MOVE -> {
                if (startReading(event)) return true
            }
        }
        return super.onInterceptTouchEvent(event)
    }
    override fun onTouchEvent(event: MotionEvent): Boolean {
        // A ScrollView already handling a gesture (or a touch on blank space)
        // receives MOVE directly here, without another interception callback.
        if (event.actionMasked == MotionEvent.ACTION_DOWN) begin(event)
        if (!readingGesture) startReading(event)
        if (!readingGesture) {
            val handled = super.onTouchEvent(event)
            lastY = event.y
            return handled
        }
        when (event.actionMasked) {
            MotionEvent.ACTION_MOVE -> {
                remainder += event.y - lastY; lastY = event.y
                val height = lineHeight().coerceAtLeast(1f)
                val lines = (remainder / height).toInt()
                if (lines != 0) { remainder -= lines * height; scrollHistory(lines) }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                readingGesture = false; parent?.requestDisallowInterceptTouchEvent(false)
            }
        }
        return true
    }
    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_SCROLL && canReadHistory()) {
            val vertical = event.getAxisValue(MotionEvent.AXIS_VSCROLL)
            if (vertical != 0f && (isReading() || !canScrollVertically(if (vertical > 0) -1 else 1))) {
                stopFollowing()
                val lines = (vertical * 3).toInt().let { if (it == 0) { if (vertical > 0) 1 else -1 } else it }
                scrollHistory(lines)
                return true
            }
        }
        return super.onGenericMotionEvent(event)
    }
}
