# Android API 25 与 x86 兼容及设备验收 TODO

- Status: Completed
- Updated: 2026-09-23

## Checklist

- [x] 调整 APK/NDK/Rust x86 兼容配置，完成对应 API 25 原生库构建
- [x] 保护 API 26 专有 UI 调用并完成构建、lint 和基本回归
- [x] 在 127.0.0.1:62001 安装应用并运行可用的端到端设备验收

## Verification evidence

- `python3 scripts/build-mobile.py android --toolchain stable --webrtc --android-abi {x86,arm64-v8a,x86_64} --ndk /Users/carl/Library/Android/sdk/ndk/28.2.13676358`：分别完成；x86 compiler-rt builtins 解决 API 25 运行时缺符号。
- `python3 scripts/prepare-bindings.py --toolchain stable`、`./apps/android/gradlew -p apps/android :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug --offline`：通过；APK 声明 minSdk 25，包含三 ABI。
- `adb -s 127.0.0.1:62001 shell am instrument ...`：状态/显示 10 项（1 条按 API 29 跳过）；UI 4 项；最终 APK 的真实终端工作流 1 项通过。
- `build/android-api25-final-workflow.log` 和 `build/android-api25-final-workflow.json`：完整设备结果、`passed=true`、relay 路径；`cargo +stable test --locked -p ai-terminal --test agent`：7 项通过。
- 独立账号 fixture 两次运行后退出并删除；设备专用 `adb reverse` 删除，App 和测试 APK 保留，未碰正式账号。
