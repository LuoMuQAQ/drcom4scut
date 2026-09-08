//! DPAPI 加密设置存储。
//!
//! 行为对齐 .NET 版 `Services.cs:158-241` 的 `DpapiSettingsProtector` 与 `SettingsStore`：
//! - CurrentUser 作用域的 DPAPI（`CryptProtectData` / `CryptUnprotectData`）；
//! - 附加熵固定为字节串 `drcom4scutGUI/settings.v1`，标志位 `CRYPTPROTECT_UI_FORBIDDEN`；
//! - 同目录临时文件 + rename 的原子写，写入后读回解密并做常量时间校验；
//! - 可从旧版明文 `gui.json` 迁移，迁移成功后删除明文源文件，失败时保留明文。
//!
//! 核心函数全部显式接收 `&Path` 以便注入临时目录做单元测试；`save` / `load` /
//! `migrate_legacy` 只是基于 `crate::paths` 的薄封装。

use std::fmt;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use crate::model::Settings;

/// DPAPI 附加熵。与 .NET 版 `Services.cs:166` 完全一致，否则解不开既有 `settings.v1.dat`。
const ENTROPY: &[u8] = b"drcom4scutGUI/settings.v1";

/// 用户可读的设置存储错误。消息面向最终用户展示，具体原因附在末尾便于排查。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsError {
    message: String,
}

impl SettingsError {
    pub fn new(message: impl Into<String>) -> Self {
        SettingsError {
            message: message.into(),
        }
    }

    /// 面向用户展示的消息文本。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SettingsError {}

/// 把设置加密写入默认位置（`paths::settings_file()`）。
pub fn save(settings: &Settings) -> Result<(), SettingsError> {
    save_to(&settings_path()?, settings)
}

/// 从默认位置读取设置；文件不存在时返回默认设置。
pub fn load() -> Result<Settings, SettingsError> {
    load_from(&settings_path()?)
}

/// 从旧版明文 `gui.json` 迁移到默认加密位置（基于 `paths::settings_file()` 与
/// `paths::legacy_gui_json()`）。
pub fn migrate_legacy() -> Result<Settings, SettingsError> {
    let settings_path = settings_path()?;
    let legacy_path = crate::paths::legacy_gui_json()
        .ok_or_else(|| SettingsError::new("无法确定旧设置文件路径。"))?;
    migrate_legacy_with(&settings_path, &legacy_path)
}

/// 把设置加密写入 `path`。流程：脱敏（for_persistence）→ JSON 序列化 → DPAPI 加密
/// → 原子写 → 读回解密 → 常量时间比对校验。
///
/// 对应 .NET 版 `SettingsStore.Save`（`Services.cs:232-241`）。
pub fn save_to(path: &Path, settings: &Settings) -> Result<(), SettingsError> {
    let json = serde_json::to_vec(&settings.for_persistence())
        .map_err(|e| SettingsError::new(format!("设置序列化失败：{e}")))?;
    let encrypted = dpapi_protect(&json)?;
    atomic_write(path, &encrypted)
        .map_err(|e| SettingsError::new(format!("写入设置文件失败：{e}")))?;

    // 写入后读回解密，与原始明文做常量时间比对，防止静默损坏。
    let read_back =
        std::fs::read(path).map_err(|e| SettingsError::new(format!("写入后读回设置失败：{e}")))?;
    let round_trip = dpapi_unprotect(&read_back)?;
    if !constant_time_eq(&json, &round_trip) {
        return Err(SettingsError::new("受保护设置写入校验失败。"));
    }
    Ok(())
}

/// 从 `path` 读取并解密设置。文件不存在返回 `Settings::default()`；
/// 任何失败都包装成统一的用户可读错误。
///
/// 对应 .NET 版 `SettingsStore.Load`（`Services.cs:217-230`）。
pub fn load_from(path: &Path) -> Result<Settings, SettingsError> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    let loaded = (|| -> Result<Settings, SettingsError> {
        let protected = std::fs::read(path)
            .map_err(|e| SettingsError::new(format!("读取设置文件失败：{e}")))?;
        let json = dpapi_unprotect(&protected)?;
        serde_json::from_slice(&json).map_err(|e| SettingsError::new(format!("设置内容无效：{e}")))
    })();
    loaded.map_err(|e| {
        SettingsError::new(format!(
            "无法读取受保护的设置，请确认当前 Windows 用户未变更。原因：{e}"
        ))
    })
}

/// 迁移旧版明文设置：若 `legacy_path` 存在且 `settings_path` 不存在，读旧 JSON
/// → 加密保存 → 读回验证一致后删除明文源文件；任何一步失败都保留明文源文件，
/// 并返回含「原文件已保留」的错误。
///
/// 对应 .NET 版 `SettingsStore.LoadOrMigrate`（`Services.cs:192-215`）。
pub fn migrate_legacy_with(
    settings_path: &Path,
    legacy_path: &Path,
) -> Result<Settings, SettingsError> {
    if settings_path.exists() {
        return load_from(settings_path);
    }
    if !legacy_path.exists() {
        return Ok(Settings::default());
    }

    let migrated = (|| -> Result<Settings, SettingsError> {
        let raw = std::fs::read(legacy_path)
            .map_err(|e| SettingsError::new(format!("读取旧设置失败：{e}")))?;
        // 旧 JSON 为 camelCase；缺 rememberPassword 时由 model 的 serde default 视为 true。
        let legacy: Settings = serde_json::from_slice(&raw)
            .map_err(|e| SettingsError::new(format!("旧设置文件格式无效：{e}")))?;
        save_to(settings_path, &legacy)?;
        let verified = load_from(settings_path)?;
        // save_to 会应用 for_persistence 脱敏，因此与脱敏后的结果比对，
        // 避免「不保留密码的旧文件带密码」导致迁移永远失败。
        if verified != legacy.for_persistence() {
            return Err(SettingsError::new("加密设置写入校验失败。"));
        }
        Ok(verified)
    })();

    match migrated {
        Ok(settings) => {
            std::fs::remove_file(legacy_path).map_err(|e| {
                SettingsError::new(format!(
                    "无法迁移旧设置。原文件已保留在 {}。请检查文件权限后重试。原因：删除明文文件失败：{e}",
                    legacy_path.display()
                ))
            })?;
            Ok(settings)
        }
        Err(e) => Err(SettingsError::new(format!(
            "无法迁移旧设置。原文件已保留在 {}。请检查文件权限后重试。原因：{e}",
            legacy_path.display()
        ))),
    }
}

/// DPAPI 加密（CurrentUser 作用域）。空明文同样是合法数据（等价 .NET 允许空字节数组）。
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, SettingsError> {
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input/entropy 两个 blob 的缓冲区在调用期间有效且只读；输出 blob 由
    // API 分配，读取拷贝后立即用 LocalFree 释放；标志位禁止弹 UI。
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            // 空明文时 as_ptr 为对齐的悬垂指针，cbData=0 下 API 不会解引用（.NET 语义相同）。
            pbData: plain.as_ptr() as *mut u8,
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        CryptProtectData(
            &input,
            PCWSTR::null(),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| SettingsError::new(format!("DPAPI 加密失败：{e}")))?;

        let protected = copy_blob(&output);
        LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(protected)
    }
}

/// DPAPI 解密（CurrentUser 作用域，附加熵与加密时一致）。
fn dpapi_unprotect(protected: &[u8]) -> Result<Vec<u8>, SettingsError> {
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input/entropy 两个 blob 的缓冲区在调用期间有效且只读；输出 blob 由
    // API 分配，读取拷贝后立即用 LocalFree 释放；描述符输出指针传 None 不接收。
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: protected.len() as u32,
            pbData: protected.as_ptr() as *mut u8,
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| SettingsError::new(format!("DPAPI 解密失败：{e}")))?;

        let plain = copy_blob(&output);
        LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(plain)
    }
}

/// 拷贝 DPAPI 输出 blob（cbData 可能为 0，且空时指针可能为空，不能直接切 slice）。
fn copy_blob(blob: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if blob.cbData > 0 && !blob.pbData.is_null() {
        // SAFETY: API 刚分配的缓冲区，cbData 即长度；拷贝后由调用处 LocalFree。
        unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec() }
    } else {
        Vec::new()
    }
}

/// 原子写：同目录临时文件（`.{文件名}.{随机}.tmp`）→ 写入 + flush + sync → rename 覆盖。
///
/// 对应 .NET 版 `CoreResources.AtomicWrite`（`Services.cs:120-139`）。
fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "目标路径无效。",
            ))
        }
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "目标路径无效。"))?;
    std::fs::create_dir_all(dir)?;

    // 临时文件名冲突时换名重试（对应 .NET 的 Guid 命名，此处用时间+计数+pid）。
    let mut last_conflict = None;
    for _ in 0..8 {
        let temp = dir.join(format!(".{}.{}.tmp", name.to_string_lossy(), temp_suffix()));
        match write_and_rename(&temp, path, data) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last_conflict = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last_conflict.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::AlreadyExists, "无法生成唯一的临时文件名。")
    }))
}

/// 写临时文件并 rename 到目标；任何失败都清理临时文件（对应 .NET 的 finally 清理）。
fn write_and_rename(temp: &Path, target: &Path, data: &[u8]) -> io::Result<()> {
    let result = (|| -> io::Result<()> {
        // CreateNew 语义：临时文件已存在即失败。
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temp)?;
        file.write_all(data)?;
        file.flush()?;
        // 对应 .NET Flush(true)：连同磁盘缓冲一并落盘后再 rename。
        file.sync_all()?;
        drop(file);
        // Windows 上 rename 带替换语义（MOVEFILE_REPLACE_EXISTING），等价 File.Move(..., true)。
        std::fs::rename(temp, target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

/// 生成进程内唯一的临时文件后缀（等价 .NET 的 `Guid.NewGuid():N`）。
fn temp_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:04x}-{}", nanos, counter, std::process::id())
}

/// 常量时间字节比较；长度不同立即返回 false（与 .NET `FixedTimeEquals` 一致）。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn settings_path() -> Result<PathBuf, SettingsError> {
    crate::paths::settings_file().ok_or_else(|| SettingsError::new("无法确定设置文件路径。"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试独享一个 `%TEMP%` 下的随机子目录，结束自动清理，绝不触碰真实 %LOCALAPPDATA%。
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "drcom4scut-gui-tests-{}-{}",
                tag,
                temp_suffix()
            ));
            std::fs::create_dir_all(&dir).expect("创建临时目录失败");
            TempDir(dir)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 与 .NET ServicesTests.SampleSettings 同款样例数据。
    fn sample_settings() -> Settings {
        Settings {
            mac: "00:11:22:33:44:55".into(),
            username: "synthetic-user".into(),
            password: "synthetic-password-42".into(),
            auto_login: true,
            remember_password: true,
            adapter_id: "adapter-id".into(),
            start_with_windows: true,
            minimize_to_tray: false,
        }
    }

    /// 二进制安全的子串判断（密文不是合法 UTF-8，不能用字符串查找）。
    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn save_load_roundtrip_keeps_no_plaintext_at_rest() {
        let dir = TempDir::new("roundtrip");
        let path = dir.join("settings.v1.dat");
        let settings = sample_settings();

        save_to(&path, &settings).expect("保存失败");
        let loaded = load_from(&path).expect("读取失败");

        assert_eq!(loaded, settings);
        let raw = std::fs::read(&path).expect("读回失败");
        assert!(
            !contains(&raw, b"synthetic-password-42"),
            "文件字节中出现了密码明文"
        );
        assert!(
            !contains(&raw, b"synthetic-user"),
            "文件字节中出现了用户名明文"
        );
        // 成功保存后目录里不应残留 .tmp 临时文件。
        let leftovers: Vec<_> = std::fs::read_dir(&dir.0)
            .expect("列出临时目录失败")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件：{leftovers:?}");
    }

    #[test]
    fn save_applies_for_persistence_before_writing() {
        let dir = TempDir::new("persistence");
        let path = dir.join("settings.v1.dat");
        let settings = Settings {
            username: "synthetic-user".into(),
            password: "synthetic-password-42".into(),
            remember_password: false,
            auto_login: true,
            ..Settings::default()
        };

        save_to(&path, &settings).expect("保存失败");
        let loaded = load_from(&path).expect("读取失败");

        assert_eq!(loaded.password, "");
        assert!(!loaded.remember_password);
        assert_eq!(loaded.username, "synthetic-user");
        assert!(loaded.auto_login);
        let encrypted = std::fs::read(&path).expect("读回失败");
        assert!(!contains(&encrypted, b"synthetic-password-42"));
    }

    #[test]
    fn dpapi_roundtrip_accepts_empty_plaintext() {
        // 空密码（空字符串）也是合法数据：CryptProtectData 对空输入必须仍产出合法密文。
        let protected = dpapi_protect(b"").expect("加密空明文失败");
        let plain = dpapi_unprotect(&protected).expect("解密空明文失败");
        assert!(plain.is_empty());
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let dir = TempDir::new("missing");
        let path = dir.join("absent.v1.dat");
        assert_eq!(
            load_from(&path).expect("文件不存在时应返回默认值"),
            Settings::default()
        );
    }

    #[test]
    fn load_corrupt_file_reports_user_readable_error() {
        let dir = TempDir::new("corrupt");
        let path = dir.join("settings.v1.dat");
        std::fs::write(&path, b"not-a-dpapi-blob").expect("写入失败");

        let error = load_from(&path).expect_err("损坏文件应报错");
        assert!(
            error.message().contains("无法读取受保护的设置"),
            "实际消息：{}",
            error.message()
        );
    }

    #[test]
    fn migration_verifies_then_deletes_plaintext_source() {
        let dir = TempDir::new("migrate-ok");
        let protected_path = dir.join("settings.v1.dat");
        let legacy_path = dir.join("gui.json");
        let settings = sample_settings();
        std::fs::write(&legacy_path, serde_json::to_vec(&settings).unwrap()).expect("写入失败");

        let migrated = migrate_legacy_with(&protected_path, &legacy_path).expect("迁移失败");

        assert_eq!(migrated, settings);
        assert!(protected_path.exists());
        assert!(!legacy_path.exists(), "迁移成功后明文源文件应被删除");
    }

    #[test]
    fn migration_defaults_missing_remember_password_to_true() {
        let dir = TempDir::new("migrate-legacy-fields");
        let protected_path = dir.join("settings.v1.dat");
        let legacy_path = dir.join("gui.json");
        std::fs::write(
            &legacy_path,
            br#"{"username":"synthetic-user","password":"synthetic-password-42"}"#,
        )
        .expect("写入失败");

        let migrated = migrate_legacy_with(&protected_path, &legacy_path).expect("迁移失败");

        assert_eq!(migrated.username, "synthetic-user");
        assert_eq!(migrated.password, "synthetic-password-42");
        assert!(
            migrated.remember_password,
            "缺少 rememberPassword 应视为 true"
        );
        assert!(!legacy_path.exists());
    }

    #[test]
    fn failed_migration_retains_plaintext_source() {
        let dir = TempDir::new("migrate-fail");
        // 注入写失败：让加密设置文件的父目录位置被一个普通文件占据，建目录必然失败。
        // （Windows 上目录只读属性不会阻止创建文件，故采用此法。）
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"occupied").expect("写入失败");
        let protected_path = blocker.join("settings.v1.dat");
        let legacy_path = dir.join("gui.json");
        std::fs::write(
            &legacy_path,
            serde_json::to_vec(&sample_settings()).unwrap(),
        )
        .expect("写入失败");

        let error =
            migrate_legacy_with(&protected_path, &legacy_path).expect_err("注入失败后迁移应报错");

        assert!(
            error.message().contains("原文件已保留"),
            "实际消息：{}",
            error.message()
        );
        assert!(legacy_path.exists(), "明文源文件必须保留");
        assert!(!protected_path.exists());
    }

    #[test]
    fn migrate_returns_defaults_when_no_files_exist() {
        let dir = TempDir::new("migrate-empty");
        let migrated = migrate_legacy_with(&dir.join("settings.v1.dat"), &dir.join("gui.json"))
            .expect("两个文件都不存在时应返回默认值");
        assert_eq!(migrated, Settings::default());
    }
}
