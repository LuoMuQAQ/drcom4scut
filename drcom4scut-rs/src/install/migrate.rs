//! 将当前用户可解密的旧 LocalAppData 设置迁入安装目录。不删除共享旧目录。

use std::path::Path;

use crate::settings::{self, SettingsError};

#[derive(Debug)]
pub struct MigrationReport {
    pub settings_copied: bool,
    pub config_copied: bool,
    pub source_retained: bool,
    pub message: String,
}

pub fn migrate_user_data(
    legacy_root: &Path,
    dest_root: &Path,
) -> Result<MigrationReport, SettingsError> {
    std::fs::create_dir_all(dest_root.join("config"))
        .map_err(|e| SettingsError::new(format!("无法创建用户数据目录：{e}")))?;
    std::fs::create_dir_all(dest_root.join("logs"))
        .map_err(|e| SettingsError::new(format!("无法创建日志目录：{e}")))?;

    let dest_settings = dest_root.join("settings.v1.dat");
    let src_settings = legacy_root.join("settings.v1.dat");
    let mut settings_copied = false;
    if !dest_settings.exists() && src_settings.is_file() {
        std::fs::copy(&src_settings, &dest_settings)
            .map_err(|e| SettingsError::new(format!("复制旧设置失败：{e}")))?;
        match settings::load_from(&dest_settings) {
            Ok(_) => settings_copied = true,
            Err(e) => {
                let _ = std::fs::remove_file(&dest_settings);
                return Err(SettingsError::new(format!(
                    "旧设置无法在当前用户下解密，已保留源文件。原因：{}",
                    e.message()
                )));
            }
        }
    }

    let dest_config = dest_root.join("config").join("config.yml");
    let src_config = legacy_root.join("config").join("config.yml");
    let mut config_copied = false;
    if !dest_config.exists() && src_config.is_file() {
        std::fs::copy(&src_config, &dest_config)
            .map_err(|e| SettingsError::new(format!("复制旧配置失败：{e}")))?;
        config_copied = true;
    }

    Ok(MigrationReport {
        settings_copied,
        config_copied,
        source_retained: true,
        message: "旧 %LOCALAPPDATA% 数据可能与其他客户端共用，已保留源文件。".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Settings;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "drcom-mig-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn copies_and_verifies_and_keeps_source() {
        let src = tmp("src");
        let dst = tmp("dst");
        std::fs::create_dir_all(src.join("config")).unwrap();
        let mut s = Settings::default();
        s.username = "123456".into();
        s.password = "secret".into();
        s.remember_password = true;
        settings::save_to(&src.join("settings.v1.dat"), &s).unwrap();
        std::fs::write(src.join("config").join("config.yml"), b"device: ''\n").unwrap();

        let report = migrate_user_data(&src, &dst).unwrap();
        assert!(report.settings_copied);
        assert!(report.config_copied);
        assert!(report.source_retained);
        assert!(src.join("settings.v1.dat").is_file());
        let loaded = settings::load_from(&dst.join("settings.v1.dat")).unwrap();
        assert_eq!(loaded.username, "123456");
        assert_eq!(loaded.password, "secret");

        let report2 = migrate_user_data(&src, &dst).unwrap();
        assert!(!report2.settings_copied, "已存在时不应覆盖");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn failed_decrypt_keeps_source_and_does_not_leave_bad_dest() {
        let src = tmp("bad");
        let dst = tmp("bad-dst");
        std::fs::write(src.join("settings.v1.dat"), b"not-dpapi").unwrap();
        let err = migrate_user_data(&src, &dst).unwrap_err();
        assert!(err.message().contains("保留源文件") || err.message().contains("解密"));
        assert!(src.join("settings.v1.dat").is_file());
        assert!(!dst.join("settings.v1.dat").exists());
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }
}
