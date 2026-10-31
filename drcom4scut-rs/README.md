# drcom4scut Windows GUI 0.3.8

基于 [SeaLoong/drcom4scut](https://github.com/SeaLoong/drcom4scut) 的 Windows 原生图形客户端，使用 Rust / Win32 编写。本项目是独立维护的 GUI fork，感谢原作者提供认证核心。

## 下载和安装

前往 [Releases](https://github.com/LuoMuQAQ/drcom4scut/releases/latest)，下载 **drcom4scut-Setup-0.3.8.exe** 安装版。默认安装到 64 位 Program Files，可自行选择本地目录；已安装时在原目录覆盖升级。

也提供 **drcom4scut-Portable-0.3.8-win-x64.zip** 便携版，完整解压后运行 `drcom4scutGUI.exe`，需要系统已有兼容 x64 Npcap / WinPcap 驱动。便携模式数据位于 `%LOCALAPPDATA%/drcom4scutGUI`，不会随解压目录移动。

安装器自动检测兼容 x64 Npcap / WinPcap。缺少时从官网下载 Npcap 1.88，核验 SHA-256 与 Authenticode 后启动官方安装窗口。免费 Npcap 需要在官方窗口确认许可和安装，不能全静默安装；本项目不内嵌或再分发 Npcap 安装包。

## 终端安装与升级

在**管理员 PowerShell** 中执行。终端模式需要系统已有兼容的 x64 Npcap / WinPcap，缺少时返回错误，使用不带参数的图形安装器安装驱动即可。

```powershell
$p = Start-Process -FilePath .\drcom4scut-Setup-0.3.8.exe `
    -ArgumentList '--install --json' -PassThru `
    -RedirectStandardOutput .\install-result.json
$p.WaitForExit()
$p.ExitCode
Get-Content .\install-result.json
```

`--install`（或 `--silent`）不显示安装窗口，也不会弹出 UAC，需事先打开管理员终端。默认沿用已安装目录进行覆盖升级，保留用户设置和原有快捷方式选项；新安装默认位于 64 位 Program Files。升级时会停止旧客户端，网络可能短暂断开。

可追加 `--install-dir="D:\Campus Client"` 指定目录（已有安装时仍需使用原目录）；追加 `--launch` 在安装后静默启动客户端，自动连接按原“自动登录”设置执行。默认只安装。`--help` 查看帮助，`--version` 查询版本，`--json` 输出包含 `ok`、`version`、`installDir` 和 `exitCode` 的单条 JSON。

| 退出码 | 含义 |
| --- | --- |
| 0 | 安装成功 |
| 2 | 参数错误 |
| 740 | 需要管理员终端 |
| 1603 | 安装失败或缺少兼容驱动 |
| 1618 | 另一安装或卸载正在进行 |

不带参数运行安装包时仍打开图形安装界面。

## 功能

- HeroUI v3 风格的原生中文界面，保留原版 logo；主窗口、安装/卸载统一内嵌 Noto Sans SC 字体。
- 仅账号、密码输入框显示蓝色焦点框，点击空白处取消；连接按钮和输入框统一 12 DIP 圆角。
- UDP 每代独立接收认证通知，失效状态清除成功缓存，修复重连消息竞争。
- 开关滑动与网卡菜单淡入/滑入动画，采用局部重绘和菜单缓存，支持连续切换及高 DPI。
- 运行时切换分辨率、系统缩放或显示器时，窗口、字体、输入框、图标和点击区域同步适配；小工作区整体缩放，恢复后回到正常尺寸。
- 首次打开、托盘打开和恢复窗口时，先完成布局、主界面及原生子控件绘制，再统一显示，避免组件逐步出现。
- 客户端默认申请管理员权限，可配置以最高权限运行的登录自启计划任务；自启时静默后台运行，任务栏尚未就绪时持续重试托盘，不弹出主窗口或启动提示。双击托盘或手动打开程序后显示窗口。
- 核心内部自动恢复，外层提供停滞检测和退避；只有 UDP 心跳成功才判定完整连接健康，EAP 心跳无法掩盖 UDP 停滞。EAP 认证成功后界面显示已连接。正常心跳和夜间定时等待不会被周期重启。
- 关闭、断开或结束 GUI 进程时，核心先发送 EAPOL-Logoff 再退出；核心崩溃不会连带退出 GUI。
- 密码设置使用 Windows DPAPI 保护，凭据通过子进程环境传入核心，不放在命令行中；核心不会在启动日志中输出密码。
- 安装版核心、设置及日志位于安装目录的 `runtime` 和 `data/users/<Windows SID>`。
- 原生卸载器、Windows 卸载入口和开始菜单快捷方式；卸载清理本安装的数据，保留共享驱动。
- 安装与卸载共用维护锁；校验产品身份、清理路径和重解析点；清理失败保留重试入口。

## 版本与来源

Windows GUI / 安装包的公开版本号从 **0.3.1** 开始，与上游最新 Release 编号对齐。本次 Release 标记使用 **windows-v0.3.8**，避免与保留的上游 `v0.3.1` 标记混淆。

内嵌认证核心基于上游提交 `ef20ae5c71744eb9e096f5e586713490ba01f4ee`（核心自身版本 0.3.2），在上游之外加入环境变量凭据、移除密码日志，以及 GUI 退出时发送 EAPOL-Logoff 并随 GUI 结束。核心保持真实版本号，与 GUI 的发行版本独立；见 `drcom4scut_0.3.1/vendor/drcom4scut-0.3.2/PATCHES.md`。

本次 0.3.8 的对应源码为 [windows-v0.3.8](https://github.com/LuoMuQAQ/drcom4scut/tree/windows-v0.3.8)，可直接使用该 Release 的源码附件或 GitHub 自动生成的 Source code。

## 构建

要求 Windows x64、Rust `nightly-2026-09-06-x86_64-pc-windows-gnu`、GNU MinGW `windres` / `ar` 在 PATH 中。构建输出目录需使用纯 ASCII 路径。

```powershell
cd drcom4scut-rs
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'drcom4scut-rs-target'
cargo +nightly-2026-09-06-x86_64-pc-windows-gnu test --target x86_64-pc-windows-gnu --lib --bin drcom4scut-Setup --bin uninstall
cargo +nightly-2026-09-06-x86_64-pc-windows-gnu fmt --all -- --check
powershell -NoProfile -ExecutionPolicy Bypass -File .\publish-setup.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\publish-portable.ps1
```

临时目录含非 ASCII 字符时，向发布脚本传入 `-BuildDirectory C:\drcom-build -PayloadDirectory C:\drcom-payload`。安装包在 `drcom4scut-rs/release/setup`，GUI 和便携 ZIP 在 `drcom4scut-rs/release`。内嵌核心的对应源码和可复现构建说明见 `drcom4scut-rs/BUILD.txt`。

本轮验证：203 项 GUI/安装测试通过，5 项需显式运行的测试跳过；新增回归跟踪主窗口及所有可见控件的首帧绘制，覆盖深浅色、100% / 150% / 200% 缩放、首次及再次打开；保留终端安装、静默自启与托盘恢复测试。实际重启登录、真实校园网断线恢复、睡眠唤醒和长期稳定性仍待实机验证，Windows 10022 底层原因尚未确定。发行 EXE 未代码签名。

## 许可证

GUI / 安装器按 GPL-3.0-or-later 分发，见 `drcom4scut-rs/LICENSE`。保留上游原始 LICENSE；上游 Cargo 声明 GPL-3.0-or-later，而 LICENSE 文件为 LGPL-3.0，原始声明与文件均保留，当前组合发行按 GPL-3.0-or-later 处理。Noto Sans SC 字体使用 SIL OFL 1.1，Lucide 图标使用 ISC；详见 `drcom4scut-rs/resources/licenses/NOTICE.txt`。
