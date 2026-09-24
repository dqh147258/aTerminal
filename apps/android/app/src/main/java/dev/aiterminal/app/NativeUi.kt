package dev.aiterminal.app

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.widget.*

object Palette {
    val background = 0xff121416.toInt()
    val surface = 0xff1a1d20.toInt()
    val control = 0xff24292d.toInt()
    val line = 0xff30363a.toInt()
    val text = 0xffedf0f2.toInt()
    val muted = 0xffa0a8ae.toInt()
    val accent = 0xffa5c4d4.toInt()
    val green = 0xffa2c6ae.toInt()
    val danger = 0xffe4a3a3.toInt()
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
    importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_YES
}
fun Context.actionButton(value: String, primary: Boolean = false, action: () -> Unit) = Button(this).apply {
    text = value; isAllCaps = false; textSize = 14f; minHeight = dp(48); minimumWidth = 0
    setPadding(dp(12), dp(6), dp(12), dp(6)); setTextColor(if (primary) Palette.background else Palette.text)
    background = shape(if (primary) Palette.accent else Palette.control)
    setOnClickListener { action() }
}
fun Context.iconButton(icon: Int, description: String, action: () -> Unit) = ImageButton(this).apply {
    setImageResource(icon); imageTintList = ColorStateList.valueOf(Palette.accent)
    contentDescription = description; tooltipText = description
    background = shape(Palette.control); setPadding(dp(13), dp(13), dp(13), dp(13))
    scaleType = ImageView.ScaleType.CENTER_INSIDE
    layoutParams = LinearLayout.LayoutParams(dp(48), dp(48)).apply { marginStart = dp(4) }
    setOnClickListener { action() }
}
fun LinearLayout.gap(size: Int = 12) { addView(View(context), LinearLayout.LayoutParams(1, context.dp(size))) }
fun LinearLayout.fill(view: View) { addView(view, LinearLayout.LayoutParams(0, -2, 1f)) }
fun LinearLayout.grow(view: View) { addView(view, LinearLayout.LayoutParams(-1, 0, 1f)) }
fun Context.scroll(view: View) = ScrollView(this).apply { isFillViewport = true; addView(view) }
