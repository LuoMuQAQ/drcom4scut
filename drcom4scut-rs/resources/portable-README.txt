drcom4scut Windows GUI @VERSION@ 便携版（Windows x64）

使用方法
1. 将整个 ZIP 解压到独立文件夹，不要直接从压缩软件中运行。
2. 双击 drcom4scutGUI.exe，并确认 Windows 管理员授权。
3. 填写学号和密码，选择网卡或使用自动选择，然后点击连接。
4. 默认关闭窗口会退出；开启“保留系统托盘”后，关闭窗口隐藏到托盘，可从托盘菜单退出。

升级时先退出正在运行的客户端，再使用新版。客户端采用单实例机制，旧版仍运行时，启动新版会唤起旧版窗口。

驱动要求
需要系统已安装兼容的 x64 Npcap / WinPcap。没有驱动时，客户端会提示打开官方页面：
https://npcap.com/#download
安装 Npcap 时启用 WinPcap API 兼容模式，完成后重新打开客户端。
本 ZIP 不包含驱动安装包。如需驱动检测及下载引导，可使用同一 Release 的 Setup 安装版。

便携模式与数据
本包免安装，不创建 Windows 卸载登记，也不包含卸载器。
认证核心嵌入 EXE，运行时自动释放。
设置、日志和核心缓存位于 %LOCALAPPDATA%\drcom4scutGUI，不会随解压目录移动。
密码由当前 Windows 用户的 DPAPI 加密；不保证跨账户迁移可用。
请在独立文件夹中运行，不要放入带 install-state.json 的已安装客户端目录。
开启“开机启动”后，登录时在托盘后台启动，不弹出主窗口。

移除
如开启过“开机启动”，先在程序中关闭该选项，再退出客户端并删除解压文件夹。
如需清理数据，退出所有使用该数据目录的便携客户端后，可手动删除 %LOCALAPPDATA%\drcom4scutGUI。
多个便携副本可能共享此目录；删除它会清除保存的设置与日志。无需卸载共享 Npcap 驱动。

版本与源码
GUI / 安装版版本：@VERSION@。包含静默自启及开关、网卡菜单动画修复。
认证核心版本：patched 0.3.2，来源于 SeaLoong/drcom4scut 提交 ef20ae5c71744eb9e096f5e586713490ba01f4ee。
对应源码：https://github.com/LuoMuQAQ/drcom4scut/tree/windows-v@VERSION@
下载页面：https://github.com/LuoMuQAQ/drcom4scut/releases/tag/windows-v@VERSION@
许可证见 licenses 文件夹，文件校验值见 SHA256SUMS.txt。EXE 未代码签名。
