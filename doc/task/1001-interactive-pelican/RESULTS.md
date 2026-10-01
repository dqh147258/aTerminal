# Codex 交互字符动画功能验收

- Date: 2026-10-01
- Primary workflow: Completed with one UI recovery after task submission
- Android: `aiterminal_api36_test` / `emulator-5586`
- Agent model: `stealth/space-bunny-alpha`

模拟器正常 AI Agent 输入框启动真实 `codex` TUI，在目标目录输入完整制作要求并单独发送 Enter。Codex 创建 `pelican_bike.py`，执行有限帧预览及退出恢复验证。手机 Agent 随后退出 Codex，在同一 Terminal 启动 `python3 pelican_bike.py`，目前保持播放。未执行 `codex exec`。

## 证据

- 实际顺序为 launch 621 → Enter 630 → TUI banner 653 → task draft 659 → Enter 668 → submitted echo/Working 676；[引用链](evidence/verified-interactive-chain.json)。
- [完整工作流](evidence/live-prompt-recovery/android-workflow.json)保存调用/回执和归档终端正文。该次 instrumentation 的最后断言误选了倒序归档的晚期重复 banner，原报告保留失败状态。修正验收器后，对同一真实记录的正反序 JVM 重放均通过，精确匹配上述六个步骤；[离线验收](evidence/chronology-verification.txt)。没有重写失败报告为成功。
- [运行画面 1](evidence/animation/frame-1.png)、[运行画面 2](evidence/animation/frame-2.png)：同一 PTY revision 22616 → 22686，4190 个像素发生变化；[播放校验](evidence/animation/verification.json)。
- 程序的 48 帧全部不同，均为纯 ASCII、74 列 × 23 行；只用 Python 标准库；[程序检查](evidence/animation/program-verification.json)。
- [Android 画面](evidence/animation/android-live.png)来自仍登录的真实 App。

## 产物与使用

文件：[pelican_bike.py](/Users/carl/Downloads/Temp2026/Temp10/test-1001/pelican_bike.py)。

```sh
cd /Users/carl/Downloads/Temp2026/Temp10/test-1001
python3 pelican_bike.py
```

Ctrl-C 退出；`python3 pelican_bike.py --frames 48` 做有限帧预览。终端需至少 75 列 × 24 行。当前 Session `bb62538b3469573d`，测试窗格 GUID `51F18086-7020-4222-82AA-AB2B94A061FC`。

## 修复与实际限制

首次与恢复在旧版本原文校验暂停；保存失败回复后证明 U+200A 被模型替换成普通空格。新增有界原文行引用，精确展开后继续原严格校验，并保留失败诊断。窗口 focus 通知的人工输入计数问题由真实 PTY/actor red/green 回归证明并修复；真实键入/粘贴/鼠标/resize 仍抢占 Agent。另补行动阶段指令，避免内部分析 JSON 被误当成最终答复。

实际任务提交后，模型把 TUI 行范围中的空白分隔行一并分类，触发一次分析暂停；通过手机继续消息恢复既有 Codex，没有重复启动。源码已修正为先校验整个引用范围，再忽略不可作为搜索锚点的空白分隔行；旧字符串与错误索引校验保留，关键回归通过。当前播放实例在此次源码修正前启动，未为刷新代码中断已经运行的动画。

登录恢复与验收选择器的失败记录也保留：只恢复同一测试账号/设备密钥，未清除 App 数据；直接测试 Account HTTP 操作后补上现有持久化步骤。根目录测试数据库改用私有 data 子目录，保留生产权限要求。

集成 Rust 检查：107 个 Desktop/runtime/CLI 与相关集成测试通过、1 个既有真实 ModelScope 用例保持 ignored；额外 protocol 5 / mobile-core 12 项已通过。后续 runtime 空白引用回归为 45 unit + 5 model_boundary；fmt、clippy 通过。Android 三架构、Debug/AndroidTest APK、Lint 与 iOS 模拟器 App 编译链接通过；iOS 本轮未做设备 UI 验收。
