//! Rust ↔ 前端受控版本化通信协议（v1）。
//! 严格白名单校验、无明文密码泄露、独立主题持久化、强类型边界。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PROTOCOL_VERSION: u32 = 1;

/// 前端发给 Rust 的命令（受控白名单）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum FrontendCommand {
    /// 前端初始化就绪，请求全量快照
    #[serde(rename_all = "camelCase")]
    Init { client_version: String },
    /// 切换主题模式（"system" | "light" | "dark"）
    #[serde(rename_all = "camelCase")]
    SetTheme { mode: String },
    /// 标题栏窗口操作（"minimize" | "close"）
    #[serde(rename_all = "camelCase")]
    WindowAction { action: String },

    // 主窗口业务命令
    /// 发起认证连接
    Connect,
    /// 断开连接
    Disconnect,
    /// 更新常规设置（绝无明文密码在此流转）
    #[serde(rename_all = "camelCase")]
    UpdateSettings {
        username: Option<String>,
        password: Option<String>, // 仅前端主动修改输入时单向提交给 Rust DPAPI，Rust 从不回传明文
        auto_connect: Option<bool>,
        remember_password: Option<bool>,
        startup: Option<bool>,
        tray_keep: Option<bool>,
    },
    /// 选择网卡
    #[serde(rename_all = "camelCase")]
    SelectAdapter { id: String },

    // 安装向导命令
    /// 弹出原生文件夹选择框选择安装路径
    BrowseInstallDir,
    /// 设置安装目录输入文本
    #[serde(rename_all = "camelCase")]
    SetInstallDir { path: String },
    /// 设置安装选项
    #[serde(rename_all = "camelCase")]
    SetInstallOption { key: String, value: bool },
    /// 开始安装
    StartInstall,
    /// 取消安装
    CancelInstall,

    // 卸载命令
    /// 确认卸载
    ConfirmUninstall,
    /// 取消卸载退出
    CancelUninstall,
}

/// Rust 回传给前端的状态事件
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum HostEvent {
    /// 初始全量状态快照
    InitSnapshot(Box<InitSnapshotPayload>),
    /// 连接状态与诊断信息更新
    StatusChanged {
        state: String, // "idle" | "connecting" | "connected" | "error"
        title: String,
        detail: String,
        indicator: String, // "idle" | "progress" | "success" | "danger"
    },
    /// 设置更新广播（注意：绝不回传密码明文）
    SettingsUpdated(SafeSettingsSnapshot),
    /// 网卡列表更新
    AdaptersUpdated {
        adapters: Vec<AdapterInfo>,
        selected_id: Option<String>,
    },
    /// 主题变动通知
    ThemeChanged {
        mode: String,  // "system" | "light" | "dark"
        actual: String, // "light" | "dark"
    },
    /// 安装向导目录校验结果
    InstallDirValidated {
        path: String,
        valid: bool,
        error: Option<String>,
    },
    /// 安装进度更新
    InstallProgress {
        percent: u32,
        message: String,
    },
    /// 安装完成/失败结果
    InstallFinished {
        ok: bool,
        message: String,
        need_reboot: bool,
    },
    /// 卸载进度/结果
    UninstallFinished {
        ok: bool,
        message: String,
    },
    /// 通用错误通知
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitSnapshotPayload {
    pub protocol_version: u32,
    pub window_type: String, // "main" | "setup" | "uninstall"
    pub theme_mode: String,
    pub actual_theme: String,
    pub main: Option<MainSnapshot>,
    pub setup: Option<SetupSnapshot>,
    pub uninstall: Option<UninstallSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MainSnapshot {
    pub settings: SafeSettingsSnapshot,
    pub adapters: Vec<AdapterInfo>,
    pub selected_adapter_id: Option<String>,
    pub status: StatusInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeSettingsSnapshot {
    pub username: String,
    pub has_password: bool, // 仅指示密码是否存在，绝对不输出明文
    pub auto_connect: bool,
    pub remember_password: bool,
    pub startup: bool,
    pub tray_keep: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub mac: String,
    pub is_up: bool,
    pub has_gateway: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusInfo {
    pub state: String,
    pub title: String,
    pub detail: String,
    pub indicator: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupSnapshot {
    pub default_dir: String,
    pub desktop_shortcut: bool,
    pub start_menu_shortcut: bool,
    pub launch_after: bool,
    pub driver_status: String,
    pub driver_detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallSnapshot {
    pub install_dir: String,
    pub version: String,
    pub is_elevated: bool,
}

/// 主题持久化存储配置（按用户独立存储，不碰 settings.v1.dat）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub mode: String, // "system" | "light" | "dark"
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mode: "system".into(),
        }
    }
}

impl ThemeConfig {
    pub fn load_for_current_user() -> Self {
        let path = theme_config_path();
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(cfg) = serde_json::from_slice::<ThemeConfig>(&bytes) {
                if ["system", "light", "dark"].contains(&cfg.mode.as_str()) {
                    return cfg;
                }
            }
        }
        Self::default()
    }

    pub fn save_for_current_user(&self) -> Result<(), String> {
        let path = theme_config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// 解析当前生效的实际主题（light 或 dark）
    pub fn resolve_actual(&self) -> String {
        match self.mode.as_str() {
            "light" => "light".into(),
            "dark" => "dark".into(),
            _ => {
                if is_system_dark_mode() {
                    "dark".into()
                } else {
                    "light".into()
                }
            }
        }
    }
}

fn theme_config_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("drcom4scut")
        .join("theme.json")
}

/// 读取 Windows 当前系统是否处于深色模式
pub fn is_system_dark_mode() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    };
    unsafe {
        let mut key = HKEY::default();
        let subkey = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_READ, &mut key).is_err() {
            return false;
        }
        let mut value = 0u32;
        let mut size = 4u32;
        let mut kind = REG_DWORD;
        let val_name = w!("AppsUseLightTheme");
        let res = RegQueryValueExW(
            key,
            val_name,
            None,
            Some(&mut kind),
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if res.is_ok() && kind == REG_DWORD {
            value == 0 // 0 为深色模式，1 为浅色模式
        } else {
            false
        }
    }
}

/// 解析前端命令并进行强类型与白名单安全校验
pub fn parse_command(json_str: &str) -> Result<FrontendCommand, String> {
    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("无效的 JSON 格式: {e}"))?;

    // 白名单字段检查：必须含有有效 type 且为已知命令
    let cmd_type = parsed.get("type").and_then(|t| t.as_str())
        .ok_or_else(|| "缺失 type 字段".to_string())?;

    const ALLOWED_TYPES: &[&str] = &[
        "init", "setTheme", "windowAction", "connect", "disconnect",
        "updateSettings", "selectAdapter", "browseInstallDir", "setInstallDir",
        "setInstallOption", "startInstall", "cancelInstall", "confirmUninstall",
        "cancelUninstall"
    ];

    if !ALLOWED_TYPES.contains(&cmd_type) {
        return Err(format!("未授权或未知的命令: {cmd_type}"));
    }

    serde_json::from_value(parsed)
        .map_err(|e| format!("命令负载格式校验失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_commands() {
        let json = r#"{"type":"init","payload":{"clientVersion":"1.0.0"}}"#;
        assert!(matches!(parse_command(json), Ok(FrontendCommand::Init { .. })));

        let json = r#"{"type":"setTheme","payload":{"mode":"dark"}}"#;
        assert_eq!(
            parse_command(json).unwrap(),
            FrontendCommand::SetTheme { mode: "dark".into() }
        );

        let json = r#"{"type":"connect"}"#;
        assert_eq!(parse_command(json).unwrap(), FrontendCommand::Connect);
    }

    #[test]
    fn reject_unknown_or_malformed_commands() {
        let malformed = r#"{"type":"unknownCommand"}"#;
        assert!(parse_command(malformed).is_err());

        let prototype_pollution = r#"{"type":"__proto__","payload":{}}"#;
        assert!(parse_command(prototype_pollution).is_err());

        let bad_json = r#"{"type":}"#;
        assert!(parse_command(bad_json).is_err());
    }

    #[test]
    fn settings_snapshot_never_serializes_password_plaintext() {
        let snapshot = SafeSettingsSnapshot {
            username: "20240001".into(),
            has_password: true,
            auto_connect: false,
            remember_password: true,
            startup: false,
            tray_keep: true,
        };
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains("password\""));
        assert!(serialized.contains("hasPassword\":true"));
    }

    #[test]
    fn frontend_html_contains_no_external_cdns() {
        const HTML: &str = include_str!("../../frontend/dist/index.html");
        assert!(!HTML.is_empty(), "前端静态单文件不可为空");
        assert!(HTML.contains("<html"), "前端 HTML 必须包含 html 标签");
        assert!(HTML.contains("id=\"root\""), "前端 HTML 必须包含 root 节点");
        
        // 关键安全要求：严禁引用任何外部 CDN
        assert!(!HTML.contains("https://cdn."), "严禁包含外部 CDN");
        assert!(!HTML.contains("http://cdn."), "严禁包含外部 CDN");
        assert!(!HTML.contains("unpkg.com"), "严禁包含 unpkg 外部脚本");
        assert!(!HTML.contains("jsdelivr.net"), "严禁包含 jsdelivr 外部脚本");
        assert!(!HTML.contains("cdnjs.cloudflare.com"), "严禁包含 cdnjs 外部脚本");
    }

    #[test]
    fn path_safety_rejects_dangerous_paths() {
        use crate::install::flow::{validate_options, InstallOptions};
        let system_root = PathBuf::from(r"C:\Windows");
        
        // 尝试设置为 C:\Windows 敏感目录
        let mut opts = InstallOptions {
            install_dir: PathBuf::from(r"C:\Windows"),
            ..Default::default()
        };
        assert!(validate_options(&opts, &system_root, None).is_err());

        // 尝试设置为盘符根目录
        opts.install_dir = PathBuf::from(r"C:\");
        assert!(validate_options(&opts, &system_root, None).is_err());

        // 尝试相对路径
        opts.install_dir = PathBuf::from(r"..\Program Files\drcom");
        assert!(validate_options(&opts, &system_root, None).is_err());
    }
}
