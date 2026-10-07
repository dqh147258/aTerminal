# 核心远程屏幕

状态：Completed。复用协调者已授权的范围和 Medium 验证；主计划位于主 checkout `doc/task/1007-remote-screens/PLAN.md`。不修改 Android/iOS，不再委派。

专用 Operation 27/28，独立 screen_id/max_width 字段，JSON 返回在 Reply.history[0]。List 增加可选 screen_protocol_version=1；mobile-core 先检测能力再请求，新客户端连接旧 Desktop 时提示升级，保持终端连接。

已认证账号及配对用户允许只读屏幕操作；只读配对也可查看。屏幕内容授权继承 Desktop 连接查看权限，不依赖终端会话或写控制。Host 校验本地 token 和 account_scope，remote bridge 覆盖客户端身份，截屏模块不获取 Host 或终端服务锁。

屏幕 stream job 独立后台槽，每连接一次在途，独立本地 Client 避免占用终端 RPC 锁。全局单采集工作线程，2 秒返回超时；底层不能取消时保留 busy 许可，不无限创建线程。macOS 使用安全 core-graphics 枚举及系统 screencapture，系统 sips 先缩小再解码，两个子进程合计 1.5 秒超时后 kill。Linux/Windows 用 xcap 0.4.1 安全 API；Linux X11 支持，Wayland 因库依赖 XWayland 元数据、不能保证全部真实显示器而明确 unsupported，不回传虚拟屏幕。稳定 native ID 不用列表索引，OS 重启/热插拔可以更换身份。

源图预检 16M 像素（RGBA 64MiB；库可能还有内部副本）；缩放不超过 1920x1920，不放大。JPEG 120KiB，JSON 164KiB，预留 WebRTC 256KiB 缓冲与 protobuf/信封开销。若高熵画面超过限额降低质量/尺寸。屏幕重放图像缓存另设小额预算并保留请求签名，过期图像重放返回刷新错误。

Linux 编译新增 libxcb1-dev、libxcb-randr0-dev、libwayland-dev；已有 libdbus-1-dev 继续使用。修改对应 workspace CI 安装步骤。不引入 PipeWire。

必要验证：协议兼容与能力字段、只读授权、未知屏幕/错误、噪声与竖屏 JPEG 限额、超时后单并发、异步屏幕请求不干扰终端。运行所辖 Rust 测试、fmt 和 clippy。补充本机真实元数据；真实像素采集涉及录屏权限与隐私，只在合适现成环境下验证并如实记录，无物理手机验收承诺。


验证结果：`cargo test --locked --no-default-features -p ai-terminal-agent -p ai-terminal-mobile -p ai-terminal-protocol --lib` 通过：Desktop 70、mobile 15、protocol 6；2 个需要桌面条件的 ignored 检查另行通过。全库已有 PTY/cwd 测试在 sandbox 内无法观察进程目录，授权的环境外运行通过，不改这些测试语义。加密 relay RPC 夹具覆盖采集在途时 List/History 可继续、只读拒绝 Input、未知 Operation 返回错误后连接可继续、并发 busy 与重放不重复采集；夹具模拟 native 输出，不冒充真实端到端移动 UI。

`cargo check --locked -p ai-terminal-agent -p ai-terminal-mobile --features ai-terminal-mobile/webrtc` 通过；`cargo clippy --locked --no-default-features -p ai-terminal-agent -p ai-terminal-mobile -p ai-terminal-protocol --all-targets -- -D warnings`、fmt、diff 检查通过。

本机真实元数据得到 4 块屏幕，像素 5120×2880（主屏）、1080×1920、2560×1080、2880×5120。真实主屏 JPEG 1200×675、72,976 bytes、约 0.91 秒通过；没有持久保存或输出屏幕像素。Debug 初次全尺寸 Rust 解码超过 2 秒，已改为系统 sips 在解码前缩小，超时限制没有放宽。

Linux/Windows 已做平台静态审查，未在真实 Linux/Windows 桌面执行；对应 CI 构建仍需协调者/CI 确认。Wayland 明确不支持；native ID 跨 OS 重启/热插拔可能变化；16M 像素以上屏幕枚举仍可见，帧返回 screen_size_limit；不能取消的 Linux/Windows 库调用超时后保持单线程 busy，恢复底层 API 或重启 Desktop 后可恢复。依赖内部/系统图像副本不等于严格进程 RSS 上限。
