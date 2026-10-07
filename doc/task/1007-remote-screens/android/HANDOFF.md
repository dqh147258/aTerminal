# Android 远程屏幕交接

实现范围完成，待协调者整合新的 core/UniFFI 绑定并进行完整构建和设备验收。本 worktree 不合并到原始 checkout，不安装正常用户 App。

主页右侧“远程屏幕”直接读取当前已连接 Desktop 的所有真实显示器，与 selected/controlled 无关。原生列表包含名称、分辨率、主屏标记，以及未连接时的选设备入口、加载、空列表、失败和重试。查看使用 ImageView.FIT_CENTER，保持纵横比；返回与切换回列表，顶栏关闭整个页面。

MainActivity 的独立 screenWorker 处理 RPC、JSON/base64 与 Bitmap 解码。Activity 共享 RemoteScreenRequests 在旧任务排空之前保留 busy 门控，快速关闭/重开或切换只保留最新请求；每次帧完成后约一秒再请求。关闭、onPause/onStop、断连、重连、设备/账号/服务地址变化均停止或使旧结果失效。重连成功可继续当前查看，source 每次固定当前 transport 与 reconnect epoch。旧 Bitmap 从未显示时主动 recycle；已显示图片交给 Android 回收，避免 RenderThread 使用已回收位图。现有遮罩、IME 和 hardware key 条件阻止终端误输入。

协议限制已与协调者给出的 core 合同对齐：JSON 164 KiB、base64 160 KiB、JPEG 120 KiB、帧宽高 1920、显示器 64。显示器边长上限 32768；先用 Long 验证范围，再转为 Int，防止溢出。实际 JPEG bounds/mime 必须与帧元数据一致。已知错误提供中文升级、系统录屏设置、刷新显示器、重试或切换 X11 的指引。

实际验证（Medium）：

- Kotlin 2.0.21 + JUnitCore：RemoteScreenRequestsTest 8 项、RemoteScreensProtocolTest 6 项、原有 WorkspaceReconnectTest 6 项，共 `OK (20 tests)`。覆盖请求不堆积、晚到成功/失败、关闭重开、后台/重连停止、显式重试、帧身份、上限与尺寸溢出、中文错误提示。
- 新生产 UI 与 RemoteScreensUiTest 6 项通过 Android API 35 静态编译。UI 测试涵盖列表/主屏/portrait、FIT_CENTER、切换/返回、关闭后晚到 decode、连接恢复、空列表/旧 Desktop 提示、JPEG bounds 不一致和未连接选设备。
- 全部生产 Android Kotlin，包括 MainActivity，使用原 checkout 的未修改基线生成绑定与 R.jar，以及 `build/remote-screens-check` 内临时两个合同方法签名和无凭据 BuildConfig，通过静态编译。该检查不验证新原生 ABI。仅有仓库既有的 Android deprecated API 等警告。
- `git diff --check` 通过；逐文件审查没有修改 Rust/iOS 或 NativeUi 的既有行为。

本机检查仅读取旧生成绑定/R.jar，解包 Gradle 缓存中公开依赖的 classes.jar 到忽略的 `build/remote-screens-check`；未复制凭据或项目源码到其他 checkout。临时合同签名仅用于静态检查，不参与源码提交或 App 构建。

后续必要集成验证由协调者负责：生成含 `remoteScreensJson()`、`remoteScreenFrameJson(screenId: String, maxWidth: UInt)` 的绑定及对应原生库；运行 `:app:testDebugUnitTest`、完整 APK/测试 APK 构建，并在隔离 fixture 包运行 RemoteScreensUiTest。真实 Desktop 验证横竖屏、多显示器、权限拒绝、拔出显示器、后台/返回、连接切换及终端按键隔离。当前尚未执行 instrumentation、真机/模拟器、真实 RPC 或新 ABI 完整 build，不能将静态编译描述为这些验收通过。

查看目前仅 fit-center，没有额外缩放手势。抓屏失败会停止连续请求并显示重试按钮，避免持续错误请求；X11/Wayland 支持边界由 core 提供。
