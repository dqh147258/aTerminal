# iOS 后续统一参考

本轮 Android 已完成，iOS 按用户确认后续同步。

首先读取 [共用原型图集](UI_REFERENCE.md)、[Android 实现对照](ANDROID_REFERENCE.md) 与 [验收结果](RESULTS.md)。截图为项目永久文件，并有源文件/图片指纹，不依赖外部目录或临时浏览器状态。

必须沿用三个用户例外：名称 `aTerminal`；服务器摘要与编辑交互以现有合理结构为基准，视觉统一最新蓝色；不提供旧手机归档、导入或迁移旧手机数据。原型样例文字仍有旧切换器描述，应采用实际独立 Global 多会话结构。

主要落点为 WorkspaceScreen.swift 的全屏设置和返回、ChatPanel.swift 中配置入口独立化、WorkspaceStyle.swift 的主题/表单、AssistantModel.swift 的配置回调与当前能力。复用现有 configuration RPC、expected_revision、安全凭据、Scope/草稿/历史语义；目录刷新不可覆盖新能力，保存成功直接采用返回 snapshot，冲突保留草稿并由用户再次确认保存。

后续需移除 iOS 当前 ChatPanel/AssistantModel 的旧手机归档入口与自动 importLegacy 调用；不把 Android 当前正常 Session/Global 历史误删。共享 Rust 未使用的 legacy API 可在该工作中按真实调用关系处理，不清理用户磁盘文件。

按同一页面编号保存 iOS 实现截图与设备/字体/commit 信息，实际验证键盘、返回/取消、失败草稿、秘密清理、目录迟到和配置完整性。本说明不宣称 iOS 已改造或测试通过。
