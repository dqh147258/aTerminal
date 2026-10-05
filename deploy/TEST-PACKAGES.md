# aTerminal 测试安装包

这些是 GitHub Actions 产生的测试构建，不是正式发行版。范围仅包含
Windows x64、Linux x64、macOS Apple Silicon/Intel 的 Desktop CLI，以及 Android Debug APK。
不包含 iOS、Server 镜像、安装器、GitHub Release、正式签名或公证。
Desktop 使用 Rust `release` 优化配置；这里的 `release` 是编译配置，不表示正式发布。

## 下载与内容核对

在对应提交的 GitHub Actions 运行页面下载平台 artifact。GitHub 下载的外层 ZIP
内含本脚本生成的 `.zip` 或 `.tar.gz`、对应的 `.sha256` 和 `.build-info.json`。
先核对运行页提交 SHA，再核对校验文件；同一来源的校验和用于发现损坏，不能替代来源认证。

内层包包含：

- `aTerminal` / `aTerminal.exe`，或 `aTerminal-debug.apk`
- `README.md`、`THIRD_PARTY.md`、本说明；Desktop 另含 Noto CJK 字体许可
- `BUILD-INFO.json`：完整源码提交 SHA、目标、预期编译配置、工作流上下文、文件大小/权限/哈希、二进制头部或 APK 原生库检查结果
- `SHA256SUMS`：除其自身外全部包内文件的 SHA-256（包含 `BUILD-INFO.json`）

外部 `.build-info.json` 还记录整个归档的 SHA-256 和全部内部文件（含 `SHA256SUMS`）的清单。
归档生成后会重新读取，核对文件集合、内容哈希及权限。Linux/macOS 的 tar 包保留输入文件的普通执行权限，丢弃 setuid/setgid/sticky 位。

例如，Linux 在外层解压后的目录验证归档：

```sh
sha256sum -c aTerminal-test-*.tar.gz.sha256
```

macOS 可用 `shasum -a 256 <归档文件>`，Windows PowerShell 可用
`Get-FileHash -Algorithm SHA256 <归档文件>`，将结果与对应 `.sha256` 内容比较。
解开内层包后，在包目录中验证 `SHA256SUMS`（Linux：`sha256sum -c SHA256SUMS`；
macOS：`shasum -a 256 -c SHA256SUMS`）。

## Desktop 试用注意事项

请选择与操作系统及 CPU 对应的包。macOS arm64 和 x64 是两个独立的 thin 二进制，非 Universal 包。
在终端中进入解压目录，先用 `./aTerminal --help`（PowerShell：`./aTerminal.exe --help`）查看用法。
这是 CLI，不是 GUI 应用安装器。不要直接用无参数启动来代替无副作用的帮助检查。

Desktop 包没有发行身份签名或 macOS 公证；Apple Silicon 链接器可能带 ad-hoc 签名。
系统或组织安全策略可能阻止运行。先验证来源、提交和校验和，再遵循平台/管理员的正常审批流程；不要关闭系统安全防护。

Linux 包在 Ubuntu 22.04 runner 构建，依赖兼容的 glibc、系统动态库以及 D-Bus 运行库。
Secret Service 凭据存储还需要可用的桌面会话/密钥环服务。
这不是静态 musl 可执行文件，也不保证在更老发行版、无桌面容器或所有 Linux 系统运行。
缺库时应使用发行版正常的软件包管理方式，由测试人员确认所需依赖。

首次功能试用应使用隔离状态目录与测试账号，避免干扰现有 Agent、登录身份及终端会话。
不要为了替换测试包自动停止已有 Agent；停止可能使仍在运行的 PTY 会话无法恢复。
完整本地开发文档中的局域网地址及一键安装脚本是特定开发环境配置，不能直接当作本测试包的服务端配置。

## Android 试用注意事项

- Android API 25+，应用 ID `com.yxf.aterminal`
- 一个 APK 同时包含 `arm64-v8a`、`x86_64`、`x86`
- 每个 ABI 必须含 `libai_terminal_mobile.so` 和 `libc++_shared.so`
- 这是可调试的 Debug APK，使用临时 CI runner 的标准调试密钥；不适合商店发布或生产使用
- 后续运行可能生成不同调试密钥。同应用 ID 的已安装包若签名不同，无法直接覆盖安装；可能需要卸载，而卸载会删除应用本地数据。请先评估并备份，不要自动执行卸载

本测试工作流显式传入空的 `-PterminalServerUrl=`，默认不内置服务器地址，也不猜测生产端点。
在登录界面设置你有权限使用的测试服务器和测试账号。构建自己的测试包时，可显式使用
`-PterminalServerUrl=https://your-test-server.example`；不要提交账号密码、令牌、私钥或私有 CA。
已保存的服务器配置可能优先于新的构建默认值。

APK 结构检查不会验证账号、连通性或运行时功能。工作流另用 `aapt` 核对 APK manifest
及目标 SDK/应用 ID/ABI，并用 `apksigner verify` 验证签名；应查看对应作业日志。
默认地址等构建配置需由工作流单独检查，不能从文件名或通过头部检查推断。

## 本地打包与自测

打包脚本使用 Python 3.10+ 标准库。它只消费已构建文件，不会编译、安装、执行、签名、上传或发布。
从仓库运行，`--sha` 应传入构建该文件时实际检出的完整 `git rev-parse HEAD` 值。
脚本记录这个调用方提供的值；不会假装从可执行文件恢复源码提交。

```sh
python3 scripts/package-test-artifact.py --self-test
python3 scripts/package-test-artifact.py desktop \
  --target x86_64-unknown-linux-gnu \
  --binary target/x86_64-unknown-linux-gnu/release/aTerminal \
  --sha "$(git rev-parse HEAD)"
python3 scripts/package-test-artifact.py android \
  --apk apps/android/app/build/outputs/apk/debug/app-debug.apk \
  --sha "$(git rev-parse HEAD)"
```

Desktop `--target` 支持：

```text
x86_64-pc-windows-msvc
x86_64-unknown-linux-gnu
aarch64-apple-darwin
x86_64-apple-darwin
```

默认输出到仓库 `dist/`；可用 `--output-dir` 指向单独目录。已有同名产物时会拒绝覆盖。
成功时 stdout 输出 JSON，失败时非零退出；设置 `SOURCE_DATE_EPOCH` 可固定打包时间，
但工作流上下文或源文件变化仍会改变产物，所以不宣称可复现编译。

自测使用合成 PE/ELF/Mach-O 头部和合成 APK，验证四种目标、错误架构拒绝、Android
缺库/错误 ABI/重复 ZIP 项拒绝、提交格式、执行权限、拒绝覆盖，以及归档读回核对。
合成文件不可运行，测试通过不代表真实 Rust/Gradle 构建、OS 启动、Android 安装、网络连接、
远程会话或 UI 验收通过。真实工作流结果及设备/平台验证应分别报告。

`THIRD_PARTY.md` 是已有依赖说明，不是完整 SBOM 或发行许可审计；正式发布前仍需完成
所有静态/动态依赖的 notices、适用源码交付、运行依赖、签名、公证及安全验收。
