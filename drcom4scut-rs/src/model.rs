//! 与 UI 框架无关的共享数据模型。
//!
//! 字段与取值必须与现有 .NET 版 v3.3.0 保持一致，以便迁移既有设置文件。

use serde::{Deserialize, Serialize};

/// 连接状态。与 `src/MainWindow.xaml.cs:413-422` 的色板一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Offline,
    Connecting,
    Online,
    Waiting,
    Degraded,
    Error,
}

impl Default for LinkState {
    fn default() -> Self {
        LinkState::Offline
    }
}

/// 持久化设置。字段顺序与 JSON 名不得更改：既有 `settings.v1.dat` 依赖 camelCase 名称。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub mac: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub auto_login: bool,
    /// 旧设置文件缺少该字段时视为 true（兼容旧版 gui.json）。
    #[serde(default = "default_true")]
    pub remember_password: bool,
    #[serde(default)]
    pub adapter_id: String,
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default)]
    pub minimize_to_tray: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            mac: String::new(),
            username: String::new(),
            password: String::new(),
            auto_login: false,
            remember_password: true,
            adapter_id: String::new(),
            start_with_windows: false,
            minimize_to_tray: false,
        }
    }
}

impl Settings {
    /// 落盘前的脱敏：关闭“保留密码”时只清空密码，其余字段原样保留。
    /// 对应 .NET 版 `Services.cs:25-39` 的 `ForPersistence`。
    pub fn for_persistence(&self) -> Settings {
        if self.remember_password {
            self.clone()
        } else {
            Settings {
                password: String::new(),
                ..self.clone()
            }
        }
    }
}

/// 网卡条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    /// 稳定标识（.NET 版用适配器的 Id 字符串）。
    pub id: String,
    /// 规范化后的 MAC，形如 `cc:28:aa:ba:aa:5b`；无网卡时为空串。
    pub mac: String,
    pub name: String,
    /// 是否处于 Up 状态。
    pub is_up: bool,
    /// 是否配置了 IPv4 网关。
    pub has_ipv4_gateway: bool,
}

impl Adapter {
    /// 下拉框展示名。
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            self.id.clone()
        } else {
            self.name.clone()
        }
    }
}

/// 归一化 MAC：去分隔符、转小写、补零。输入 `CC-28-AA-BA-AA-5B` 输出 `cc:28:aa:ba:aa:5b`。
pub fn normalize_mac(raw: &str) -> String {
    let hex: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() % 2 != 0 {
        return String::new();
    }
    let bytes: Vec<String> = hex
        .as_bytes()
        .chunks(2)
        .map(|c| String::from_utf8_lossy(c).to_ascii_lowercase())
        .collect();
    bytes.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_password_defaults_to_true_for_legacy_files() {
        // 旧设置文件缺少 rememberPassword 字段时必须视为开启。
        let json = r#"{"username":"u","password":"p"}"#;
        let s: Settings = serde_json::from_str(json).expect("parse");
        assert!(s.remember_password);
        assert!(!s.auto_login);
        assert!(!s.minimize_to_tray);
    }

    #[test]
    fn serialize_uses_camel_case() {
        let s = Settings::default();
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("rememberPassword").is_some());
        assert!(v.get("autoLogin").is_some());
        assert!(v.get("startWithWindows").is_some());
        assert!(v.get("minimizeToTray").is_some());
    }

    #[test]
    fn for_persistence_keeps_password_when_remembered() {
        let s = Settings {
            password: "secret".into(),
            remember_password: true,
            ..Default::default()
        };
        assert_eq!(s.for_persistence().password, "secret");
    }

    #[test]
    fn for_persistence_clears_password_when_not_remembered() {
        let s = Settings {
            username: "u".into(),
            password: "secret".into(),
            remember_password: false,
            auto_login: true,
            ..Default::default()
        };
        let p = s.for_persistence();
        assert_eq!(p.password, "");
        assert!(!p.remember_password);
        // 其余字段必须逐字段保留
        assert_eq!(p.username, "u");
        assert!(p.auto_login);
    }

    #[test]
    fn normalize_mac_variants() {
        assert_eq!(normalize_mac("CC-28-AA-BA-AA-5B"), "cc:28:aa:ba:aa:5b");
        assert_eq!(normalize_mac("cc:28:aa:ba:aa:5b"), "cc:28:aa:ba:aa:5b");
        assert_eq!(normalize_mac(""), "");
        assert_eq!(normalize_mac("abc"), "");
    }
}
