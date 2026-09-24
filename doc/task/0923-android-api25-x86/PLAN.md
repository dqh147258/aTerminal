# Android API 25 与 x86 兼容及设备验收

- Status: Completed
- Updated: 2026-09-23

## 目标与范围

让现有 Android App 在 API 25、x86 设备 `127.0.0.1:62001` 上安装并运行，完成终端与账号验收。AI 仍为不可交互空壳；不实现模型、录音或 AI 操作。保留 arm64-v8a、x86_64 支持，不覆盖工作区已有未提交改动。

## 当前状态与证据

现有设备是 API 25、`x86`；`adb install -r .../app-debug.apk` 返回 `INSTALL_FAILED_NO_MATCHING_ABIS`。`apps/android/app/build.gradle.kts` 固定 `minSdk=26` 且只包含 arm64-v8a/x86_64；`scripts/build-mobile.py` 和 `scripts/prepare-bindings.py` 无 i686 支持，`scripts/android-arm64.cmake` 固定 NDK API 26。`NativeUi.kt`、`MainActivity.kt` 直接使用 API 26 的 autofill/tooltip。stable 工具链已包含 `i686-linux-android`，NDK r28.2 有 API 25 x86 clang。

首轮真机启动时另复现 `__atomic_compare_exchange` 无法从 API 25 系统解析；x86 原生链接加入 NDK compiler-rt builtins 后再次安装及真实终端流程均通过。

## 方案与执行

用户在 2026-09-23 明确要求支持 API 25/x86 并继续在指定设备测试，视为执行授权。将应用最小版本及三 ABI 的 NDK 编译 API 降为 25；构建脚本支持 i686，复制对应 `.so` 和 C++ 运行库；对仅 API 26 存在的 Android UI 方法加系统版本保护。重建原生库与 APK，在真机优先跑无需账号的渲染/界面回归；如有可用隔离测试账号及桌面 fixture，再跑实际登录、PTY 输入、恢复、会话关闭。

## 验证

运行 Android APK/测试 APK 构建与 lint；用 `aapt` 验证 minSdk 和 ABI，ADB 在该设备安装并执行 UI/状态/显示一致性 instrumentation。用现有 Rust E2E 保障跨端通道。用户已明确要求在真设备验收；若隔离 fixture 不可用，明确记录未完成的真实登录范围，不触碰正式账号。

已验证三 ABI API 25 原生库和 APK、`assembleDebug assembleDebugAndroidTest lintDebug`；设备安装成功，状态/显示 10 项通过（API 29 硬件缓存项按条件跳过），UI 4 项通过。最终 APK 上隔离 `MobileWorkflowTest` 完整通过，结果见 `build/android-api25-final-workflow.json`：真实登录/PTY、120 列横滑、字号、Ctrl-C、后台恢复、会话关闭后连接仍可用、AI 占位无操作、退出登录。连接路径为 USB `adb reverse` 的 relay，不表示公网或蜂窝验证。Rust `agent` 7 项测试通过；本次转发、测试密码及 fixture 服务已清理。

## 风险与回退

API 25 上 NDK WebRTC/C++ 依赖可能引用较新符号，须真机加载并验证而非只做编译。设备已有用户数据不清空；测试只使用隔离命名空间/账号，结束清理本次转发与夹具。可通过恢复原先 minSdk/ABI 配置回退。

## 未决问题、歧义与确认

None.
