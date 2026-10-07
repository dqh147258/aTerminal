# 第三方依赖记录（P0）

实际版本以 Cargo.lock 和 Gradle 配置为准。以下是关键直接/原生依赖的来源，不替代发布时完整 SBOM、notices 和适用源码交付。

| 组件 | 版本 | 许可/来源 |
| --- | --- | --- |
| alacritty_terminal | 0.26.0 | Apache-2.0，https://github.com/alacritty/alacritty |
| portable-pty | 0.9.0 | MIT，https://github.com/wezterm/wezterm |
| crossterm | Cargo.lock | MIT，https://github.com/crossterm-rs/crossterm |
| UniFFI | 0.32.1 | MPL-2.0，https://github.com/mozilla/uniffi-rs |
| datachannel | 0.16.1 | MPL-2.0，https://github.com/lerouxrgd/datachannel-rs |
| libdatachannel | 0.23.2（datachannel-sys 绑定版本） | MPL-2.0，https://github.com/paullouisageneau/libdatachannel |
| libjuice / usrsctp / OpenSSL | 原生依赖包中固定版本 | 随 datachannel-sys / openssl-src 构建；各自许可必须随发行审查 |
| argon2 | 0.5.3 | MIT OR Apache-2.0，https://github.com/RustCrypto/password-hashes |
| keyring | 3.6.3 | MIT OR Apache-2.0，https://github.com/hwchen/keyring-rs |
| rpassword | 7.5.4 | Apache-2.0，https://github.com/conradkleinespel/rpassword |
| image（Desktop JPEG 编解码） | Cargo.lock | MIT OR Apache-2.0，https://github.com/image-rs/image |
| core-graphics（macOS 显示器与录屏权限） | 0.25.0 | MIT OR Apache-2.0，https://github.com/servo/core-foundation-rs |
| xcap（Linux/Windows 屏幕采集） | 0.4.1 | Apache-2.0，https://github.com/nashaofu/xcap |
| Markwon（Android Markdown） | 4.6.2 | Apache-2.0，https://github.com/noties/Markwon；许可证见 APK assets/licenses/markwon.txt |
| commonmark-java（含 GFM 扩展） | 0.13.0 | BSD-2-Clause，https://github.com/commonmark/commonmark-java；许可证见 APK assets/licenses/commonmark-java.txt |
| JNA | 5.17.0 | LGPL-2.1-or-later / Apache-2.0 双许可，https://github.com/java-native-access/jna |
| Lucide（Web Admin/Android 本地图标） | 0.468.0 | ISC，https://github.com/lucide-icons/lucide；许可证见 `crates/server/admin/LUCIDE-LICENSE`、`apps/android/NOTICE-LUCIDE.md` 及 APK assets |

未修改上游 crate。Android ABI 修复通过项目自有 CMake 工具链传参实现。媒体和 libdatachannel 自带 WebSocket 已关闭。正式发布前需完整检查所有 native 静态/动态依赖及来源交付要求。

账号/输入优化仅参考 VS Code、xterm.js、Mosh、Termux 和 SwiftTerm 的机制，未复制其源代码或新增其运行时依赖。Linux Secret Service 凭据库构建需要 libdbus 开发包。

远程屏幕仅参考 Sirix 的枚举与采集流程，未复制其服务器或客户端实现。macOS 调用系统 `screencapture`/`sips`；Linux X11 另需 libxcb、RandR 和 Wayland 开发库，CI 安装步骤已同步。
