import SwiftUI

struct AgentItem: Identifiable {
    let id: String
    let kind: String
    let text: String
    let records: [String]
    let terminalStatusKey: String?
    init(_ object: [String: Any]) {
        id = object["id"] as? String ?? UUID().uuidString
        let eventKind = object["kind"] as? String ?? "记录"
        kind = ["user":"你","assistant":"AI Agent","interaction":"操作与证据"][eventKind] ?? "状态"
        let value = object["value"] as? [String: Any] ?? [:]
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
    @Published var draft = ""
    @Published var allowInput = false
    @Published var global = false
    @Published var browsing = false
    @Published var loading = false
    @Published var hasMore = false
    @Published var available = false
    @Published var running = false
    @Published var evidence = ""
    @Published var image: UIImage?
    @Published var evidenceVisible = false
    @Published var settingsVisible = false
    @Published var legacyVisible = false
    @Published var legacyRows: [[String: String]] = []
    @Published var legacyText = ""
    @Published var legacyBefore: Int64?
    private var legacyScope = ""
    private var legacyPrefix = ""
    @Published var legacyListCursor:String?
    private var scope: ChatScope?
    private var historyTarget:ChatScope?
    private var connected = false
    private var desktopName=""
    var destinationLabel:String {(scope?.identity.account ?? "")+" · "+desktopName}
    private var visible = false
    private var epoch = 0
    private var generation: Int64?
    private var cursor: String?
    private var pages: [[AgentItem]] = []
    private var task: Task<Void, Never>?
    private weak var terminal: TerminalModel?
    private var cache: AgentCache?
    var canSend: Bool { connected && available && !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    var canCancel: Bool { connected && running }
    var target: ChatScope? { (historyTarget ?? scope).map { ChatScope(identity: $0.identity, device: $0.device, session: global ? "" : $0.session) } }
    init() {
        do {
            let root = WorkspacePreferences.historyDirectory
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            cache = try AgentCache.open(path: root.appendingPathComponent("agent-cache.sqlite3").path)
        } catch { status = "历史缓存不可用：\(error.localizedDescription)" }
    }
    func context(identity: ChatIdentity?, scope: ChatScope?, title: String, device: String, connected: Bool, core: RemoteTerminal, terminal: TerminalModel) {
        self.terminal = terminal;desktopName=device
        guard self.scope != scope || self.connected != connected else { return }
        stop(); self.scope = scope; historyTarget=nil; self.connected = connected; available = false; archives = []
        if let scope { archives = [ChatArchive(scope: scope, title: title, deviceName: device)] }
        if connected {loadArchives()}
        reset(); if visible { start() }
    }
    func setVisible(_ value: Bool, core: RemoteTerminal) { visible = value; stop(); if value { reset(); start() } }
    func stop() { epoch += 1; task?.cancel(); task = nil; loading = false }
    func switchScope() { historyTarget=nil; stop(); reset(); if visible { start() } }
    func reset() { epoch += 1; loading = false; generation = nil; cursor = nil; pages = []; items = []; hasMore = true; load(first: true) }
    func start() {
        task?.cancel()
        task = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refresh()
                try? await Task.sleep(nanoseconds: 1_500_000_000)
            }
        }
    }
    func request(_ command: [String: Any], configuration: Bool = false) async throws -> [String: Any] {
        guard let terminal, let target else { throw ChatFailure.message("Desktop 未连接") }
        var body = command; if !configuration { body["version"] = 1 }
        let json = String(decoding: try JSONSerialization.data(withJSONObject: body), as: UTF8.self)
        let text = try await terminal.agent(scope: target, json: json, configuration: configuration)
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
            running = ["running", "stopping", "finishing"].contains(state)
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
                    response = try await request(command)
                    let text = String(decoding: try JSONSerialization.data(withJSONObject: response), as: UTF8.self)
                    try cache?.storePage(scope: target.key, cursor: before, page: text)
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
                if offline { status = "离线缓存 · 删除状态尚未同步" }
            } catch { if version == epoch { status = "历史加载失败：\(terminalError(error))" } }
            if version == epoch { loading = false }
        }
    }
    func send(_ core: RemoteTerminal) {
        guard canSend else { return }
        let message = draft; let version = epoch
        Task { guard version==epoch else{return}; do {
            _ = try await request(["action": "send", "request_id": UUID().uuidString, "message": message, "allow_input": allowInput])
            guard version == epoch else { return }; draft = ""; if !browsing { reset() }; await refresh()
        } catch { if version == epoch { status = "发送未确认：\(terminalError(error))，请查询状态" } } }
    }
    func cancel(_ core: RemoteTerminal) {
        let version=epoch
        Task { guard version==epoch else{return}; do { _ = try await request(["action": "cancel"]); await refresh() } catch { status = terminalError(error) } }
    }
    func loadArchives() {
        guard let scope else{return};let version=epoch
        Task {do {
            let result=try await request(["action":"list"])
            guard self.scope?.identity==scope.identity,self.scope?.device==scope.device else{return}
            archives=(result["agents"] as? [[String:Any]] ?? []).compactMap { row in
                guard let source=row["scope"] as? [String:Any] else{return nil}
                let session=source["session"] as? String ?? ""
                return ChatArchive(scope:ChatScope(identity:scope.identity,device:scope.device,session:session),title:session.isEmpty ? "全局 Agent" : "终端 "+String(session.prefix(8)),deviceName:scope.device)
            }
        }catch {if epoch==version{status=terminalError(error)}}}
    }
    func openHistory(_ archive:ChatArchive) {historyTarget=archive.scope;global=archive.scope.session.isEmpty;browsing=true;reset()}
    func legacy() {
        guard let identity=scope?.identity, let cache else { return }
        let root=WorkspacePreferences.historyDirectory.appendingPathComponent(identity.key)
        let prefix="legacy/"+identity.key+"/"
        legacyPrefix=prefix
        Task {
            do {
                let rows = try await Task.detached { () -> [[String: String]] in
                    if FileManager.default.fileExists(atPath:root.path) {
                        let files=try FileManager.default.contentsOfDirectory(at:root,includingPropertiesForKeys:nil)
                        for file in files where file.pathExtension=="json" { _ = try cache.importLegacy(scope:prefix+file.deletingPathExtension().lastPathComponent,path:file.path) }
                    }
                    let text=try cache.legacyScopes(identityPrefix:prefix,before:nil)
                    let result=try JSONSerialization.jsonObject(with:Data(text.utf8)) as? [String:Any] ?? [:]
                    return (result["items"] as? [[String:Any]] ?? []).map { row in ["scope":row["scope"] as? String ?? "", "title":(row["metadata"] as? [String:Any])?["title"] as? String ?? "旧对话"] }
                }.value
                guard scope?.identity==identity else{return}
                legacyRows=rows;legacyListCursor=rows.count==50 ? rows.last?["scope"] : nil;legacyText="";legacyBefore=nil;legacyVisible=true
            } catch { status="旧归档导入失败，原文件已保留："+terminalError(error) }
        }
    }
    func legacyMore(){
        guard let cache,let cursor=legacyListCursor else{return}
        do {let text=try cache.legacyScopes(identityPrefix:legacyPrefix,before:cursor);let result=try JSONSerialization.jsonObject(with:Data(text.utf8)) as? [String:Any] ?? [:]
            legacyRows=(result["items"] as? [[String:Any]] ?? []).map{row in ["scope":row["scope"] as? String ?? "","title":(row["metadata"] as? [String:Any])?["title"] as? String ?? "旧对话"]}
            legacyListCursor=legacyRows.count==50 ? legacyRows.last?["scope"] : nil
        }catch{status=terminalError(error)}
    }
    func legacyPage(_ scope:String?=nil) {
        if let scope {legacyScope=scope;legacyBefore=nil}
        do {
            guard let text=try cache?.legacyPage(scope:legacyScope,before:legacyBefore), let page=try JSONSerialization.jsonObject(with:Data(text.utf8)) as? [String:Any] else {return}
            legacyText=(page["items"] as? [[String:Any]] ?? []).map { item in let value=item["value"] as? [String:Any] ?? [:];return (value["role"] as? String ?? "")+"\n"+(value["content"] as? String ?? "") }.joined(separator:"\n\n")
            legacyBefore=(page["before"] as? NSNumber)?.int64Value
        } catch {status=terminalError(error)}
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
                next = result["cursor"] as? String
            } while next != nil && bytes.count + text.utf8.count < 4 * 1024 * 1024
            image = bytes.isEmpty ? nil : UIImage(data: bytes); evidence = text; evidenceVisible = true
        } catch { if version == epoch { status = "证据不可用：\(terminalError(error))" } } }
    }
}
private extension Optional where Wrapped == String { var orEmpty: String { self ?? "" } }
