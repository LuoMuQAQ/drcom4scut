# drcom4scut Windows GUI 0.3.2

基于 [SeaLoong/drcom4scut](https://github.com/SeaLoong/drcom4scut) 的 Windows 原生图形客户端，使用 Rust / Win32 编写。本项目是独立维护的 GUI fork，感谢原作者提供认证核心。

## 下载和安装

前往 [Releases](https://github.com/LuoMuQAQ/drcom4scut/releases/latest)，下载 **drcom4scut-Setup-0.3.2.exe** 安装版。默认安装到 64 位 Program Files，可自行选择本地目录；已安装时在原目录覆盖升级。

也提供 **drcom4scut-Portable-0.3.2-win-x64.zip** 便携版，完整解压后运行 `drcom4scutGUI.exe`，需要系统已有兼容 x64 Npcap / WinPcap 驱动。便携模式数据位于 `%LOCALAPPDATA%/drcom4scutGUI`，不会随解压目录移动。

安装器自动检测兼容 x64 Npcap / WinPcap。缺少时从官网下载 Npcap 1.88，核验 SHA-256 与 Authenticode 后启动官方安装窗口。免费 Npcap 需要在官方窗口确认许可和安装，不能全静默安装；本项目不内嵌或再分发 Npcap 安装包。

## 功能

- 原生中文界面、网卡选择、托盘运行、SVG 密码显示/隐藏图标。
- 开关滑动与网卡菜单淡入/滑入动画，采用局部重绘和菜单缓存，支持连续切换及高 DPI。
- 运行时切换分辨率、系统缩放或显示器时，窗口、字体、输入框、图标和点击区域同步适配；小工作区整体缩放，恢复后回到正常尺寸。
- 客户端默认申请管理员权限，可配置以最高权限运行的登录自启计划任务；自启时不显示窗口，在托盘后台运行。
- 核心内部自动恢复，外层提供停滞检测和退避；正常心跳和夜间定时等待不会被周期重启。
- 密码设置使用 Windows DPAPI 保护，凭据通过子进程环境传入核心，不放在命令行中；核心不会在启动日志中输出密码。
- 安装版核心、设置及日志位于安装目录的 `runtime` 和 `data/users/<Windows SID>`。
- 原生卸载器、Windows 卸载入口和开始菜单快捷方式；卸载清理本安装的数据，保留共享驱动。
- 安装与卸载共用维护锁；校验产品身份、清理路径和重解析点；清理失败保留重试入口。

## 版本与来源

Windows GUI / 安装包的公开版本号从 **0.3.1** 开始，与上游最新 Release 编号对齐。本次 Release 标记使用 **windows-v0.3.2**，避免与保留的上游 `v0.3.1` 标记混淆。

内嵌认证核心基于上游提交 `ef20ae5c71744eb9e096f5e586713490ba01f4ee`（核心自身版本 0.3.2），仅加入环境变量凭据和移除密码日志两项补丁。核心保持真实版本号，与 GUI 的发行版本独立；见 `drcom4scut_0.3.1/vendor/drcom4scut-0.3.2/PATCHES.md`。

源码已同步到本仓库，[windows-v0.3.2](https://github.com/LuoMuQAQ/drcom4scut/tree/windows-v0.3.2) 标签对应本次二进制。Release 仅上传安装包和便携 ZIP，不额外上传源码包。

## 构建

要求 Windows x64、Rust `nightly-2026-09-06-x86_64-pc-windows-gnu`、GNU MinGW `windres` / `ar` 在 PATH 中。构建输出目录需使用纯 ASCII 路径。

```powershell
cd drcom4scut-rs
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'drcom4scut-rs-target'
cargo +nightly-2026-09-06-x86_64-pc-windows-gnu test --target x86_64-pc-windows-gnu
cargo +nightly-2026-09-06-x86_64-pc-windows-gnu fmt --all -- --check
powershell -NoProfile -ExecutionPolicy Bypass -File .\publish-setup.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\publish-portable.ps1
```

临时目录含非 ASCII 字符时，向发布脚本传入 `-BuildDirectory C:\drcom-build -PayloadDirectory C:\drcom-payload`。安装包在 `drcom4scut-rs/release/setup`，GUI 和便携 ZIP 在 `drcom4scut-rs/release`。内嵌核心的对应源码和可复现构建说明见 `drcom4scut-rs/BUILD.txt`。

验证：166 项自动测试通过；网络测试和离屏性能导出测试共 2 项默认忽略，后者已单独运行通过。100%～200% 缩放的动画局部重绘与整窗结果逐像素一致，运行时缩放切换、工作区恢复及输入内容/选择/撤销保留已通过回归测试。实际登录自启、UAC、驱动安装和断线恢复仍依赖运行环境；发行 EXE 未代码签名。

## 许可证

GUI / 安装器按 GPL-3.0-or-later 分发，见 `drcom4scut-rs/LICENSE`。保留上游原始 LICENSE；上游 Cargo 声明 GPL-3.0-or-later，而 LICENSE 文件为 LGPL-3.0，原始声明与文件均保留，当前组合发行按 GPL-3.0-or-later 处理。Lucide 图标使用 ISC；详见 `drcom4scut-rs/resources/licenses/NOTICE.txt`。
