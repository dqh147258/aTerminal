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
    init(_ object: [String: Any]) {
        sequence = (object["sequence"] as? NSNumber)?.int64Value ?? 0
        id = object["id"] as? String ?? UUID().uuidString
        let eventKind = object["kind"] as? String ?? "记录"
        kind = ["user":"你","assistant":"AI Agent","interaction":"操作与证据"][eventKind] ?? "状态"
        let value = object["value"] as? [String: Any] ?? [:]
        reply = ["assistant", "interaction"].contains(eventKind); partial = value["partial"] as? Bool ?? false
        terminalStatusKey = eventKind.hasPrefix("pty_status") ? (value["session_id"] as? String ?? "") : nil
        let summary = (value["updates"] as? [[String:Any]])?.reversed().compactMap{$0["summary"] as? String}.first
        let fallback = eventKind.hasPrefix("pty_status") ? "终端状态："+((value["session_process"] as? [String:Any])?["state"] as? String ?? value["state"] as? String ?? "已更新") : (eventKind=="interaction" ? "终端或扩展操作记录，可查看关联证据。" : "Agent 状态："+(value["state"] as? String ?? "已更新"))
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
    @Published var draft = "" { didSet { if !restoringDraft { requestID = UUID().uuidString; saveDraft() } } }
    @Published var attachments: [AgentAttachment] = []
    @Published var submitting = false
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
    private var globalConfirmed = false
    private var globalEpoch = 0
    private var contextConnection = -1
    private var archiveLoading = false
    @Published var allowInput = false { didSet { if !restoringDraft { requestID = UUID().uuidString; saveDraft() } } }
    @Published var global = false
    @Published var browsing = false
    @Published var loading = false
    @Published var hasMore = false
    @Published var available = false
    @Published var running = false
    @Published var liveText = ""
    @Published var evidence = ""
    @Published var image: UIImage?
    @Published var evidenceVisible = false
    @Published var settingsVisible = false
    private var scope: ChatScope?
    private var historyTarget:ChatScope?
    private var connected = false
    private var desktopName=""
    var contextLabel: String {
        if global { return desktopName }
        guard let target else { return "未选择终端" }
        if target.device == terminal?.deviceID, let session = terminal?.sessions.first(where: { $0.id == target.session }) { return session.cwd }
        return archives.first(where: { $0.scope == target })?.title ?? "离线历史"
    }
    var destinationLabel:String {(scope?.identity.account ?? "")+" · "+desktopName}
    private var visible = false
    private var epoch = 0
    private var generation: Int64?
    private var cursor: String?
    private var pages: [[AgentItem]] = []
    private var fullMessages: [String: [String: Any]] = [:]
    private var task: Task<Void, Never>?
    private weak var terminal: TerminalModel?
    private var cache: AgentCache?
    var canSend: Bool { writeReason == nil && connected && available && !submitting && (global ? globalID != nil : !(target?.session ?? "").isEmpty) && (!draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty) }
    var canCancel: Bool { writeReason == nil && connected && running }
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
        do {
            let root = WorkspacePreferences.historyDirectory
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            cache = try AgentCache.open(path: root.appendingPathComponent("agent-cache.sqlite3").path)
        } catch { status = "历史缓存不可用：\(error.localizedDescription)" }
    }
    func context(identity: ChatIdentity?, scope: ChatScope?, title: String, device: String, connected: Bool, core: RemoteTerminal, terminal: TerminalModel) {
        self.terminal = terminal;desktopName=device
        let connected = connected || (WorkspacePreferences.fixture && identity != nil)
        let connectionChanged = contextConnection != terminal.generation
        guard self.scope != scope || self.connected != connected || connectionChanged else { return }
        contextConnection = terminal.generation
        if connectionChanged || self.connected != connected { globalEpoch += 1; globalLoading = false; globalCreating = false; globalConfirmed = false; archiveLoading = false }
        let changedDesktop = self.scope?.identity != scope?.identity || self.scope?.device != scope?.device
        saveDraft(); stop(); self.scope = scope; historyTarget=nil; self.connected = connected; available = false
        if let identity, let data = WorkspacePreferences.defaults.data(forKey: "agent.archives." + identity.key) { archives = (try? JSONDecoder().decode([ChatArchive].self, from: data)) ?? [] }
        else { archives = [] }
        if changedDesktop { globalID = nil; global = false; globalEpoch += 1; globalLoading = false; globalCreating = false; globalRows = []; restoreGlobals() }
        restoreDraft()
        if let scope, !scope.session.isEmpty {
            archives.removeAll { $0.scope == scope }; archives.append(ChatArchive(scope: scope, title: title, deviceName: device)); saveArchives()
        }
        #if DEBUG
        if SettingsFixture.enabled, let identity {
            let examples = [ChatArchive(scope: ChatScope(identity: identity, device: "fixture-desktop", session: "fixture-closed"), title: "已关闭会话", deviceName: "Fixture Desktop"),
                            ChatArchive(scope: ChatScope(identity: identity, device: "fixture-offline-desktop", session: "fixture-offline"), title: "离线历史", deviceName: "Offline Desktop")]
            for archive in examples where !archives.contains(where: { $0.scope == archive.scope }) { archives.append(archive) }
            saveArchives()
        }
        #endif
        if connected {loadArchives()}
        reset(); if visible { start() }
    }
    func setVisible(_ value: Bool, core: RemoteTerminal) { visible = value; stop(); if value { reset(); start() } }
    func stop() { epoch += 1; task?.cancel(); task = nil; loading = false }
    func switchScope() { historyTarget=nil; restoreDraft(); stop(); reset(); if visible { start() } }
    func openSession() { saveDraft(); global = false; globalID = nil; browsing = false; switchScope() }
    func openGlobal(_ row: [String: Any]) {
        guard let id = (row["scope"] as? [String: Any])?["agent"] as? String else { return }
        saveDraft(); globalID = id; global = true; globalTitle = row["title"] as? String ?? "新会话"; browsing = false; switchScope()
    }
    func reset() { epoch += 1; loading = false; generation = nil; cursor = nil; pages = []; items = []; liveText = ""; hasMore = true; load(first: true) }
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
        if let expectedEpoch, expectedEpoch != connectionEpoch { throw ChatFailure.message("Desktop 连接已变化") }
        #if DEBUG
        if SettingsFixture.enabled { return SettingsFixture.agent(command, session: (destination ?? target)?.session) }
        #endif
        guard let terminal, let target = destination ?? target else { throw ChatFailure.message("Desktop 未连接") }
        var body = command; if !configuration { body["version"] = 1 }
        var session = target.session
        if session.hasPrefix("global:") { if !configuration { body["agent_id"] = String(session.dropFirst(7)) }; session = "" }
        let json = String(decoding: try JSONSerialization.data(withJSONObject: body), as: UTF8.self)
        let wire = ChatScope(identity: target.identity, device: target.device, session: session)
        let text = try await terminal.agent(scope: wire, json: json, configuration: configuration)
        if let expectedEpoch, expectedEpoch != connectionEpoch { throw ChatFailure.message("Desktop 连接已变化") }
        guard let value = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else { throw ChatFailure.message("无效的 Desktop 响应") }
        return value
    }
    private func refresh() async {
        guard connected, let target else { return }; let version = epoch
        do {
            let response = try await request(["action": "state"])
            guard version == epoch, !Task.isCancelled else { return }
            available = response["available"] as? Bool ?? false
            let state = response["state"] as? String ?? "idle"
            running = ["running", "monitoring", "stopping", "finishing", "cancelling"].contains(state)
            liveText = response["live_text"] as? String ?? ""
            status = (global ? "全局" : "当前终端") + " · " + state + (response["error"] as? String).map { " · " + $0 }.orEmpty
            if let next = (response["history_generation"] as? NSNumber)?.int64Value {
                try cache?.reconcile(scope: target.key, generation: next)
                if let generation, generation != next { reset() }
                else if !browsing { load(first: true) }
            }
        } catch { if version == epoch { status = "离线缓存 · \(terminalError(error))" } }
    }
    func load(first: Bool = false) {
        guard !loading, first || hasMore, let target else { return }
        loading = true; let version = epoch; let before = first ? nil : cursor
        Task {
            guard version==epoch else{return}
            do {
                var command: [String: Any] = ["action": "history"]; if let before { command["cursor"] = before }
                var offline = false
                var response: [String: Any]
                do {
                    response = try await request(command, destination: target)
                    var expanded: [[String: Any]] = []
                    for item in response["items"] as? [[String: Any]] ?? [] {
                        guard version == epoch else { return }
                        expanded.append((try? await completeMessage(item, target: target, version: version)) ?? item)
                    }
                    response["items"] = expanded
                    let text = String(decoding: try JSONSerialization.data(withJSONObject: response), as: UTF8.self)
                    try? cache?.storePage(scope: target.key, cursor: before, page: text)
                } catch {
                    guard let text = try cache?.page(scope: target.key, cursor: before), let cached = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else { throw error }
                    response = cached; offline = true
                }
                guard version == epoch else { return }
                generation = (response["generation"] as? NSNumber)?.int64Value
                cursor = response["cursor"] as? String; hasMore = response["has_more"] as? Bool ?? false
                if first { pages = [] }
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
                if global && globalTitle == "新会话", let first = chronological.first(where: { $0.kind == "你" }) { globalTitle = String(first.text.prefix(48)) }
                if offline { status = "离线缓存 · 删除状态尚未同步" }
            } catch { if version == epoch { status = "历史加载失败：\(terminalError(error))" } }
            if version == epoch { loading = false }
        }
    }
    func send(_ core: RemoteTerminal) {
        guard canSend, let target else { return }
        let message = draft; let pictures = attachments; let id = requestID; let permission = allowInput; let connection = connectionEpoch
        sending.insert(target.key); submitting = true; saveDraft()
        Task {
            var uploads: [String] = []
            do {
                for picture in pictures {
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
                var command: [String: Any] = ["action": "send", "request_id": id, "message": message, "allow_input": permission]
                if !uploads.isEmpty { command["images"] = uploads }
                _ = try await request(command, destination: target, expectedEpoch: connection)
                if self.target == target && requestID == id {
                    restoringDraft = true; draft = ""; attachments = []; requestID = UUID().uuidString; restoringDraft = false; saveDraft()
                    if !browsing { reset() }; await refresh()
                } else { AgentComposer.clearConfirmed(target, requestID: id) }
            } catch { if self.target == target { status = "发送未确认：\(terminalError(error))，草稿已保留，可重试" } }
            for upload in uploads { _ = try? await request(["action": "image_release", "upload_id": upload], destination: target, expectedEpoch: connection) }
            sending.remove(target.key); submitting = self.target.map { sending.contains($0.key) } ?? false
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
    private func completeMessage(_ item: [String: Any], target: ChatScope, version: Int) async throws -> [String: Any] {
        guard let value = item["value"] as? [String: Any], value["partial"] as? Bool == true,
              let id = value["record_id"] as? String else { return item }
        let key = target.key + ":" + id
        if let original = fullMessages[key] { var result = item; result["value"] = original; return result }
        var text = ""; var cursor: String?
        repeat {
            guard version == epoch else { throw CancellationError() }
            var command: [String: Any] = ["action": "record", "record_id": id, "part": "body"]; if let cursor { command["cursor"] = cursor }
            let part = try await request(command, destination: target)
            guard version == epoch, part["kind"] as? String == "history_event" else { throw ChatFailure.message("消息原文不可用") }
            text += part["body"] as? String ?? ""
            guard text.utf8.count <= 4 * 1024 * 1024 else { throw ChatFailure.message("消息超过原文大小限制") }
            cursor = part["cursor"] as? String
        } while cursor != nil
        guard let original = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else { throw ChatFailure.message("消息原文格式错误") }
        if fullMessages.count >= 100 { fullMessages.removeAll() }
        fullMessages[key] = original
        var result = item; result["value"] = original; return result
    }
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

struct AgentAttachment: Codable, Identifiable {
    var id = UUID().uuidString
    let mime: String
    let data: Data
}
private struct AgentComposer: Codable {
    var text = ""
    var images: [AgentAttachment] = []
    var requestID = UUID().uuidString
    var allowInput: Bool?
    static func url(_ scope: ChatScope) -> URL { WorkspacePreferences.historyDirectory.appendingPathComponent("draft-" + scope.key + ".json") }
    static func read(_ scope: ChatScope) -> AgentComposer { (try? Data(contentsOf: url(scope))).flatMap { try? JSONDecoder().decode(Self.self, from: $0) } ?? Self() }
    func save(_ scope: ChatScope) throws { try JSONEncoder().encode(self).write(to: Self.url(scope), options: .atomic) }
    static func clearConfirmed(_ scope: ChatScope, requestID: String) {
        if read(scope).requestID == requestID { try? Self().save(scope) }
    }
}

extension AssistantModel {
    var desktopScope: ChatScope? { scope.map { ChatScope(identity: $0.identity, device: $0.device, session: "") } }
    var desktopConnected: Bool { connected }
    var connectionEpoch: Int { terminal?.generation ?? 0 }
    private var globalKey: String { "agent.globals." + (desktopScope?.key ?? "") }
    private func saveDraft() {
        guard !restoringDraft, let draftScope else { return }
        do { try AgentComposer(text: draft, images: attachments, requestID: requestID, allowInput: allowInput).save(draftScope) }
        catch { status = "草稿保存失败：" + error.localizedDescription }
    }
    private func restoreDraft() {
        draftScope = target; restoringDraft = true
        let saved = target.map(AgentComposer.read) ?? AgentComposer()
        draft = saved.text; attachments = saved.images; requestID = saved.requestID; allowInput = saved.allowInput ?? false
        submitting = target.map { sending.contains($0.key) } ?? false; restoringDraft = false
    }
    func addImages(_ urls: [URL]) {
        guard !submitting else { return }
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
            attachments.append(contentsOf: added); requestID = UUID().uuidString; saveDraft()
        } catch { status = terminalError(error) }
    }
    func removeImage(_ id: String) { guard !submitting else { return }; attachments.removeAll { $0.id == id }; requestID = UUID().uuidString; saveDraft() }
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
