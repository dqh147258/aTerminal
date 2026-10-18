import SwiftUI
import UniformTypeIdentifiers

struct AgentItem: Identifiable {
    let id: String
    let kind: String
    let text: String
    let records: [String]
    let terminalStatusKey: String?
    let sequence: Int64
    let reply: Bool
    let partial: Bool
    let interrupted: Bool
    init(_ object: [String: Any]) {
        sequence = (object["sequence"] as? NSNumber)?.int64Value ?? 0
        id = object["id"] as? String ?? UUID().uuidString
        let eventKind = object["kind"] as? String ?? "记录"
        let value = object["value"] as? [String: Any] ?? [:]
        interrupted = AgentMessagePresentation.interrupted(kind: eventKind, value: value)
        kind = interrupted ? "回复中断" : ["user":"你","assistant":"AI Agent","interaction":"操作与证据"][eventKind] ?? "状态"
        reply = ["assistant", "interaction"].contains(eventKind); partial = value["partial"] as? Bool ?? false
        terminalStatusKey = eventKind.hasPrefix("pty_status") ? (value["session_id"] as? String ?? "") : nil
        let summary = (value["updates"] as? [[String:Any]])?.reversed().compactMap{$0["summary"] as? String}.first
        let fallback = interrupted ? "回复已中断，已收到的内容仍保留。" : eventKind.hasPrefix("pty_status") ? "终端状态："+((value["session_process"] as? [String:Any])?["state"] as? String ?? value["state"] as? String ?? "已更新") : (eventKind=="interaction" ? "终端或扩展操作记录，可查看关联证据。" : "Agent 状态："+(value["state"] as? String ?? "已更新"))
        text = value["message"] as? String ?? value["text"] as? String ?? value["summary"] as? String ?? summary ?? fallback
        var ids = Set<String>()
        func collect(_ node: Any) {
            if let dict = node as? [String: Any] { for (key, value) in dict { if key == "record_id" || key.hasSuffix("_record_id"), let id = value as? String { ids.insert(id) } else { collect(value) } } }
            if let array = node as? [Any] { array.forEach { collect($0) } }
        }
        collect(value); records = ids.sorted()
    }
}

@MainActor final class AssistantModel: ObservableObject {
    @Published private(set) var archives: [ChatArchive] = []
    @Published private(set) var items: [AgentItem] = []
    @Published var status = "连接 Desktop 后使用 AI"
    @Published var draft = "" { didSet { if !restoringDraft && oldValue != draft { requestID = UUID().uuidString; scheduleDraft() } } }
    @Published var attachments: [AgentAttachment] = []
    @Published var submitting = false
    @Published private(set) var pendingDelivery: AgentSendSnapshot?
    @Published private(set) var deliveryError = ""
    @Published var globalRows: [[String: Any]] = []
    @Published var globalLoading = false
    @Published var globalCreating = false
    @Published var globalError = ""
    @Published var globalTitle = "新会话"
    @Published var globalID: String?
    private var restoringDraft = false
    private var requestID = UUID().uuidString
    private var draftScope: ChatScope?
    private var sending = Set<String>()
    private var sendingMessages = Set<String>()
    private let composerStore = AgentComposerStore(root: WorkspacePreferences.historyDirectory)
    #if DEBUG
    private var fixtureSendAttempts = 0
    private var fixtureHandoffStateReads = 0
    private var fixtureHandoffHistoryReads = 0
    #endif
    private var globalConfirmed = false
    private var globalEpoch = 0
    private var contextConnection = -1
    private var archiveLoading = false
    let authorization = AgentAuthorization()
    let interactions = AgentInteractionState()
    @Published var authorizationVisible = false
    @Published var global = false
    @Published var browsing = false
    @Published var loading = false
    @Published var hasMore = false
    @Published var available = false
    @Published var running = false
    @Published private(set) var modelRetryStatus: String?
    @Published var liveText = ""
    private var liveReply = AgentLiveReplyState()
    var liveAwaitingHistory: Bool { liveReply.awaitingHistory }
    @Published var evidence = ""
    @Published var image: UIImage?
    @Published var evidenceVisible = false
    @Published var settingsVisible = false
    @Published private(set) var historyCacheWarning = ""
    private var scope: ChatScope?
    private var historyTarget:ChatScope?
    private var connected = false
    private var desktopName=""
    var contextLabel: String {
        if global { return desktopName }
        guard let target else { return "未选择终端" }
        if target.device == terminal?.deviceID, let session = terminal?.sessions.first(where: { $0.id == target.session }) { return session.cwd }
        return archives.first(where: { $0.scope == target })?.workingDirectory ?? "离线历史"
    }
    var destinationLabel:String {(scope?.identity.account ?? "")+" · "+desktopName}
    private var visible = false
    private var epoch = 0
    private var authorizationEpoch = 0
    private var presentationEpoch = 0
    private var generation: Int64?
    private var historyGeneration = AgentHistoryGenerationFence()
    private var cursor: String?
    private var pages: [[AgentItem]] = []
    private var task: Task<Void, Never>?
    private weak var terminal: TerminalModel?
    private var cache: AgentCache?
    private let historyCache: AgentHistoryCache
    private let historySearch: AgentSearchSession
    @Published private(set) var historyCacheRevision = 0
    var historySearchBusy: Bool { historySearch.busy }
    var historySearchError: String { historySearch.error }
    func searchCachedHistory(query: String, scopes: [ChatScope], refresh: Bool = false) {
        guard let identity = scope?.identity else { historySearch.cancel(); return }
        let allowed = Set(scopes.filter { $0.identity == identity && !$0.session.isEmpty && !$0.session.hasPrefix("global:") }.map(\.key))
        historySearch.update(identity: identity.key, query: query, scopes: allowed, refresh: refresh)
    }
    func cachedHistoryMatches(_ scope: ChatScope, query: String) -> Bool {
        guard scope.identity == self.scope?.identity else { return false }
        return historySearch.contains(scope: scope.key, identity: scope.identity.key, query: query)
    }
    func cancelHistorySearch() { historySearch.cancel() }
    private var canSubmitMessage: Bool { authorization.permissions?.canMutate == true && !authorization.busy && agentConnectionReason == nil && connected && available && !submitting && (global ? globalID != nil : !(target?.session ?? "").isEmpty) }
    var canSend: Bool { canSubmitMessage && pendingDelivery == nil && (!draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty) }
    var canRetrySend: Bool { canSubmitMessage && pendingDelivery?.scopeKey == target?.key && pendingDelivery != nil }
    var isSendingMessage: Bool { target.map { sendingMessages.contains($0.key) } ?? false }
    var canCancel: Bool { authorization.permissions?.canMutate == true && agentConnectionReason == nil && connected && running }
    // Agent management uses the server device grant, independently of PTY control/attachment.
    var agentConnectionReason: String? {
        guard connected, let target else { return "Desktop 未连接 · 只读缓存" }
        if WorkspacePreferences.fixture { return target.device == "fixture-desktop" ? nil : "设备离线 · 只读缓存" }
        guard let terminal, terminal.identity == target.identity, terminal.deviceID == target.device else { return "设备离线 · 只读缓存" }
        return nil
    }
    var writeReason: String? {
        guard connected, let target else { return "Desktop 未连接 · 只读缓存" }
        if WorkspacePreferences.fixture {
            if global { return nil }
            if target.device != "fixture-desktop" { return "设备离线 · 只读缓存" }
            return target.session == "fixture-session" ? nil : "会话已关闭 · 只读"
        }
        guard let terminal, terminal.identity == target.identity, terminal.deviceID == target.device else { return "设备离线 · 只读缓存" }
        if global { return nil }
        guard let session = terminal.sessions.first(where: { $0.id == target.session }) else { return "会话已关闭 · 只读" }
        if session.exited || (session.id == terminal.selected && terminal.sessionExited) { return "会话已结束 · 只读" }
        if !session.desktopAttached || (session.id == terminal.selected && !terminal.desktopAttached) { return "Desktop 已离开 · 只读" }
        return nil
    }
    var target: ChatScope? { (historyTarget ?? scope).map { ChatScope(identity: $0.identity, device: $0.device, session: global ? globalID.map { "global:" + $0 } ?? "" : $0.session) } }
    init() {
        let path = WorkspacePreferences.historyDirectory.appendingPathComponent("agent-cache.sqlite3").path
        historySearch = AgentSearchSession { scopes, query in
            let worker = Task.detached(priority: .userInitiated) { try AgentCacheSearch.matches(path: path, scopes: scopes, query: query) }
            return try await withTaskCancellationHandler(operation: { try await worker.value }, onCancel: { worker.cancel() })
        }
        let pagesCache: AgentCache?
        var cacheFailure: String?
        do {
            let root = WorkspacePreferences.historyDirectory
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            pagesCache = try AgentCache.open(path: path)
        } catch { pagesCache = nil; cacheFailure = error.localizedDescription }
        historyCache = AgentHistoryCache(write: { scope, cursor, page in
            guard let pagesCache else { throw AgentMessageCache.Failure.unavailable }
            try pagesCache.storePage(scope: scope, cursor: cursor, page: page)
        }, read: { scope, cursor in try pagesCache?.page(scope: scope, cursor: cursor) }, messages: try? AgentMessageCache(path: path))
        cache = pagesCache
        interactions.changed = { [weak self] in self?.objectWillChange.send() }
        authorization.changed = { [weak self] in
            guard let self else { return }
            if let key = self.target?.key, self.authorization.permissions != nil,
               !self.authorization.loading, !self.authorization.busy, self.authorization.error.isEmpty {
                self.interactions.reconcile(scopeKey: key, pending: self.authorization.pending)
            }
            self.objectWillChange.send()
        }
        historySearch.changed = { [weak self] in self?.objectWillChange.send() }
        if let cacheFailure { status = "历史缓存不可用：\(cacheFailure)" }
    }
    func context(identity: ChatIdentity?, scope: ChatScope?, title: String, device: String, connected: Bool, core: RemoteTerminal, terminal: TerminalModel) {
        self.terminal = terminal;desktopName=device
        let connected = connected || (WorkspacePreferences.fixture && identity != nil)
        let connectionChanged = contextConnection != terminal.generation
        guard self.scope != scope || self.connected != connected || connectionChanged else { return }
        contextConnection = terminal.generation
        if connectionChanged || self.connected != connected { globalEpoch += 1; globalLoading = false; globalCreating = false; globalConfirmed = false; archiveLoading = false }
        let changedDesktop = self.scope?.identity != scope?.identity || self.scope?.device != scope?.device
        if self.scope?.identity != identity { historySearch.cancel() }
        if self.scope == scope {
            // The page, history cursor, global agent, open question and draft belong
            // to this scope, not the transport. Reconnect only replaces its RPC epoch.
            stop(preservingContext: true); self.connected = connected; available = false
            bindAuthorization()
            if connected { loadArchives(); if visible { start() } }
            else { status = "连接暂时中断 · 当前页面和草稿已保留" }
            return
        }
        saveDraft(); stop(); self.scope = scope; historyTarget=nil; self.connected = connected; available = false; historyCacheWarning = ""
        if let identity, let data = WorkspacePreferences.defaults.data(forKey: "agent.archives." + identity.key) { archives = (try? JSONDecoder().decode([ChatArchive].self, from: data)) ?? [] }
        else { archives = [] }
        if changedDesktop { globalID = nil; global = false; globalEpoch += 1; globalLoading = false; globalCreating = false; globalRows = []; restoreGlobals() }
        restoreDraft()
        if let scope, !scope.session.isEmpty {
            if !archives.contains(where: { $0.scope == scope }) { archives.append(ChatArchive(scope: scope, title: title, deviceName: device)) }
            saveArchives()
        }
        #if DEBUG
        if SettingsFixture.enabled, let identity {
            let examples = [ChatArchive(scope: ChatScope(identity: identity, device: "fixture-desktop", session: "fixture-closed"), title: "已关闭会话", deviceName: "Fixture Desktop", cwd: "/fixture/closed/workspace"),
                            ChatArchive(scope: ChatScope(identity: identity, device: "fixture-offline-desktop", session: "fixture-offline"), title: "离线历史", deviceName: "Offline Desktop", cwd: "/fixture/offline/workspace")]
            for archive in examples {
                if let index = archives.firstIndex(where: { $0.scope == archive.scope }) { archives[index].updateDirectory(archive.workingDirectory, title: archive.title, deviceName: archive.deviceName) }
                else { archives.append(archive) }
            }
            saveArchives()
            let closed = ChatScope(identity: identity, device: "fixture-desktop", session: "fixture-closed")
            let page: [String: Any] = ["generation": 1, "has_more": false, "items": [["id": "fixture-body-search", "sequence": 1, "kind": "assistant", "value": ["text": "正文专有词 星河缓存检索 cedar-body-only-731"]]]]
            if let bytes = try? JSONSerialization.data(withJSONObject: page) { try? cache?.storePage(scope: closed.key, cursor: nil, page: String(decoding: bytes, as: UTF8.self)) }
        }
        #endif
        if connected {loadArchives()}
        reset(); if visible { start() }
    }
    func setVisible(_ value: Bool, core: RemoteTerminal) { visible = value; stop(); if value { reset(); start() } }
    func stop(preservingContext: Bool = false) {
        saveDraft()
        if !preservingContext {
            presentationEpoch += 1
            interactions.bind(scopeKey: nil, context: UUID().uuidString)
        }
        authorizationEpoch += 1
        authorization.bind(context: UUID().uuidString, scopeKey: target?.key, writable: false, transport: nil)
        epoch += 1; task?.cancel(); task = nil; loading = false
    }
    func switchScope() { historyTarget=nil; restoreDraft(); stop(); reset(); if visible { start() } }
    func openSession() { saveDraft(); global = false; globalID = nil; browsing = false; switchScope() }
    func openGlobal(_ row: [String: Any]) {
        guard let id = (row["scope"] as? [String: Any])?["agent"] as? String else { return }
        saveDraft(); globalID = id; global = true; globalTitle = row["title"] as? String ?? "新会话"; browsing = false; switchScope()
    }
    func reset() { epoch += 1; loading = false; generation = nil; historyGeneration.bind(target?.key); cursor = nil; pages = []; items = []; liveReply = AgentLiveReplyState(); liveText = ""; modelRetryStatus = nil; hasMore = true; bindAuthorization(); load(first: true) }
    func start() {
        task?.cancel()
        task = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refresh()
                try? await Task.sleep(nanoseconds: 1_500_000_000)
            }
        }
    }
    func request(_ command: [String: Any], configuration: Bool = false, destination: ChatScope? = nil, expectedEpoch: Int? = nil) async throws -> [String: Any] {
        guard let destination = destination ?? target else { throw ChatFailure.message("Desktop 未连接") }
        let connection = expectedEpoch ?? connectionEpoch
        return try await AgentReadRetry.run(action: command["action"] as? String ?? "", configuration: configuration,
            valid: { self.connectionEpoch == connection && self.connected }, message: terminalError) {
                try await self.requestOnce(command, configuration: configuration, destination: destination, connection: connection)
            }
    }
    private func requestOnce(_ command: [String: Any], configuration: Bool, destination: ChatScope, connection: Int) async throws -> [String: Any] {
        try Task.checkCancellation()
        guard connection == connectionEpoch else { throw CancellationError() }
        #if DEBUG
        if SettingsFixture.enabled {
            if ProcessInfo.processInfo.arguments.contains("--chat-handoff-fixture") {
                let reply = "这段已收到的回复在历史加载完成前仍可阅读。"
                if command["action"] as? String == "state" {
                    fixtureHandoffStateReads += 1
                    return ["available": true, "history_generation": 1, "state": fixtureHandoffStateReads == 1 ? "running" : "completed", "run_id": "fixture-handoff-run", "root_user_message_id": "fixture-handoff-root", "live_text": fixtureHandoffStateReads == 1 ? reply : ""]
                }
                if command["action"] as? String == "pending" { return ["items": [], "has_more": false, "cursor": NSNull()] }
                if command["action"] as? String == "history" {
                    fixtureHandoffHistoryReads += 1
                    if fixtureHandoffHistoryReads == 1, ProcessInfo.processInfo.arguments.contains("--chat-stale-history-fixture") {
                        try await Task.sleep(nanoseconds: 4_000_000_000)
                        return ["generation": 0, "has_more": false, "items": [["id": "stale-history", "sequence": 1, "kind": "assistant", "value": ["text": "这条过期历史不能重新出现。"]]]]
                    }
                    if fixtureHandoffStateReads < 2 { return ["generation": 1, "has_more": false, "items": []] }
                    try await Task.sleep(nanoseconds: 6_000_000_000)
                    return ["generation": 1, "has_more": false, "items": [["id": "fixture-handoff-message", "sequence": 1, "kind": "assistant", "root_user_message_id": "fixture-handoff-root", "value": ["run_id": "fixture-handoff-run", "text": reply]]]]
                }
            }
            if command["action"] as? String == "permissions", ProcessInfo.processInfo.arguments.contains("--chat-slow-permissions-fixture") {
                try await Task.sleep(nanoseconds: 1_200_000_000)
            }
            if command["action"] as? String == "send", ProcessInfo.processInfo.arguments.contains("--chat-delivery-fixture") {
                fixtureSendAttempts += 1
                try await Task.sleep(nanoseconds: 6_000_000_000)
                if fixtureSendAttempts == 1, ProcessInfo.processInfo.arguments.contains("--chat-send-failure-fixture") { throw ChatFailure.message("测试发送响应丢失") }
            }
            if command["action"] as? String == "history", ProcessInfo.processInfo.arguments.contains("--chat-interrupted-fixture") {
                return ["generation": 1, "has_more": false, "items": [["id": "fixture-interrupted", "sequence": 1, "kind": "interaction", "value": ["source": "model_interrupted", "interrupted": true, "state": "interrupted", "text": "已收到的回复内容仍可阅读。"]]]]
            }
            if AuthorizationFixture.enabled, command["action"] as? String == "approval_details" { try await Task.sleep(nanoseconds: 1_000_000_000) }
            if AuthorizationFixture.enabled, let value = AuthorizationFixture.shared.request(command, scope: destination.session) { return value }
            return SettingsFixture.agent(command, session: destination.session)
        }
        #endif
        guard let terminal else { throw ChatFailure.message("Desktop 未连接") }
        var body = command; if !configuration { body["version"] = 1 }
        var session = destination.session
        if session.hasPrefix("global:") { if !configuration { body["agent_id"] = String(session.dropFirst(7)) }; session = "" }
        let json = String(decoding: try JSONSerialization.data(withJSONObject: body), as: UTF8.self)
        let wire = ChatScope(identity: destination.identity, device: destination.device, session: session)
        let text = try await terminal.agent(scope: wire, json: json, configuration: configuration)
        guard connection == connectionEpoch else { throw CancellationError() }
        guard let value = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else { throw ChatFailure.message("无效的 Desktop 响应") }
        return value
    }
    private func bindAuthorization() {
        let destination = target; let connection = connectionEpoch; let version = authorizationEpoch
        let context = "\(destination?.key ?? ""):\(connection):\(version)"
        interactions.bind(scopeKey: destination?.key, context: "\(destination?.key ?? ""):\(presentationEpoch)")
        authorization.bind(context: context, scopeKey: destination?.key, writable: agentConnectionReason == nil && connected,
            transport: connected && destination != nil ? { [weak self] command in
                guard let self, self.target == destination, self.authorizationEpoch == version else { throw CancellationError() }
                let value = try await self.request(command, destination: destination, expectedEpoch: connection)
                guard self.target == destination, self.authorizationEpoch == version else { throw CancellationError() }
                return value
            } : nil)
    }
    func openQuestion(_ item: AgentPending, destination: ChatScope?) {
        guard authorization.canEditAnswer(item), let target, target == destination,
              let current = authorization.pending.first(where: { $0.id == item.id && $0.kind == "question" && $0.actionable }) else { return }
        interactions.open(current, kind: .question, scopeKey: target.key)
    }
    func openApprovalDetails(_ item: AgentPending, destination: ChatScope?) {
        guard let target, target == destination,
              let current = authorization.pending.first(where: { $0.id == item.id && $0.kind == "approval" && $0.fingerprint == item.fingerprint }),
              authorization.canReadDetails(current) else { return }
        interactions.open(current, kind: .details, scopeKey: target.key)
    }
    private func refresh() async {
        guard connected, let target else { return }; let version = epoch
        historyGeneration.bind(target.key)
        do {
            let response = try await request(["action": "state"])
            guard version == epoch, !Task.isCancelled else { return }
            let nextGeneration = (response["history_generation"] as? NSNumber)?.int64Value
            if let nextGeneration, !historyGeneration.accept(nextGeneration, scopeKey: target.key) { return }
            available = response["available"] as? Bool ?? false
            let state = response["state"] as? String ?? "idle"
            running = ["running", "monitoring", "stopping", "finishing", "cancelling", "waiting", "suspended", "waiting_approval", "waiting_user", "waiting_for_user"].contains(state)
            liveReply.receive(response); liveText = liveReply.text
            modelRetryStatus = AgentMessagePresentation.modelRetryStatus(response["model_retry"])
            status = (global ? "全局" : "当前终端") + " · " + (modelRetryStatus ?? state) + (response["error"] as? String).map { " · " + $0 }.orEmpty
            if let next = nextGeneration {
                let advanced = generation.map { next > $0 } ?? false
                if advanced { reset() }
                generation = next
                do { try cache?.reconcile(scope: target.key, generation: next); historyCacheRevision += 1 }
                catch { historyCacheWarning = "离线历史同步失败，正在读取最新内容" }
                if !advanced && !browsing { load(first: true) }
            }
            bindAuthorization(); await authorization.refresh()
        } catch { if version == epoch { status = "离线缓存 · \(terminalError(error))" } }
    }
    func load(first: Bool = false) {
        guard !loading, first || hasMore, let target else { return }
        historyGeneration.bind(target.key)
        loading = true; let version = epoch; let before = first ? nil : cursor
        Task {
            guard version==epoch else{return}
            defer { if version == epoch { loading = false } }
            do {
                var command: [String: Any] = ["action": "history"]; if let before { command["cursor"] = before }
                let page = try await historyCache.load(scope: target.key, cursor: before,
                    fetch: { try await self.request(command, destination: target) },
                    record: { id, cursor in
                        var request: [String: Any] = ["action": "record", "record_id": id, "part": "body"]
                        if let cursor { request["cursor"] = cursor }
                        return try await self.request(request, destination: target)
                    }, valid: { version == self.epoch })
                guard version == epoch else { return }
                let response = page.value
                guard let nextGeneration = (response["generation"] as? NSNumber)?.int64Value,
                      historyGeneration.accept(nextGeneration, scopeKey: target.key) else { return }
                historyCacheWarning = page.warning; historyCacheRevision += 1
                let generationChanged = generation.map { $0 != nextGeneration } ?? false
                if generationChanged { liveReply = AgentLiveReplyState() }
                generation = nextGeneration
                cursor = response["cursor"] as? String; hasMore = response["has_more"] as? Bool ?? false
                if first || generationChanged { pages = [] }
                pages.append((response["items"] as? [[String: Any]] ?? []).map(AgentItem.init))
                if pages.count > 3 { pages.removeFirst() }
                var seen = Set<String>()
                let unique = pages.flatMap { $0 }.filter { seen.insert($0.id).inserted }
                var lastStatus: [String: String] = [:]
                let chronological = unique.reversed().filter { item in
                    guard let key = item.terminalStatusKey else { return true }
                    let previous = lastStatus.updateValue(item.text, forKey: key)
                    return previous != item.text
                }
                items = browsing ? Array(chronological.reversed()) : chronological
                if !page.offline { liveReply.reconcile(history: response["items"] as? [[String: Any]] ?? []) }
                liveText = liveReply.text
                if global && globalTitle == "新会话", let first = chronological.first(where: { $0.kind == "你" }) { globalTitle = String(first.text.prefix(48)) }
                if page.offline { status = "离线缓存 · 删除状态尚未同步" }
            } catch { if version == epoch { status = "历史加载失败：\(terminalError(error))" } }
        }
    }
    func send(_ core: RemoteTerminal) {
        guard canSend, let target else { return }
        let delivery = AgentSendSnapshot(scopeKey: target.key, composer: composer, permissionMode: authorization.mode)
        do {
            try composerStore.flush(delivery.composer, key: target.key)
            try composerStore.saveDelivery(delivery)
        } catch { deliveryError = "待发送消息保存失败，尚未发送：" + error.localizedDescription; return }
        pendingDelivery = delivery
        transmit(delivery, to: target)
    }
    func retrySend(_ delivery: AgentSendSnapshot) {
        guard canRetrySend, pendingDelivery?.requestID == delivery.requestID, let target, delivery.scopeKey == target.key else { return }
        transmit(delivery, to: target)
    }
    func discardPendingDelivery(_ delivery: AgentSendSnapshot) {
        guard !submitting, pendingDelivery?.requestID == delivery.requestID, delivery.scopeKey == target?.key else { return }
        do { try composerStore.discard(delivery); pendingDelivery = nil; deliveryError = "" }
        catch { deliveryError = "发送记录清理失败：" + error.localizedDescription }
    }
    private func transmit(_ delivery: AgentSendSnapshot, to target: ChatScope) {
        let connection = connectionEpoch
        guard delivery.scopeKey == target.key, !sending.contains(target.key) else { return }
        sending.insert(target.key); sendingMessages.insert(target.key); submitting = true; deliveryError = ""
        Task {
            var uploads: [String] = []
            var confirmed = false
            do {
                for picture in delivery.composer.images {
                    let begin = try await request(["action": "image_begin", "media_type": picture.mime, "size": picture.data.count], destination: target, expectedEpoch: connection)
                    guard let upload = begin["upload_id"] as? String else { throw ChatFailure.message("无效图片上传响应") }
                    uploads.append(upload)
                    var offset = 0
                    while offset < picture.data.count {
                        let end = min(offset + 32768, picture.data.count)
                        _ = try await request(["action": "image_chunk", "upload_id": upload, "offset": offset, "data": picture.data.subdata(in: offset..<end).base64EncodedString()], destination: target, expectedEpoch: connection)
                        offset = end
                    }
                }
                var command: [String: Any] = ["action": "send", "request_id": delivery.requestID, "message": delivery.composer.text, "permission_mode": delivery.permissionMode]
                if !uploads.isEmpty { command["images"] = uploads }
                _ = try await request(command, destination: target, expectedEpoch: connection)
                confirmed = true
                do { try composerStore.confirm(delivery) }
                catch { if self.target == target { deliveryError = "消息已发送，本地记录清理失败：" + error.localizedDescription } }
                if self.target == target && pendingDelivery?.requestID == delivery.requestID { pendingDelivery = nil }
                if self.target == target && requestID == delivery.requestID {
                    restoringDraft = true; draft = ""; attachments = []; requestID = UUID().uuidString; restoringDraft = false; saveDraft()
                }
            } catch {
                if self.target == target {
                    deliveryError = "发送结果未确认，原消息已保留。恢复连接后可重试。"
                    if connection == connectionEpoch, !(error is CancellationError) { deliveryError += " " + terminalError(error) }
                }
            }
            // Settling delivery cannot wait for best-effort upload cleanup or a
            // fresh history/authorization round. Local edits were never locked.
            sendingMessages.remove(target.key); sending.remove(target.key); submitting = self.target.map { sending.contains($0.key) } ?? false
            if confirmed, self.target == target, connection == connectionEpoch, !browsing { load(first: true) }
            for upload in uploads {
                guard connection == connectionEpoch else { break }
                _ = try? await request(["action": "image_release", "upload_id": upload], destination: target, expectedEpoch: connection)
            }
        }
    }
    func cancel(_ core: RemoteTerminal) {
        guard canCancel, !submitting, let target else { return }; let version = epoch; let connection = connectionEpoch
        sending.insert(target.key); submitting = true
        Task {
            defer { sending.remove(target.key); submitting = self.target.map { sending.contains($0.key) } ?? false }
            guard version == epoch, connection == connectionEpoch else { return }
            do {
                _ = try await request(["action": "cancel"], destination: target, expectedEpoch: connection)
                if version == epoch && connection == connectionEpoch { await refresh() }
            } catch { if version == epoch && connection == connectionEpoch { status = terminalError(error) } }
        }
    }
    private func saveArchives() {
        guard let identity = scope?.identity, let data = try? JSONEncoder().encode(archives) else { return }
        WorkspacePreferences.defaults.set(data, forKey: "agent.archives." + identity.key)
    }
    func rememberSessions(identity: ChatIdentity?, device: String, deviceName: String, sessions: [RemoteSession]) {
        guard let identity, identity == scope?.identity, !device.isEmpty else { return }
        var updated = archives; var changed = false
        for session in sessions where !session.cwd.isEmpty {
            let target = ChatScope(identity: identity, device: device, session: session.id)
            if let index = updated.firstIndex(where: { $0.scope == target }) {
                changed = updated[index].updateDirectory(session.cwd, title: session.displayName, deviceName: deviceName) || changed
            } else {
                updated.append(ChatArchive(scope: target, title: session.displayName, deviceName: deviceName, cwd: session.cwd)); changed = true
            }
        }
        if changed { archives = updated; saveArchives() }
    }
    func loadArchives() {
        guard connected, !archiveLoading, let scope else { return }
        archiveLoading = true; let connection = connectionEpoch
        let destination = ChatScope(identity: scope.identity, device: scope.device, session: "")
        Task {
            do {
                let result = try await request(["action": "list"], destination: destination, expectedEpoch: connection)
                guard self.scope?.identity == scope.identity, self.scope?.device == scope.device, connectionEpoch == connection else { return }
                var resolved = archives.filter { $0.scope.device != scope.device }
                for row in result["agents"] as? [[String: Any]] ?? [] {
                    guard let source = row["scope"] as? [String: Any], let session = source["session"] as? String, !session.isEmpty else { continue }
                    let target = ChatScope(identity: scope.identity, device: scope.device, session: session)
                    let known = archives.first { $0.scope == target }
                    resolved.append(known ?? ChatArchive(scope: target, title: "终端 " + String(session.prefix(8)), deviceName: desktopName))
                }
                // A newly selected terminal can have no agent history yet.
                if !scope.session.isEmpty, !resolved.contains(where: { $0.scope == scope }), let current = archives.first(where: { $0.scope == scope }) { resolved.append(current) }
                archives = resolved; saveArchives()
            } catch { /* Existing metadata and cached history remain accessible offline. */ }
            if connectionEpoch == connection { archiveLoading = false }
        }
    }
    func openHistory(_ archive:ChatArchive) { saveDraft(); historyTarget=archive.scope; global=archive.scope.session.isEmpty; globalID=nil; restoreDraft(); browsing=true; reset() }
    func record(_ id: String) {
        let version = epoch
        Task { guard version==epoch else{return}; do {
            var next: String?; var bytes = Data(); var text = ""
            repeat {
                var command: [String: Any] = ["action": "record", "record_id": id, "part": "body"]; if let next { command["cursor"] = next }
                let result = try await request(command); guard version == epoch else { return }
                if result["encoding"] as? String == "base64url" {
                    var value = (result["body"] as? String ?? "").replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
                    value += String(repeating: "=", count: (4 - value.count % 4) % 4)
                    guard let data = Data(base64Encoded: value) else { throw ChatFailure.message("图片数据无效") }; bytes.append(data)
                } else { text += result["body"] as? String ?? "" }
                guard bytes.count + text.utf8.count <= 4 * 1024 * 1024 else { throw ChatFailure.message("证据超过 4 MiB 读取限制") }
                next = result["cursor"] as? String
            } while next != nil
            image = bytes.isEmpty ? nil : UIImage(data: bytes); evidence = text; evidenceVisible = true
        } catch { if version == epoch { status = "证据不可用：\(terminalError(error))" } } }
    }
}
private extension Optional where Wrapped == String { var orEmpty: String { self ?? "" } }

extension AssistantModel {
    var desktopScope: ChatScope? { scope.map { ChatScope(identity: $0.identity, device: $0.device, session: "") } }
    var desktopConnected: Bool { connected }
    var connectionEpoch: Int { terminal?.generation ?? 0 }
    private var globalKey: String { "agent.globals." + (desktopScope?.key ?? "") }
    private var composer: AgentComposer { AgentComposer(text: draft, images: attachments, requestID: requestID, allowInput: nil) }
    private func scheduleDraft() {
        guard !restoringDraft, let draftScope else { return }
        let value = composer
        let id = value.requestID
        composerStore.schedule(value, key: draftScope.key) { [weak self] message in
            guard let self, self.draftScope == draftScope, self.requestID == id else { return }
            self.status = "草稿保存失败：" + message
        }
    }
    private func saveDraft() {
        guard !restoringDraft, let draftScope else { return }
        do { try composerStore.flush(composer, key: draftScope.key) }
        catch { status = "草稿保存失败：" + error.localizedDescription }
    }
    private func restoreDraft() {
        draftScope = target; restoringDraft = true
        let saved = target.map { composerStore.draft($0.key) } ?? AgentComposer()
        draft = saved.text; attachments = saved.images; requestID = saved.requestID
        pendingDelivery = target.flatMap { composerStore.delivery($0.key) }; deliveryError = ""
        submitting = target.map { sending.contains($0.key) } ?? false; restoringDraft = false
    }
    func addImages(_ urls: [URL]) {
        do {
            var added: [AgentAttachment] = []; var total = attachments.reduce(0) { $0 + $1.data.count }
            for url in urls {
                let access = url.startAccessingSecurityScopedResource(); defer { if access { url.stopAccessingSecurityScopedResource() } }
                let size = (try url.resourceValues(forKeys: [.fileSizeKey])).fileSize ?? 0
                guard size <= 4 * 1024 * 1024, attachments.count + added.count < 4 else { throw ChatFailure.message("最多 4 张图片，单张不超过 4 MiB，总计不超过 8 MiB") }
                let data = try Data(contentsOf: url)
                let mime = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? ""
                guard ["image/png", "image/jpeg", "image/webp", "image/gif"].contains(mime), UIImage(data: data) != nil, data.count <= 4 * 1024 * 1024 else { throw ChatFailure.message("仅支持 4 MiB 内的 PNG、JPEG、WebP、GIF 图片") }
                total += data.count; guard total <= 8 * 1024 * 1024 else { throw ChatFailure.message("图片总计超过 8 MiB") }
                added.append(AgentAttachment(mime: mime, data: data))
            }
            attachments.append(contentsOf: added); requestID = UUID().uuidString; scheduleDraft()
        } catch { status = terminalError(error) }
    }
    func removeImage(_ id: String) { attachments.removeAll { $0.id == id }; requestID = UUID().uuidString; scheduleDraft() }
    private func restoreGlobals() {
        if let data = WorkspacePreferences.defaults.data(forKey: globalKey), let rows = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] { globalRows = rows }
        globalConfirmed = false
    }
    func globalUnread(_ row: [String: Any]) -> Bool {
        let id = (row["scope"] as? [String: Any])?["agent"] as? String ?? ""
        let sequence = (row["last_reply_sequence"] as? NSNumber)?.int64Value ?? 0
        return sequence > Int64(WorkspacePreferences.defaults.integer(forKey: globalKey + ".read." + id))
    }
    func markGlobalRead() {
        guard global, let id = globalID, !browsing, visible else { return }
        guard !loading, !items.contains(where: { $0.reply && $0.partial }), let sequence = items.filter(\.reply).map(\.sequence).max(), sequence > 0 else { return }
        let key = globalKey + ".read." + id
        if sequence > Int64(WorkspacePreferences.defaults.integer(forKey: key)) { WorkspacePreferences.defaults.set(sequence, forKey: key) }
    }
    func globalState(_ row: [String: Any]) -> String {
        guard connected, globalConfirmed else { return "状态待同步" }
        let state = row["state"] as? String ?? "idle"
        return ["idle": "空闲", "running": "执行中", "monitoring": "监控中", "stopping": "停止中", "finishing": "完成中", "failed": "失败"][state] ?? state
    }
    func refreshGlobals() {
        guard connected, !globalLoading, let destination = desktopScope else { return }
        globalLoading = true; let version = globalEpoch; let connection = connectionEpoch
        Task {
            do {
                var rows: [[String: Any]] = []; var cursor: NSNumber?
                repeat {
                    var command: [String: Any] = ["action": "global_list"]; if let cursor { command["cursor"] = cursor }
                    let page = try await request(command, destination: destination, expectedEpoch: connection)
                    guard desktopScope == destination, version == globalEpoch, connectionEpoch == connection else { return }
                    rows.append(contentsOf: page["conversations"] as? [[String: Any]] ?? []); cursor = page["cursor"] as? NSNumber
                } while cursor != nil
                globalRows = rows; globalConfirmed = true; globalError = ""
                if let data = try? JSONSerialization.data(withJSONObject: rows) { WorkspacePreferences.defaults.set(data, forKey: globalKey) }
            } catch { if desktopScope == destination, version == globalEpoch, connectionEpoch == connection { globalConfirmed = false; globalError = "会话同步失败：" + terminalError(error) } }
            if version == globalEpoch { globalLoading = false }
        }
    }
    func createGlobal(isCurrent: @escaping () -> Bool = { true }, _ opened: @escaping () -> Void) {
        guard connected, !globalCreating, let destination = desktopScope else { return }
        globalCreating = true; let version = globalEpoch; let connection = connectionEpoch
        let key = globalKey + ".pending"; let id = WorkspacePreferences.defaults.string(forKey: key) ?? UUID().uuidString
        WorkspacePreferences.defaults.set(id, forKey: key)
        Task {
            do {
                let result = try await request(["action": "global_create", "request_id": id], destination: destination, expectedEpoch: connection)
                guard desktopScope == destination, version == globalEpoch, connectionEpoch == connection else { return }
                guard let scope = result["scope"] as? [String: Any] else { throw ChatFailure.message("无效会话响应") }
                let row: [String: Any] = ["scope": scope, "title": "新会话", "state": "idle"]
                WorkspacePreferences.defaults.removeObject(forKey: key); globalRows.insert(row, at: 0); if isCurrent() { openGlobal(row); opened() }
            } catch { if desktopScope == destination, version == globalEpoch, connectionEpoch == connection { globalError = "创建失败：" + terminalError(error) } }
            if version == globalEpoch { globalCreating = false }
        }
    }
}
