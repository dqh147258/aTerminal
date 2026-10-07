import SwiftUI

struct RemoteScreensPanel: View {
    @ObservedObject var model: RemoteScreenModel
    let chooseDevice: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            if !model.connected {
                Spacer()
                EmptyWorkspace(symbol: "display", title: "尚未连接 Desktop", detail: "连接设备后即可查看远程屏幕")
                Button("选择设备", action: chooseDevice).frame(minHeight: 44).accessibilityIdentifier("screens.devices")
                Spacer()
            } else if let selected = model.selected {
                viewer(selected)
            } else {
                displayList
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity).background(WorkspaceStyle.background)
            .onDisappear { model.stop() }
    }

    private var displayList: some View {
        VStack(spacing: 0) {
            HStack {
                Text("显示器 · \(model.screens.count)").font(.subheadline).foregroundColor(WorkspaceStyle.muted)
                Spacer()
                ToolButton(symbol: "arrow.clockwise", label: "刷新显示器") { model.refreshList() }
                    .disabled(model.listLoading).accessibilityIdentifier("screens.refresh")
            }.padding(.horizontal, 16)
            if model.listLoading {
                Spacer(); ProgressView("正在读取显示器…").accessibilityIdentifier("screens.loading"); Spacer()
            } else if let error = model.listError {
                Spacer()
                EmptyWorkspace(symbol: "exclamationmark.triangle", title: "无法读取显示器", detail: error)
                Button("重试") { model.refreshList() }.frame(minHeight: 44).accessibilityIdentifier("screens.retry")
                Spacer()
            } else if model.screens.isEmpty {
                Spacer()
                EmptyWorkspace(symbol: "display", title: "暂无可用显示器", detail: "请检查 Desktop 的显示器连接和屏幕录制权限")
                Button("重新读取") { model.refreshList() }.frame(minHeight: 44).accessibilityIdentifier("screens.retry")
                Spacer()
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(model.screens) { screen in
                            Button { model.select(screen.id) } label: {
                                HStack(spacing: 14) {
                                    Image(systemName: "display").font(.system(size: 24)).foregroundColor(WorkspaceStyle.accent)
                                    VStack(alignment: .leading, spacing: 6) {
                                        Text(screen.displayName).font(.headline).foregroundColor(WorkspaceStyle.foreground)
                                        Text(screen.dimensions).font(.system(size: 12, design: .monospaced)).foregroundColor(WorkspaceStyle.muted)
                                    }
                                    Spacer(minLength: 0)
                                    if screen.isPrimary { Text("主屏").font(.caption).foregroundColor(WorkspaceStyle.accent) }
                                    Image(systemName: "chevron.right").foregroundColor(WorkspaceStyle.muted)
                                }.padding(16).frame(maxWidth: .infinity).background(WorkspaceStyle.control).cornerRadius(8)
                            }.buttonStyle(.plain).accessibilityIdentifier("screens.select." + screen.id)
                        }
                    }.padding(.horizontal, 16).padding(.bottom, 16)
                }.accessibilityIdentifier("screens.list")
            }
        }
    }

    private func viewer(_ selected: RemoteDisplay) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                ToolButton(symbol: "arrow.left", label: "返回显示器列表") { model.showList() }.accessibilityIdentifier("screens.back")
                VStack(alignment: .leading, spacing: 2) {
                    Text(selected.displayName).font(.subheadline).lineLimit(1)
                    Text(selected.dimensions + (selected.isPrimary ? " · 主屏" : "")).font(.caption2).foregroundColor(WorkspaceStyle.muted)
                }
                Spacer(minLength: 0)
                ToolButton(symbol: "rectangle.on.rectangle", label: "切换显示器") { model.showList() }
                    .accessibilityIdentifier("screens.switch")
            }.padding(.horizontal, 8)
            ZStack {
                Color.black
                if let frame = model.frame {
                    Image(frame.image, scale: 1, label: Text(selected.displayName + "远程屏幕")).resizable().scaledToFit()
                        .accessibilityIdentifier("screens.image")
                } else if model.frameLoading {
                    ProgressView("正在获取屏幕画面…").accessibilityIdentifier("screens.frame.loading")
                }
                if let error = model.frameError {
                    VStack(spacing: 8) {
                        Text("屏幕刷新已暂停").font(.headline)
                        Text(error).font(.caption).multilineTextAlignment(.center).foregroundColor(WorkspaceStyle.muted)
                        Button(model.frameNeedsListRefresh ? "刷新显示器" : "重试") {
                            if model.frameNeedsListRefresh { model.showList(); model.refreshList() }
                            else { model.retryFrame() }
                        }.frame(minHeight: 44).accessibilityIdentifier("screens.frame.retry")
                    }.padding(20).background(WorkspaceStyle.surface.opacity(0.95)).cornerRadius(8).padding(16)
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
            Text(model.frameError == nil ? "仅查看 · 约每秒刷新" : "当前画面已停止刷新")
                .font(.caption2).foregroundColor(WorkspaceStyle.muted).padding(8)
        }
    }
}
