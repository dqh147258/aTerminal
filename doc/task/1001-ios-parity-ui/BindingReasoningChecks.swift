import Foundation
@main struct BindingReasoningChecks {
    static func object(_ value: Any?) -> [String: Any] { value as? [String: Any] ?? [:] }
    static func config() -> [String: Any] {
        ["providers": ["p": ["connection": ["protocol": "openai_responses", "endpoint": "https://fixture.invalid"]]],
         "models": ["m": ["id": "m", "model": "same", "provider_id": "p", "context_window": 128000, "max_tokens": 4096, "reasoning": ["mode": "provider_default"], "capabilities": ["reasoning_levels": ["low", "high"]]]],
         "bindings": ["global": ["model_id": "m", "reasoning": ["mode": "level", "level": "low"], "future": "keep"], "session-default": ["model_id": "m", "reasoning": ["mode": "level", "level": "high"], "future": "keep-high"], "session/unrelated": ["model_id": "other", "reasoning": ["mode": "level", "level": "low"], "future": "other"]]]
    }
    static func require(_ condition: Bool, _ reason: String) throws { if !condition { throw NSError(domain: reason, code: 1) } }
    static func main() throws {
        for catalog in [false, true] {
            let config = config(); var draft = ModelDraft(id: "m", item: object(object(config["models"])["m"]))
            if catalog { draft.select(["id": "same", "capabilities": ["reasoning_levels": ["high"]]]) } else { draft.levels = "high" }
            let bindings = object(try draft.candidate(config, editing: true)["bindings"])
            try require(object(bindings["global"])["reasoning"] == nil && object(bindings["global"])["future"] as? String == "keep", "invalid low override/unknown field")
            try require(object(object(bindings["session-default"])["reasoning"])["level"] as? String == "high" && object(bindings["session-default"])["future"] as? String == "keep-high", "compatible high override")
            try require(object(object(bindings["session/unrelated"])["reasoning"])["level"] as? String == "low", "unrelated model")
        }
        var config = config(); var draft = ModelDraft(id: "m", item: object(object(config["models"])["m"]))
        config["providers"] = ["p": ["connection": ["protocol": "anthropic", "endpoint": "https://fixture.invalid"]]]
        let protocolBindings = object(try draft.candidate(config, editing: true)["bindings"])
        try require(object(protocolBindings["session-default"])["reasoning"] == nil, "protocol incompatibility")
        config["bindings"] = ["global": ["model_id": "m", "reasoning": ["mode": "budget", "tokens": 2048], "future": "budget"]]
        draft.budgetMin = "0"; draft.budgetMax = "4096"; draft.output = "2048"
        let budget = object(object(try draft.candidate(config, editing: true)["bindings"])["global"])
        try require(budget["reasoning"] == nil && budget["future"] as? String == "budget", "budget output compatibility")
        print("PASS: same-ID catalog, advanced capability edits, compatible override preservation, unknown fields, other models, protocol and output limits")
    }
}
