import SwiftUI

enum WorkspaceStyle {
    static let background = Color(red: 18/255, green: 20/255, blue: 22/255)
    static let surface = Color(red: 26/255, green: 29/255, blue: 32/255)
    static let control = Color(red: 36/255, green: 41/255, blue: 45/255)
    static let foreground = Color(red: 237/255, green: 240/255, blue: 242/255)
    static let muted = Color(red: 160/255, green: 168/255, blue: 174/255)
    static let accent = Color(red: 165/255, green: 196/255, blue: 212/255)
    static let success = Color(red: 162/255, green: 198/255, blue: 174/255)
    static let danger = Color(red: 228/255, green: 163/255, blue: 163/255)
    static let line = Color(red: 48/255, green: 54/255, blue: 58/255)
}

struct ToolButton: View {
    let symbol: String
    let label: String
    var active = false
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            Image(systemName: symbol).font(.system(size: 19, weight: .regular))
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
                .background(WorkspaceStyle.accent).cornerRadius(6)
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
