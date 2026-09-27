package com.yxf.aterminal

import android.content.Context
import android.widget.TextView
import io.noties.markwon.AbstractMarkwonPlugin
import io.noties.markwon.Markwon
import io.noties.markwon.SoftBreakAddsNewLinePlugin
import io.noties.markwon.core.MarkwonTheme
import io.noties.markwon.ext.strikethrough.StrikethroughPlugin
import io.noties.markwon.ext.tables.TablePlugin
import io.noties.markwon.ext.tasklist.TaskListPlugin

/** Native selectable Markdown for both streamed and durable chat messages. */
class ChatMarkdown(private val context: Context) {
    private val renderer = Markwon.builder(context)
        .usePlugin(SoftBreakAddsNewLinePlugin.create())
        .usePlugin(StrikethroughPlugin.create())
        .usePlugin(TaskListPlugin.create(context))
        .usePlugin(TablePlugin.create { theme ->
            theme.tableCellPadding(context.dp(8)).tableBorderWidth(context.dp(1))
                .tableBorderColor(Palette.line).tableHeaderRowBackgroundColor(Palette.control)
                .tableOddRowBackgroundColor(Palette.surface)
        })
        .usePlugin(object : AbstractMarkwonPlugin() {
            override fun configureTheme(builder: MarkwonTheme.Builder) {
                builder.linkColor(Palette.accent).blockQuoteColor(Palette.accent)
                    .codeTextColor(Palette.text).codeBackgroundColor(Palette.control)
                    .codeBlockTextColor(Palette.text).codeBlockBackgroundColor(Palette.control)
                    .headingBreakColor(Palette.line).thematicBreakColor(Palette.line)
            }
        }).build()

    fun view(source: String): TextView = context.label("", 16f).apply {
        tag = "markdown-message"
        setTextIsSelectable(true)
        renderer.setMarkdown(this, source)
    }
}
