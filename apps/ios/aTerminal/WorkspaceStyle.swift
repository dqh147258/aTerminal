import SwiftUI

enum WorkspaceStyle {
    static let background = Color(red: 9/255, green: 13/255, blue: 22/255)
    static let surface = Color(red: 16/255, green: 22/255, blue: 35/255)
    static let control = Color(red: 20/255, green: 28/255, blue: 44/255)
    static let foreground = Color(red: 248/255, green: 250/255, blue: 252/255)
    static let muted = Color(red: 131/255, green: 145/255, blue: 167/255)
    static let accent = Color(red: 56/255, green: 189/255, blue: 248/255)
    static let success = Color(red: 162/255, green: 198/255, blue: 174/255)
    static let danger = Color(red: 228/255, green: 163/255, blue: 163/255)
    static let line = Color(red: 34/255, green: 46/255, blue: 65/255)
}

struct ToolButton: View {
    let symbol: String
    let label: String
    var active = false
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            Image(systemName: symbol).font(.system(size: 16, weight: .regular))
                .frame(width: 44, height: 44).foregroundColor(active ? WorkspaceStyle.background : WorkspaceStyle.accent)
                .background(active ? WorkspaceStyle.accent : Color.clear).cornerRadius(6)
        }.buttonStyle(.plain).accessibilityLabel(label).help(label)
    }
}

struct PrimaryButton: View {
    let title: String
    var symbol = "arrow.right"
    var busy = false
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) { if busy { ProgressView().tint(WorkspaceStyle.background) }; Text(title).fontWeight(.semibold); Image(systemName: symbol) }
                .frame(maxWidth: .infinity, minHeight: 52).foregroundColor(WorkspaceStyle.background)
                .background(LinearGradient(colors: [WorkspaceStyle.accent, Color(red: 14/255, green: 165/255, blue: 233/255)], startPoint: .topLeading, endPoint: .bottomTrailing)).cornerRadius(8)
        }.buttonStyle(.plain)
    }
}

struct FieldShell<Content: View>: View {
    let title: String
    let symbol: String
    @ViewBuilder var content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.subheadline).foregroundColor(WorkspaceStyle.muted)
            HStack(spacing: 12) { Image(systemName: symbol).foregroundColor(WorkspaceStyle.muted); content }
                .padding(.horizontal, 12).frame(minHeight: 50).background(WorkspaceStyle.surface)
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(WorkspaceStyle.line)).cornerRadius(8)
        }
    }
}

struct EmptyWorkspace: View {
    let symbol: String
    let title: String
    var detail = ""
    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: symbol).font(.system(size: 28)).foregroundColor(WorkspaceStyle.accent)
            Text(title).font(.headline)
            if !detail.isEmpty { Text(detail).font(.subheadline).foregroundColor(WorkspaceStyle.muted).multilineTextAlignment(.center) }
        }.frame(maxWidth: .infinity).padding(24)
    }
}

extension RemoteSession {
    var displayName: String { let name = (cwd as NSString).lastPathComponent; return name.isEmpty ? "终端" : name }
}
