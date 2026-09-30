package com.yxf.aterminal

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.widget.*

object Palette {
    val background = 0xff090d16.toInt()
    val surface = 0xff101623.toInt()
    val control = 0xff141c2c.toInt()
    val line = 0xff253044.toInt()
    val text = 0xfff8fafc.toInt()
    val secondary = 0xffcbd5e1.toInt()
    val warning = 0xfffbbf24.toInt()
    val muted = 0xff8391a7.toInt()
    val accent = 0xff38bdf8.toInt()
    val green = 0xff34d399.toInt()
    val danger = 0xfffb7185.toInt()
}
fun Context.dp(value: Int) = (value * resources.displayMetrics.density).toInt()
fun Context.shape(color: Int, border: Boolean = false) = GradientDrawable().apply {
    setColor(color); cornerRadius = dp(8).toFloat(); if (border) setStroke(dp(1), Palette.line)
}
fun Context.column(padding: Int = 0) = LinearLayout(this).apply {
    orientation = LinearLayout.VERTICAL; setPadding(dp(padding), dp(padding), dp(padding), dp(padding))
}
fun Context.row() = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL }
fun Context.label(value: String, size: Float = 14f, color: Int = Palette.text) = TextView(this).apply {
    text = value; textSize = size; setTextColor(color); letterSpacing = 0f
    setPadding(0, dp(4), 0, dp(4))
}
fun Context.heading(value: String) = label(value, 18f).apply { setTypeface(typeface, Typeface.BOLD) }
fun Context.field(hint: String, secret: Boolean = false) = EditText(this).apply {
    this.hint = hint; contentDescription = hint; textSize = 16f; setTextColor(Palette.text); setHintTextColor(Palette.muted)
    inputType = InputType.TYPE_CLASS_TEXT or if (secret) InputType.TYPE_TEXT_VARIATION_PASSWORD else InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
    isSingleLine = true; minHeight = dp(50); background = shape(Palette.surface, true)
    if (secret) transformationMethod = android.text.method.PasswordTransformationMethod.getInstance()
    setPadding(dp(12), dp(10), dp(12), dp(10)); backgroundTintList = null
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_YES
}
fun Context.actionButton(value: String, primary: Boolean = false, action: () -> Unit) = Button(this).apply {
    text = value; isAllCaps = false; textSize = 14f; minHeight = dp(48); minimumWidth = 0
    setPadding(dp(12), dp(6), dp(12), dp(6)); setTextColor(Palette.text)
    compoundDrawableTintList = ColorStateList.valueOf(Palette.secondary); compoundDrawablePadding = dp(8)
    background = if (primary) GradientDrawable(GradientDrawable.Orientation.LEFT_RIGHT, intArrayOf(Palette.accent, 0xff2563eb.toInt())).apply {
        cornerRadius = dp(10).toFloat(); setStroke(dp(1), 0xff60a5fa.toInt())
    } else shape(Palette.control, true)
    setOnClickListener { action() }
}
fun Context.iconButton(icon: Int, description: String, action: () -> Unit) = ImageButton(this).apply {
    setImageResource(icon); imageTintList = ColorStateList.valueOf(Palette.accent)
    contentDescription = description
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) tooltipText = description
    background = shape(Palette.control); setPadding(dp(13), dp(13), dp(13), dp(13))
    scaleType = ImageView.ScaleType.CENTER_INSIDE
    layoutParams = LinearLayout.LayoutParams(dp(44), dp(44)).apply { marginStart = dp(4) }
    setOnClickListener { action() }
}
fun LinearLayout.gap(size: Int = 12) { addView(View(context), LinearLayout.LayoutParams(1, context.dp(size))) }
fun LinearLayout.fill(view: View) { addView(view, LinearLayout.LayoutParams(0, -2, 1f)) }
fun LinearLayout.grow(view: View) { addView(view, LinearLayout.LayoutParams(-1, 0, 1f)) }
fun Context.scroll(view: View) = ScrollView(this).apply { isFillViewport = true; addView(view) }

/** Settings-specific rows; chat controls retain their compact touch targets. */
fun Context.settingsRow(title: String, subtitle: String, icon: Int? = null, action: (() -> Unit)? = null) = row().apply {
    minimumHeight = dp(80); setPadding(dp(12), dp(12), dp(12), dp(12)); background = shape(Palette.surface)
    icon?.let { addView(ImageView(context).apply { setImageResource(it); imageTintList = ColorStateList.valueOf(Palette.accent) }, LinearLayout.LayoutParams(dp(20), dp(20)).apply { marginEnd = dp(14) }) }
    fill(column().apply { addView(label(title, 16f)); addView(label(subtitle, 12f, Palette.muted)) })
    if (action != null) {
        addView(ImageView(context).apply { setImageResource(R.drawable.ic_chevron_right); imageTintList = ColorStateList.valueOf(Palette.muted) }, LinearLayout.LayoutParams(dp(18), dp(18)))
        isFocusable = true; isClickable = true; contentDescription = title; setOnClickListener { action() }
    }
}
fun LinearLayout.settingsDivider() { addView(View(context).apply { setBackgroundColor(Palette.line) }, LinearLayout.LayoutParams(-1, context.dp(1))) }
fun LinearLayout.labelled(title: String, control: View): LinearLayout = context.column().also {
    it.addView(context.label(title, 14f, Palette.secondary)); it.gap(6); it.addView(control); it.gap(20); addView(it)
}
