# Desktop Terminal 调研证据

日期：2026-09-26。仅调查、隔离诊断和规划，未修改产品代码。

## 成熟方案

| 来源 | 实际读到的合同与本项目取舍 |
| --- | --- |
| [Alacritty input/mod.rs](https://github.com/alacritty/alacritty/blob/master/alacritty/src/input/mod.rs) `scroll_terminal` | 应用 mouse mode → 鼠标报告；`ALT_SCREEN + ALTERNATE_SCROLL` → 方向键；其余 → `Scroll::Delta`。不能把所有滚轮都忽略，也不能全部伪造为 ↑/↓，否则普通 Shell 会翻命令历史。 |
| [tmux 手册](https://github.com/tmux/tmux/blob/master/tmux.1) `history-limit`、copy-mode、`alternate-screen` | 管理自己的有界历史，进入浏览模式；备用屏保留应用前的主屏并在退出时恢复。宿主备用屏不自动提供 pane 的历史。采用独立回看状态，保留 Agent 的权威实时网格。 |
| [tmux terminal-features](https://github.com/tmux/tmux/blob/master/tty-features.c) 与手册 `RGB` / `Tc` | 256 色与 RGB 是独立能力；iTerm2 在成熟实现中具有 RGB 能力。客户端能力决定输出方式，不能因内部模型存储 RGB 就无条件发送 truecolor。 |
| [xterm 控制序列](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html) | `1049h` 切换并清空备用屏；`CSI 2 J` 擦除当前显示；`CSI 3 J` 擦除历史；`1007` 控制 alternate scroll。启动时清屏不应顺带删除用户主屏历史。 |
| 锁定版本 `alacritty_terminal 0.26.0` 本机源码 `term/mod.rs` / `grid/mod.rs` | `swap_alt` 保存主/备用 grid；主屏 `clear_screen(All)` 调用 `clear_viewport`，Saved 分支清历史。已具备成熟解析/重排能力，问题集中在本项目前端与接口；无需另写 VT parser。Grid 可 clone，适合建立有界时间点回看副本。 |
| 锁定版本 `crossterm 0.29.0` 本机源码 `style.rs::available_color_count` | 已依据 `COLORTERM` / `TERM` 区分 truecolor、256 色和基础色，可复用。不要修改 Agent 给内层 PTY 的颜色能力来修宿主输出。 |

在线源码以调研当天的 master 为参考，不作为新增依赖或固定版本兼容声明。Python urllib 的本机 CA 校验失败后改用系统 curl 成功获取原始资料，没有关闭 TLS 验证。

## 现有实现定位

- `crates/desktop-cli/src/input.rs::encode`：未开启应用 mouse mode 时所有 Mouse 事件返回 `None`。
- `crates/desktop-cli/src/render.rs`：`TerminalGuard` 启用 `EnableMouseCapture`；首帧已有 `2J`；`set_style` 无条件 truecolor。
- `crates/desktop-cli/src/managed.rs::run`：Create/Attach/Poll 后才进入 guard；只消费实时 Snapshot/Delta，没有浏览偏移；按 revision 决定重绘。
- `crates/terminal-engine/src/lib.rs`：10,000 行 scrollback；纯文本 `history(offset, limit)`；snapshot 只读实时网格；input_modes 尚未暴露 ALTERNATE_SCROLL。
- `crates/desktop-agent/src/service.rs::session_request`：History 调用 engine；会话 actor 持有实时引擎；Detach 不销毁会话，Close/Shutdown 终止相应资源。
- `crates/protocol/src/lib.rs`：`MAX_CELLS=100_000`，`MAX_MESSAGE_BYTES=4 MiB`；回看不能把 10,000 行 cell 一次装入普通 Snapshot。
- `scripts/test-host-terminal.py`：现有验证检查输入输出、退出备用屏和 termios 恢复；未检查滚轮、历史或颜色降级。

## 一次隔离诊断

现有二进制 `target/debug/aTerminal`，PTY 8 行 × 60 列；临时 Agent state；内层 `/bin/sh -c` 输出 ANSI 红色 `RED_MARKER`、`ROW_000` 到 `ROW_039`、`READY` 后等待输入。宿主模拟为 Apple Terminal 标准环境，发送 `ESC[<64;10;4M` 六次。

```json
{
  "host_env": "Apple_Terminal / xterm-256color / no COLORTERM",
  "enters_alternate_screen": true,
  "has_clear_display": true,
  "has_erase_scrollback": false,
  "uses_truecolor": true,
  "uses_indexed_256": false,
  "wheel_causes_output": false,
  "retained_history_lines": 34,
  "early_output_retained": true
}
```

这确认“回滚入口缺失”，并不确认所有用户历史丢失都由此引起。用户随后确认是拖动宿主滚动条看不到原输出，与备用屏渲染没有接入 Agent 回看视图的证据吻合。测试结束发送 detach 并停止临时 Agent，无产品配置或用户会话修改。

## 尚未实证的情况

真实 Terminal.app 的颜色和启动残留尚未做 GUI 验证；PTY 捕获只能证明发出的序列。Shell 的 ↑ 命令历史另由 Shell 配置负责，本机 `/etc/zshrc_Apple_Terminal` 还会根据 `TERM_SESSION_ID` 管理历史；当前没有用户证据证明这一分支发生故障，计划不修改它。Agent 进程退出后存活 PTY/内存历史不可恢复是现有架构边界，不能与同一 Agent 内 detach/reattach 混为一谈。
