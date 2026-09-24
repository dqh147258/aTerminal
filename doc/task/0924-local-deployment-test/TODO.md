# x86 本地部署全流程验收与文档修订 TODO

- Status: Completed
- Updated: 2026-09-24

## Checklist

- [x] 修正 AI 占位、Admin 预览与 x86 构建文档
- [x] 构建并核对 x86 APK 与当前 Server 镜像
- [x] 启动隔离真实本地部署与 Desktop PTY
- [x] 在目标设备完成完整终端工作流与回归
- [x] 清理测试资源并记录结果

## Verification evidence

- `build-mobile.py android --android-abi x86`、`prepare-bindings.py`、`assembleDebug assembleDebugAndroidTest lintDebug`、Docker Server Release 镜像构建通过；APK `minSdk=25` 且含三 ABI。
- `build/local-deployment-android-ui.log`：16 项中 15 项通过、API 29 专属项按条件跳过；`build/local-deployment-android-lan-workflow.log`：真实局域网完整工作流 `OK (1 test)`。
- `build/local-deployment-android-lan/followup-results.json`：`passed=true`、120 列、真实 PTY/恢复/Ctrl-C/关闭后连接/退出均通过，终端路径 `relay`；设备无 ADB 反向映射，服务器为 `https://192.168.0.36:7200`。
- 专用 Agent、账号、Docker 容器/卷/网络、设备测试私有凭据与 CA 已清理；原部署未改动。
