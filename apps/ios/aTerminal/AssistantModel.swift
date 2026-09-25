import SwiftUI

@MainActor
final class AssistantModel: ObservableObject {
    @Published private(set) var archives: [ChatArchive] = []
    @Published private(set) var current: ChatArchive?
    @Published private(set) var available = false
    @Published private(set) var requesting = false
    @Published var status = "选择已连接的终端后使用 AI"
    @Published var draft = ""
    @Published var allowInput = true
    @Published var monitor = true
    @Published var storageError: String?
    private var identity: ChatIdentity?
    private var scope: ChatScope?
    private var connected = false
    private var visible = false
    private var epoch = 0
    private var task: Task<Void, Never>?
    private var store = ChatStore()
    private weak var terminal: TerminalModel?
    var monitoring: Bool { current?.state == "monitoring" }
    var optionsFrozen: Bool { requesting && !monitoring }
    var canCancel: Bool { connected && current?.pendingID != nil && current?.state != "stopping" }
    var canSend: Bool { connected && available && (!requesting || monitoring) && storageError == nil && (current?.pendingID == nil || monitoring) && scope != nil && !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    func context(identity: ChatIdentity?, scope: ChatScope?, title: String, device: String, connected: Bool, core: RemoteTerminal, terminal: TerminalModel) {
        self.terminal = terminal
        guard identity != self.identity || scope != self.scope || connected != self.connected else { return }
        stop()
        if identity != self.identity {
            archives = []; current = nil; draft = ""; allowInput = true; monitor = true; storageError = nil
            if let identity { do { archives = try store.load(identity) } catch { storageError = "无法读取本地对话：\(error.localizedDescription)" } }
        }
        if scope != self.scope { draft = ""; allowInput = true; monitor = true }
        self.identity = identity; self.scope = scope; self.connected = connected; available = false
        current = scope.map { scope in archives.first { $0.scope == scope } ?? ChatArchive(scope: scope, title: title, deviceName: device) }
        status = "AI 助手即将开放"
    }
    func setVisible(_ value: Bool, core: RemoteTerminal) {
        visible = value
        stop()
    }
    func stop() { epoch += 1; task?.cancel(); task = nil; requesting = false }
    @discardableResult private func persist() -> Bool {
        guard let current else { return false }
        do {
            try store.save(current)
            archives.removeAll { $0.id == current.id }; archives.insert(current, at: 0)
            return true
        } catch { storageError = "无法保存本地对话：\(error.localizedDescription)"; return false }
    }
    private func request(_ core: RemoteTerminal, scope: ChatScope, json: String) async throws -> AssistantResponse {
        guard let terminal, !Task.isCancelled else { throw CancellationError() }
        return try await terminal.assistant(scope: scope, json: json)
    }
    func refresh(_ core: RemoteTerminal) {
        guard connected, let scope, !requesting else { return }
        let version = epoch
        requesting = true
        task = Task { [weak self] in
            guard let self else { return }
            guard self.epoch == version, !Task.isCancelled else { return }
            do {
                if let pending = self.current?.pendingID {
                    try await self.poll(core, scope: scope, id: pending, version: version)
                } else {
                    let response = try await self.request(core, scope: scope, json: ChatRequest.encode(action: "status"))
                    guard self.epoch == version, !Task.isCancelled else { return }
                    self.available = response.available
                    self.status = response.message.isEmpty ? (response.available ? "AI 已就绪" : "Desktop 尚未配置 AI") : response.message
                    if !response.request_id.isEmpty, ["running", "monitoring", "stopping"].contains(response.state) {
                        self.current?.pendingID = response.request_id
                        try self.apply(response, id: response.request_id)
                        try await self.poll(core, scope: scope, id: response.request_id, version: version)
                    }
                }
            } catch { if self.epoch == version, !Task.isCancelled { self.status = "无法查询 AI：\(terminalError(error))" } }
            if self.epoch == version { self.requesting = false }
        }
    }
    func send(_ core: RemoteTerminal) {
        guard canSend, let scope, let previous = current else { return }
        let message = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        let id = UUID().uuidString
        let json: String
        do { json = try ChatRequest.encode(action: "send", requestID: id, message: message, allowInput: allowInput, monitor: monitor, history: previous.messages) }
        catch { status = error.localizedDescription; return }
        current?.messages.append(ChatMessage(role: "user", content: message))
        current?.pendingID = id; current?.state = "running"; current?.updated = Date(); current?.status = "正在请求模型"
        guard persist() else { current = previous; status = "消息未发送，本地保存失败"; return }
        stop()
        draft = ""; status = "正在请求模型"; requesting = true
        let version = epoch
        task = Task { [weak self] in
            guard let self else { return }
            guard self.epoch == version, !Task.isCancelled else { return }
            do {
                let response = try await self.request(core, scope: scope, json: json)
                guard self.epoch == version, !Task.isCancelled else { return }
                try self.apply(response, id: id)
                if self.current?.pendingID != nil { try await self.poll(core, scope: scope, id: id, version: version) }
            } catch {
                guard self.epoch == version, !Task.isCancelled else { return }
                self.status = "结果未知：\(terminalError(error))。可查询状态。"
                self.current?.state = "unknown"; self.current?.status = self.status; self.persist()
            }
            if self.epoch == version { self.requesting = false }
        }
    }
    func cancel(_ core: RemoteTerminal) {
        guard canCancel, let scope, let id = current?.pendingID else { return }
        stop(); requesting = true; current?.state = "stopping"; status = "正在停止 AI 输入与监控"; current?.status = status; persist()
        let version = epoch
        task = Task { [weak self] in
            guard let self, self.epoch == version, !Task.isCancelled else { return }
            do {
                let response = try await self.request(core, scope: scope, json: ChatRequest.encode(action: "cancel", requestID: id))
                guard self.epoch == version, !Task.isCancelled else { return }
                try self.apply(response, id: id)
                if self.current?.pendingID != nil { try await self.poll(core, scope: scope, id: id, version: version) }
            } catch {
                guard self.epoch == version, !Task.isCancelled else { return }
                self.status = "停止结果未知：\(terminalError(error))"; self.current?.state = "unknown"; self.current?.status = self.status; self.persist()
            }
            if self.epoch == version { self.requesting = false }
        }
    }
    private func poll(_ core: RemoteTerminal, scope: ChatScope, id: String, version: Int) async throws {
        while !Task.isCancelled && epoch == version {
            let response = try await request(core, scope: scope, json: ChatRequest.encode(action: "poll", requestID: id))
            guard epoch == version, !Task.isCancelled else { return }
            try apply(response, id: id)
            if current?.pendingID == nil { return }
            try await Task.sleep(nanoseconds: 1_000_000_000)
        }
    }
    private func apply(_ response: AssistantResponse, id: String) throws {
        guard response.request_id.isEmpty || response.request_id == id else { throw ChatFailure.message("AI 返回了不匹配的请求标识") }
        available = response.available
        if let events = response.events {
            current?.appendEvents(events, requestID: id)
        } else if response.state == "completed", !response.reply.isEmpty, current?.messages.contains(where: { $0.id == id + ":reply" }) == false {
            current?.messages.append(ChatMessage(id: id + ":reply", role: "assistant", content: response.reply, requestID: id))
        }
        switch response.state {
        case "completed":
            current?.pendingID = nil; status = response.message.isEmpty ? "AI 本次任务已结束" : response.message
        case "failed", "unavailable":
            current?.pendingID = nil; status = response.message.isEmpty ? "AI 暂不可用" : response.message
        case "stopped": current?.pendingID = nil; status = response.message.isEmpty ? "监控已停止" : response.message
        case "monitoring": status = response.message.isEmpty ? "正在监控终端" : response.message
        case "stopping": status = response.message.isEmpty ? "正在停止监控" : response.message
        case "running": status = response.message.isEmpty ? "模型正在处理" : response.message
        default: throw ChatFailure.message(response.message.isEmpty ? "请求状态未知" : response.message)
        }
        current?.state = response.state; current?.status = status; current?.updated = Date(); persist()
    }
}
