# 首次提交前代码审查

日期：2026-09-22。范围为首次提交的整个工作区，重点覆盖账号/设备授权、凭据持久化、远程输入与状态同步、移动端生命周期、部署及忽略规则。本次确认的问题已修复；没有发现剩余的提交阻塞项，不代表正式发布验收已全部完成。

## 已修复问题

| 级别 | 位置 | 触发与影响 | 修复及验证 |
| --- | --- | --- | --- |
| P1 | `crates/mobile-core/src/remote.rs`，`State::apply` | 控制权回复先到，携带旧 control epoch 的新画面增量随后到达，原实现整条忽略；终端可能停留旧状态或触发不必要的基线恢复。 | 控制信息与显示 revision 分别判断，忽略过期控制信息但仍校验/应用合法画面；新增回归先复现失败，再验证修复与后续 delta 连续性。 |
| P1 | `crates/mobile-core/src/remote.rs`，订阅边界处理 | 跨路径迟到的旧订阅回复可能把已经应用的状态序号回退，等待桌面已经确认并释放的旧帧。 | 订阅回复按请求 ID 判新旧，应用游标单调不回退；覆盖旧回复晚到、画面先于回复到达及正常前进的确定性测试。 |
| P1 | `crates/desktop-agent/src/service.rs`，Create 归属 | 远程创建请求通过鉴权后、PTY 启动期间切换账号，按结束时的当前账号分配 owner 会把会话分给另一账号。 | 远程会话绑定已认证请求的 account_scope；本地无账号请求保持原当前账号归属逻辑。静态并发路径确认，现有跨账号隔离与切换回归通过。 |
| P2 | `crates/desktop-agent/src/account.rs`、两端账号动作 | refresh 成功后 heartbeat/撤销/改密请求失败，新令牌可能没有保存；旧 refresh 已失效，重启会丢失登录。 | 后续请求失败也保存已轮换凭据；存储失败保留待保存标志并暂停远程任务。新增真实 HTTP 失败与写入失败/恢复测试；两端失败路径使用 defer/finally 持久化。 |
| P2 | Android `MainActivity.kt`、iOS `AITerminalApp.swift`，历史查询 | 独立历史线程完成时用户已切会话或退出，迟到回调仍显示旧结果；iOS 旧历史内容也未清空。 | 回调检查当前代次/活动状态，切换或退出清理历史内容；Android 真机注入延迟历史任务后切换会话，确认未弹出旧结果。 |

没有为修复关闭状态 hash、撤销控制权检查、丢弃输入或放宽账号边界。网络协议和现有降级合同保持不变。

## 验证

- Rust workspace 39 项测试通过：`cargo +stable test --locked --workspace --exclude ai-terminal-bindgen`。
- `cargo +stable clippy --locked --workspace --all-targets --exclude ai-terminal-bindgen -- -D warnings` 和 fmt 通过。
- WebRTC 专项测试通过，包含直连超时跨路径重试不重复执行。
- 宿主 PTY 输入、输出、备用屏与终端模式恢复测试通过。
- Android arm64/x86_64 原生库、App/测试 APK、lintDebug 构建通过；lint 仍有既有警告。
- Android 12 真机专项流程通过：登录、直连、USB 回环中转、Ctrl-C、6 轮切会话、后台恢复、退出，以及新增迟到历史抑制。此次使用 smoke 模式，不把以前的 3,000 字符压力数据当成本轮重新执行。
- iOS 真机/模拟器 Rust 库与 Xcode 模拟器 App 构建通过；iOS 历史回调修复本轮只完成代码审查与编译，没有新增真机 UI 运行证据。
- 未发现待提交内容包含实际私钥、访问令牌或邀请密钥。忽略规则覆盖本地凭据、构建产物、运行数据库和 IDE 缓存，保留 lockfile、Gradle Wrapper、Xcode 共享配置及源码测试。

原始日志在忽略目录 `build/review/`，不提交临时凭据或测试数据库。

## 仍需发布验收

Windows/Linux 原生运行、iOS 真机、真实公网 NAT/TURN/丢包矩阵尚未全面验证。已有性能报告明确部分真机帧时间和回显尾延迟未达目标；本次审查未将其改写为达标。详见 [后续验证指导](task/0922-account-input/HANDOFF.md) 和 [性能续作结果](task/0922-account-input/FOLLOWUP.md)。

本次仅创建本地首次提交，不推送远程。
