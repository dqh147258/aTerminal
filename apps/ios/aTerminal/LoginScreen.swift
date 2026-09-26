import SwiftUI

struct LoginScreen: View {
    @ObservedObject var model: TerminalModel
    @State private var server = WorkspacePreferences.defaults.string(forKey: "login.server") ?? WorkspacePreferences.defaultServer
    @State private var editingServer = false
    @State private var username = ""
    @State private var password = ""
    @State private var visible = false
    @State private var validation: String?
    @FocusState private var field: Int?
    var body: some View {
        GeometryReader { geometry in
            ScrollView {
                VStack(alignment: .leading, spacing: 28) {
                    HStack(spacing: 12) {
                        Image(systemName: "terminal").font(.system(size: 22)).frame(width: 36, height: 36).background(WorkspaceStyle.accent).foregroundColor(WorkspaceStyle.background).cornerRadius(8)
                        Text("AI").fontWeight(.semibold) + Text(" TERMINAL").foregroundColor(WorkspaceStyle.muted)
                        Spacer()
                    }.font(.subheadline)
                    VStack(alignment: .leading, spacing: 12) {
                        Text("YOUR WORKSPACE, CONNECTED.").font(.system(size: 12, design: .monospaced)).foregroundColor(WorkspaceStyle.accent)
                        Text("aTerminal").font(.system(size: 32, weight: .semibold)).accessibilityAddTraits(.isHeader)
                        Text("登录，回到你的工作现场。").foregroundColor(WorkspaceStyle.muted)
                    }.padding(.top, 12)
                    VStack(alignment: .leading, spacing: 20) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("登录地址").font(.caption).foregroundColor(WorkspaceStyle.muted)
                            if editingServer {
                                TextField("https://", text: $server).keyboardType(.URL).textContentType(.URL).focused($field, equals: 0).submitLabel(.done).onSubmit(saveServer).accessibilityIdentifier("login.server")
                            } else {
                                Text(server.isEmpty ? "未配置服务器" : server).font(.subheadline).foregroundColor(WorkspaceStyle.muted).textSelection(.enabled).accessibilityIdentifier("login.server.address")
                            }
                            Button(editingServer ? "保存服务器地址" : "修改服务器地址") {
                                if editingServer { saveServer() } else { editingServer = true; field = 0 }
                            }.font(.caption).foregroundColor(WorkspaceStyle.accent).padding(.vertical, 8).accessibilityIdentifier("login.server.edit")
                        }
                        FieldShell(title: "账号", symbol: "person") {
                            TextField("账号", text: $username).textContentType(.username).focused($field, equals: 1).submitLabel(.next).onSubmit { field = 2 }.accessibilityIdentifier("login.username")
                        }
                        FieldShell(title: "密码", symbol: "lock") {
                            Group {
                                if visible { TextField("密码", text: $password) }
                                else { SecureField("密码", text: $password) }
                            }.textContentType(.password).focused($field, equals: 2).submitLabel(.go).onSubmit(login).accessibilityIdentifier("login.password")
                            ToolButton(symbol: visible ? "eye.slash" : "eye", label: visible ? "隐藏密码" : "显示密码") { visible.toggle() }
                        }
                        if let error = validation ?? model.error { Text(error).font(.subheadline).foregroundColor(WorkspaceStyle.danger).fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("login.error") }
                        if model.busy { Text(model.status).font(.subheadline).foregroundColor(WorkspaceStyle.muted) }
                        PrimaryButton(title: model.busy ? "连接中" : "登录", busy: model.busy, action: login).disabled(model.busy).accessibilityIdentifier("login.submit")
                    }.textInputAutocapitalization(.never).autocorrectionDisabled()
                    Spacer(minLength: 0)
                    HStack {
                        Label("桌面工作，随身连接", systemImage: "desktopcomputer")
                        Spacer()
                        Text("01 / LOGIN").font(.system(size: 12, design: .monospaced))
                    }.font(.caption).foregroundColor(WorkspaceStyle.muted).padding(.top, 16).overlay(alignment: .top) { WorkspaceStyle.line.frame(height: 1) }
                }.padding(.horizontal, 24).padding(.vertical, 16).frame(minHeight: geometry.size.height)
            }
        }
    }
    private func saveServer() {
        guard let address = validatedServer() else { return }
        server = address; WorkspacePreferences.defaults.set(address, forKey: "login.server")
        editingServer = false; field = nil; validation = nil
    }
    private func validatedServer() -> String? {
        let address = server.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URLComponents(string: address), ["https", "http"].contains(url.scheme?.lowercased() ?? ""), let host = url.host, !host.isEmpty, url.user == nil, url.password == nil else {
            validation = "请输入有效的服务地址"; editingServer = true; field = 0; return nil
        }
        return address
    }
    private func login() {
        guard !model.busy else { return }
        guard let address = validatedServer() else { return }
        guard !username.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { validation = "请输入账号"; field = 1; return }
        guard !password.isEmpty else { validation = "请输入密码"; field = 2; return }
        validation = nil; field = nil
        WorkspacePreferences.defaults.set(address, forKey: "login.server")
        model.login(server: address, username: username.trimmingCharacters(in: .whitespacesAndNewlines), password: password)
        password = ""
    }
}
