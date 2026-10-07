# Android 远程屏幕

状态：Completed（Android 所辖实现与静态/单元验收）。2026-10-07。沿用协调者传入的功能、提交及 Medium 测试授权；不新增委派。完整集成验收由协调者继续，见 [HANDOFF.md](HANDOFF.md)。

目标是主页右侧工具条提供“远程屏幕”，读取当前已连接 Desktop 的全部真实显示器，并连续查看其中一个。只依赖 RemoteTerminal 连接，不依赖终端选择或控制权。复用 MainActivity.panel 与 NativeUi，显示名称、分辨率、主屏标记，以及未连接、加载、空列表、失败和重试状态。查看页保持纵横比，可返回列表、切换、关闭。

接口使用已确定合同：remoteScreensJson() 返回 screens 数组；remoteScreenFrameJson(screenId, maxWidth) 返回 JPEG base64、screen_id、尺寸及 captured_at_ms。Rust 与生成绑定由协调者整合，Android 不修改其他平台。

协调者追加的合同约束已落实：JSON ≤164 KiB、base64 ≤160 KiB、JPEG ≤120 KiB、帧宽高 ≤1920、显示器 ≤64；显示器尺寸合理上限采用 32768。真实 JPEG bounds 必须与声明尺寸一致。已知 Desktop 版本、录屏权限、显示器断开、busy/timeout 和 Wayland 不支持的错误给出中文操作提示，未知错误保留详情。

实现独立的后台请求循环，一次只允许一个任务在途；任务完成后间隔约一秒再取帧。关闭、后台、重连、设备或账号变化均取消调度并使旧请求失效。列表解析、base64 与图片解码放在后台，失效图片及时释放。弹层阻断终端输入，系统返回优先退回列表。

必要验收（Medium）：请求循环的关闭/重开、切换、后台停止、过期成功及失败、慢请求不积压；JSON 协议的空列表、损坏载荷及帧身份检查；新 UI 的静态编译和完整 diff 审查。完整 Android build 与真实 Desktop 流程由协调者在 core 绑定就绪后安排，本子任务不安装或清理正常用户 App。不在本子任务做真机测试，因为缺少新接口与隔离测试配置。

进度：
- [x] 确认工具条、原生弹层、连接重试及输入隔离结构。
- [x] 实现请求循环、协议解析、列表/查看页及主页接入。
- [x] 20 项 JUnit 通过；新 UI 与 6 项 instrumentation 测试通过 API 35 静态编译；MainActivity 在临时合同签名下通过全体 Kotlin 静态编译。
- [x] 差异审查及交接文档准备完成，提交与 executor CLI 报告为最终交付步骤。
