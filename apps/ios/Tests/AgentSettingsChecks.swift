import Foundation

// Links the production SettingsDraft.swift directly. No SwiftUI, account model,
// normal cache, Keychain, migration, network, simulator, or copied reducer.
private struct CheckFailure: Error { let message: String }
private func check(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    if try !condition() { throw CheckFailure(message: message) }
}
private func rejects(_ message: String, _ operation: () throws -> Void) throws {
    var rejected = false
    do { try operation() } catch { rejected = true }
    try check(rejected, message)
}
private func object(_ value: Any?) -> [String: Any] { value as? [String: Any] ?? [:] }
private func same(_ left: Any, _ right: Any) -> Bool {
    guard let a = try? JSONSerialization.data(withJSONObject: left, options: [.sortedKeys, .fragmentsAllowed]),
          let b = try? JSONSerialization.data(withJSONObject: right, options: [.sortedKeys, .fragmentsAllowed]) else { return false }
    return a == b
}

@MainActor private final class ControlledTransport {
    typealias Object = [String: Any]
    private(set) var commands: [Object] = []
    private var pending: [CheckedContinuation<Object, Error>] = []
    private var nextIndex = 0
    private var observer: CheckedContinuation<Object, Never>?
    func call(_ command: Object) async throws -> Object {
        try await withCheckedThrowingContinuation { continuation in
            commands.append(command); pending.append(continuation)
            if let observer { self.observer = nil; nextIndex += 1; observer.resume(returning: command) }
        }
    }
    func next() async -> Object {
        if nextIndex < commands.count { defer { nextIndex += 1 }; return commands[nextIndex] }
        return await withCheckedContinuation { observer = $0 }
    }
    func resolve(_ value: Object) { pending.removeFirst().resume(returning: value) }
    func fail(_ message: String) { pending.removeFirst().resume(throwing: SettingsFailure.message(message)) }
}

@main private struct AgentSettingsChecks {
    static func config() -> [String: Any] {
        ["schema": 1, "future_root": ["sentinel": true],
         "providers": ["p": ["id": "p", "name": "Provider", "credential_revision": 3, "enabled": true,
                             "catalog_url": "https://fixture.invalid/catalog", "secret_ref": "vault/p",
                             "future_provider": "keep", "connection": ["protocol": "openai_responses", "endpoint": "https://fixture.invalid/v1", "future_connection": 7]]],
         "models": ["m": ["id": "m", "name": "Profile", "provider_id": "p", "model": "same-model", "context_window": 128000,
                          "max_tokens": 4096, "max_rounds": 7, "max_seconds": 90, "read_only": true, "future_model": "keep",
                          "reasoning": ["mode": "level", "level": "low"],
                          "capabilities": ["tools": true, "vision": true, "streaming": true, "temperature": true, "top_p": true,
                                           "reasoning_levels": ["low", "high"], "reasoning_budget": [128, 1024],
                                           "reasoning_disabled": true, "reasoning_adaptive": false, "future_capability": "snapshot"]]],
         "bindings": ["global": ["model_id": "m", "reasoning": ["mode": "level", "level": "low"], "future_binding": "keep"],
                      "session-default": ["model_id": "m"], "session/current": ["model_id": "m", "reasoning": ["mode": "level", "level": "high"]]],
         "terminal_reading": ["head_lines": 10, "tail_lines": 20, "future_reading": 5],
         "mcp": ["existing": ["command": "fixture", "args": [], "enabled": true, "envSecretRefs": ["TOKEN": "vault/token"]]],
         "skills": ["user/existing": ["id": "user/existing", "enabled": true, "future_skill": "keep"]]]
    }
    static func providerContracts() throws {
        let original = object(object(config()["providers"])["p"])
        var draft = ProviderDraft(id: "p", item: original)
        draft.endpoint = "https://fixture.invalid/changed"
        let result = try draft.value(previous: original)
        try check(result["catalog_url"] as? String == original["catalog_url"] as? String, "Provider catalog URL lost")
        try check(result["secret_ref"] as? String == "vault/p" && result["credential_revision"] as? Int == 3, "Credential references changed")
        try check(result["future_provider"] as? String == "keep" && object(result["connection"])["future_connection"] as? Int == 7, "Provider hidden fields lost")
        try check(draft.key.isEmpty, "Editing must not hydrate credential contents")
        for key in ["", " \n\t "] { draft.key = key; try check(draft.secrets().isEmpty, "Blank provider key must preserve its existing credential") }
        draft.key = "fixture-only-key"
        try check(draft.secrets()["p"] == "fixture-only-key", "Nonblank provider credential omitted")
        draft.key = ""
        draft.protocolName = "azure_openai"; draft.apiVersion = ""
        try rejects("Azure empty version accepted") { _ = try draft.value(previous: original) }
        draft.apiVersion = "   "
        try rejects("Azure blank version accepted") { _ = try draft.value(previous: original) }
        draft.apiVersion = "2025-01-01"; let azure = try draft.value(previous: original)
        try check(object(azure["connection"])["api_version"] as? String == "2025-01-01", "Azure version omitted")
        draft.protocolName = "openai_responses"
        try check(object(try draft.value(previous: azure))["api_version"] == nil, "Non-Azure stale version survived")
        for endpoint in ["file:///tmp/local", "ftp://fixture.invalid", "https://user:fixture@fixture.invalid", "http://", ""] {
            try check(!SettingsValidation.endpoint(endpoint), "Invalid endpoint accepted")
        }
    }
    static func modelPreservationAndCapabilities() throws {
        let baseline = config(); let original = object(object(baseline["models"])["m"])
        var draft = ModelDraft(id: "m", item: original)
        var refreshed = baseline; var models = object(refreshed["models"]); var latest = original
        var caps = object(latest["capabilities"]); caps["streaming"] = false; caps["temperature"] = false; caps["future_capability"] = "refreshed"
        latest["capabilities"] = caps; models["m"] = latest; refreshed["models"] = models
        let candidate = try draft.candidate(refreshed, editing: true)
        let saved = object(object(candidate["models"])["m"]), savedCaps = object(saved["capabilities"])
        try check(saved["max_rounds"] as? Int == 7 && saved["max_seconds"] as? Int == 90 && saved["future_model"] as? String == "keep", "Model hidden fields lost")
        try check(savedCaps["streaming"] as? Bool == false && savedCaps["future_capability"] as? String == "refreshed", "Untouched capabilities did not use refreshed hidden fields")
        try check(saved["read_only"] as? Bool == true, "An existing read-only model became writable")
        for key in ["future_root", "terminal_reading", "mcp", "skills", "providers", "bindings"] {
            try check(same(candidate[key]!, refreshed[key]!), "Unrelated configuration changed")
        }
        // Selecting the same ID still owns the whole new capabilities declaration.
        draft.select(["id": "same-model", "context_window": 64000, "max_output_tokens": 2048,
                      "capabilities": ["tools": false, "vision": false, "streaming": true, "temperature": true,
                                       "top_p": false, "future_catalog": "chosen", "reasoning_levels": []]])
        let selected = object(object(try draft.candidate(refreshed, editing: true)["models"])["m"])
        let selectedCaps = object(selected["capabilities"])
        try check(selectedCaps["streaming"] as? Bool == true && selectedCaps["temperature"] as? Bool == true, "Same-ID catalog capabilities overwritten by snapshot")
        try check(selectedCaps["future_catalog"] as? String == "chosen" && selectedCaps["future_capability"] == nil, "Catalog declaration merged stale unknown fields")
        try check(selected["context_window"] as? Int == 64000 && selected["max_tokens"] as? Int == 2048, "Catalog token limits not applied")
        try check(object(selected["reasoning"])["mode"] as? String == "provider_default" && selected["read_only"] as? Bool == true, "Catalog reasoning/read-only reset failed")
        draft.model = "new-model"; draft.resetCapabilities()
        let changed = try draft.candidate(baseline, editing: true)
        try check(object(object(changed["bindings"])["global"])["reasoning"] == nil, "Identity change retained obsolete binding reasoning")
        try check(object(object(changed["bindings"])["global"])["future_binding"] as? String == "keep", "Identity change lost unrelated binding fields")
        try check(draft.caps.isEmpty && draft.strength.isEmpty && draft.temperature.isEmpty && draft.mode == "provider_default", "Manual identity reset retained stale capabilities")
    }
    static func modelValidation() throws {
        let baseline = config(), item = object(object(config()["models"])["m"])
        for (context, output) in [("4095", "100"), ("4000001", "100"), ("128000", "0"), ("4096", "2048")] {
            var draft = ModelDraft(id: "m", item: item); draft.context = context; draft.output = output
            try rejects("Invalid token limits accepted") { _ = try draft.candidate(baseline, editing: true) }
        }
        for (temperature, topP) in [("NaN", ""), ("2.1", ""), ("", "0"), ("", "1.1")] {
            var draft = ModelDraft(id: "m", item: item); draft.temperature = temperature; draft.topP = topP
            try rejects("Invalid sampling accepted") { _ = try draft.candidate(baseline, editing: true) }
        }
        try SettingsValidation.reasoning("gemini", ["mode": "budget", "tokens": 128], ["reasoning_budget": [128, 1024]], 2048, false)
        try rejects("Anthropic budget with sampling accepted") { try SettingsValidation.reasoning("anthropic", ["mode": "budget", "tokens": 128], ["reasoning_budget": [128, 1024]], 2048, true) }
        try rejects("Budget equal to output accepted") { try SettingsValidation.reasoning("gemini", ["mode": "budget", "tokens": 1024], ["reasoning_budget": [128, 1024]], 1024, false) }
        try rejects("Undeclared reasoning level accepted") { try SettingsValidation.reasoning("openai_responses", ["mode": "level", "level": "absent"], ["reasoning_levels": ["low"]], 2048, false) }
        try rejects("Unsupported adaptive protocol accepted") { try SettingsValidation.reasoning("openai_chat", ["mode": "adaptive"], ["reasoning_adaptive": true], 2048, false) }
    }
    static func extensionValidation() throws {
        try SettingsValidation.mcp("user/test", ["command": "fixture", "args": ["one"], "envSecretRefs": ["TOKEN": "vault/token"], "future_mcp": 1])
        try SettingsValidation.mcp("http", ["url": "https://fixture.invalid/mcp", "headers": ["X-Fixture": "value"], "headerSecretRefs": ["Authorization": "vault/token"]])
        for value: [String: Any] in [["command": "fixture", "url": "https://fixture.invalid"], ["command": ""], ["url": "file:///tmp/mcp"], ["command": "fixture", "startup_timeout_ms": 99], ["command": "fixture", "call_timeout_ms": 300001], ["command": "fixture", "env": ["BAD=NAME": "value"]], ["command": "fixture", "args": Array(repeating: "x", count: 129)]] {
            try rejects("Invalid MCP accepted") { try SettingsValidation.mcp("test", value) }
        }
        try rejects("Builtin MCP accepted") { try SettingsValidation.mcp("builtin/terminal", ["command": "fixture"]) }
        try rejects("Skill namespace duplicate accepted") { try SettingsValidation.id("existing", kind: "skills", editing: false, items: ["user/existing": [:]]) }
        for path in ["/Desktop/fixture", "C:\\Desktop\\fixture", "D:/Desktop/fixture", "\\\\server\\share\\fixture"] {
            try check(SettingsValidation.absolutePath(path), "Valid Desktop path rejected")
        }
        for path in ["", "relative/path", "C:relative", "\\\\server", "/bad\0path"] {
            try check(!SettingsValidation.absolutePath(path), "Invalid Desktop path accepted")
        }
    }
    static func skillFiles() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("aterminal-skill-check-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root.appendingPathComponent("resources"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try Data("# Fixture\n".utf8).write(to: root.appendingPathComponent("SKILL.md"))
        try Data([0, 255, 128, 1]).write(to: root.appendingPathComponent("resources/binary.bin"))
        try Data().write(to: root.appendingPathComponent("resources/empty"))
        let files = try SettingsValidation.skillFiles(root)
        try check(Set(files.map { $0.0 }) == Set(["SKILL.md", "resources/binary.bin", "resources/empty"]), "Full Skill resource enumeration failed")
        try check(try Data(contentsOf: files.first { $0.0 == "resources/binary.bin" }!.1) == Data([0, 255, 128, 1]), "Skill binary bytes changed")
        let link = root.appendingPathComponent("resources/link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: root.appendingPathComponent("SKILL.md"))
        try rejects("Skill symlink accepted") { _ = try SettingsValidation.skillFiles(root) }
        try FileManager.default.removeItem(at: link)
        try Data(repeating: 0, count: 2097153).write(to: root.appendingPathComponent("large"))
        try rejects("Oversized Skill file accepted") { _ = try SettingsValidation.skillFiles(root) }
        try FileManager.default.removeItem(at: root.appendingPathComponent("large"))
        try FileManager.default.removeItem(at: root.appendingPathComponent("SKILL.md"))
        try rejects("Skill without root SKILL.md accepted") { _ = try SettingsValidation.skillFiles(root) }
    }
    @MainActor static func sessionSingleFlightSnapshotAndSecrets() async throws {
        let transport = ControlledTransport(); let session = ConfigurationSession(identity: { "fixture:desktop:epoch1" }, transport: transport.call)
        let load = session.load()!; _ = await transport.next(); transport.resolve(["revision": 7, "config": config()]); await load.value
        let save = session.save(config())!; let request = await transport.next()
        try check(request["action"] as? String == "replace" && request["expected_revision"] as? Int == 7, "Replace revision/action incorrect")
        try check(object(request["secrets"]).isEmpty, "Empty-key replace sent credential content")
        try check(session.save(config()) == nil && session.busy, "Busy session allowed a duplicate mutation")
        transport.resolve(["revision": 8, "config": ["returned_snapshot": true]]); await save.value
        try check(session.snapshot["revision"] as? Int == 8 && object(session.snapshot["config"])["returned_snapshot"] as? Bool == true, "Mutation did not adopt returned snapshot")
        try check(transport.commands.count == 2 && !session.busy, "Save unexpectedly reloaded or left busy")
        let secret = "fixture-secret-value"; let failure = session.save(config(), secrets: ["p": secret])!
        _ = await transport.next(); transport.fail("fixture rejection " + secret); await failure.value
        try check(!session.error.contains(secret) && !session.busy && session.snapshot["revision"] as? Int == 8, "Failed save leaked credential or mutated snapshot")
    }
    @MainActor static func conflictRequiresExplicitRetry() async throws {
        let transport = ControlledTransport(); let session = ConfigurationSession(identity: { "fixture" }, transport: transport.call)
        let load = session.load()!; _ = await transport.next(); transport.resolve(["revision": 3, "config": config()]); await load.value
        let draft = config(); var completed = false
        let save = session.save(draft) { completed = true }!; _ = await transport.next(); transport.fail("configuration_revision_conflict")
        let refresh = await transport.next(); try check(refresh["action"] as? String == "show", "Conflict did not refresh")
        transport.resolve(["revision": 4, "config": config()]); await save.value
        try check(!completed && !session.busy && transport.commands.count == 3, "Conflict silently retried or completed save")
        try check(same(draft, config()), "Failed save mutated caller draft")
        let retry = session.save(draft)!; let request = await transport.next()
        try check(request["expected_revision"] as? Int == 4, "Explicit retry used stale revision")
        transport.resolve(["revision": 5, "config": draft]); await retry.value
    }
    @MainActor static func closedAndReconnectedLateResults() async throws {
        for fails in [false, true] {
            let transport = ControlledTransport(); let session = ConfigurationSession(identity: { "fixture" }, transport: transport.call)
            var completed = false; let request = session.save(config()) { completed = true }!; _ = await transport.next()
            session.close()
            if fails { transport.fail("late-fixture-error") } else { transport.resolve(["revision": 9, "config": config()]) }
            await request.value
            try check(!completed && session.snapshot.isEmpty && session.error.isEmpty && !session.busy, "Closed form consumed a late response")
        }
        for fails in [false, true] {
            var identity = "fixture:desktop:epoch1"
            let transport = ControlledTransport(); let session = ConfigurationSession(identity: { identity }, transport: transport.call)
            var completed = false; let request = session.save(config()) { completed = true }!; _ = await transport.next()
            identity = "fixture:desktop:epoch2"
            if fails { transport.fail("late-fixture-error") } else { transport.resolve(["revision": 10, "config": config()]) }
            await request.value
            try check(!completed && session.snapshot.isEmpty && !session.busy, "Reconnect consumed an old mutation")
            try check(!session.error.contains("late-fixture-error"), "Reconnect consumed an old failure message")
            try check(session.load() == nil && transport.commands.count == 1, "Old connection coordinator issued a new RPC")
        }
    }
    @MainActor static func conflictRefreshReconnectBarrier() async throws {
        var identity = "fixture:epoch1"
        let transport = ControlledTransport(); let session = ConfigurationSession(identity: { identity }, transport: transport.call)
        let load = session.load()!; _ = await transport.next(); transport.resolve(["revision": 1, "config": config()]); await load.value
        let save = session.save(config())!; _ = await transport.next(); transport.fail("configuration_revision_conflict")
        _ = await transport.next(); identity = "fixture:epoch2"
        transport.resolve(["revision": 99, "config": ["wrong_connection": true]]); await save.value
        try check(session.snapshot["revision"] as? Int == 1 && !session.busy, "Late conflict refresh updated snapshot")
        try check(!session.error.contains("配置已变化"), "Old conflict state shown after reconnect")
    }
    @MainActor static func cancelledReadCannotOverwriteNewOperation() async throws {
        for fails in [false, true] {
            let transport = ControlledTransport(); let session = ConfigurationSession(identity: { "fixture" }, transport: transport.call)
            var oldCompleted = false, newCompleted = false
            let old = session.run(operation: { try await session.call(["action": "discover", "search": "old-query"]) }) { _ in oldCompleted = true }!
            _ = await transport.next(); session.cancelRead()
            try check(!session.busy, "Cancelled catalog read left busy")
            let new = session.run(operation: { try await session.call(["action": "discover", "search": "new-query"]) }) { _ in newCompleted = true }!
            _ = await transport.next()
            if fails { transport.fail("late-catalog-error") } else { transport.resolve(["models": [["id": "old-model"]]]) }
            await old.value
            try check(!oldCompleted && session.busy && session.error.isEmpty, "Cancelled catalog read consumed callback or disturbed new operation")
            transport.resolve(["models": [["id": "new-model"]]]); await new.value
            try check(newCompleted && !session.busy && transport.commands.count == 2, "New read after cancellation did not complete independently")
        }
    }
    @MainActor static func main() async {
        let checks: [(String, () async throws -> Void)] = [
            ("provider hidden fields, Azure and endpoints", { try providerContracts() }),
            ("model hidden fields, same-ID catalog capabilities and bindings", { try modelPreservationAndCapabilities() }),
            ("token, sampling and reasoning validation", { try modelValidation() }),
            ("MCP and Desktop Skill path contracts", { try extensionValidation() }),
            ("full Skill files and package rejection", { try skillFiles() }),
            ("single-flight, returned snapshot and credential redaction", { try await sessionSingleFlightSnapshotAndSecrets() }),
            ("conflict refresh and explicit retry", { try await conflictRequiresExplicitRetry() }),
            ("closed/reconnected late response barriers", { try await closedAndReconnectedLateResults() }),
            ("conflict-refresh reconnect barrier", { try await conflictRefreshReconnectBarrier() }),
            ("catalog cancel/read serial barrier", { try await cancelledReadCannotOverwriteNewOperation() })
        ]
        var failures = 0
        for (name, run) in checks {
            do { try await run(); print("PASS: " + name) }
            catch { failures += 1; print("FAIL: " + name + " — " + ((error as? CheckFailure)?.message ?? "unexpected fixture-only operation failure")) }
        }
        if failures > 0 { exit(1) }
    }
}
