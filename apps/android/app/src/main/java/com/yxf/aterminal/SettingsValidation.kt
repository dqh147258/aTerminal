package com.yxf.aterminal

import org.json.JSONObject
import java.net.URI

/** Preflight for the public Rust config contract. Desktop remains authoritative. */
internal object SettingsValidation {
    fun endpoint(value: String): Boolean = runCatching {
        val uri = URI(value)
        uri.scheme in listOf("http", "https") && !uri.host.isNullOrEmpty() && uri.rawUserInfo == null
    }.getOrDefault(false)

    fun reasoning(protocol: String, value: JSONObject, caps: JSONObject, max: Long, sampling: Boolean): String? {
        return when (value.getString("mode")) {
            "level" -> {
                val levels = caps.optJSONArray("reasoning_levels")
                when {
                    protocol !in listOf("openai_responses", "openai_chat", "azure_openai", "gemini") -> "此协议不支持思考等级"
                    levels == null || (0 until levels.length()).none { levels.optString(it) == value.optString("level") } -> "思考等级必须在模型声明的等级中"
                    else -> null
                }
            }
            "budget" -> {
                val range = caps.optJSONArray("reasoning_budget"); val tokens = value.optLong("tokens", -1)
                when {
                    protocol !in listOf("anthropic", "gemini") -> "此协议不支持 token 思考预算"
                    range == null || tokens < range.optLong(0) || tokens > range.optLong(1) || tokens >= max -> "思考预算须在声明范围内且小于最大输出"
                    protocol == "anthropic" && sampling -> "Anthropic 思考预算不能同时设置温度或 Top P"
                    else -> null
                }
            }
            "adaptive" -> when {
                protocol != "anthropic" || !caps.optBoolean("reasoning_adaptive") -> "此模型或协议不支持 adaptive"
                sampling -> "Adaptive 不能同时设置温度或 Top P"
                else -> null
            }
            "disabled" -> if (!caps.optBoolean("reasoning_disabled") || protocol !in listOf("anthropic", "gemini", "ollama")) "此模型或协议不支持关闭思考" else null
            else -> null
        }
    }
}
