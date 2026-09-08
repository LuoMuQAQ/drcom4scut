//! Automatic dependency installation, independent of the setup window.
use super::driver::*;
use super::flow::InstallOptions;
use std::path::Path;

pub struct DriverProgress {
    pub percent: u32,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, path::PathBuf};
    struct Host {
        available: Cell<bool>,
        installed: bool,
        wpcap_only: bool,
        service: bool,
        downloads: Cell<usize>,
        runs: Cell<usize>,
        exit: i32,
        signed: bool,
        offline: bool,
    }
    impl Default for Host {
        fn default() -> Self {
            Self {
                available: Cell::new(false),
                installed: true,
                wpcap_only: false,
                service: true,
                downloads: Cell::new(0),
                runs: Cell::new(0),
                exit: 0,
                signed: true,
                offline: false,
            }
        }
    }
    impl DriverHost for Host {
        fn system_root(&self) -> PathBuf {
            PathBuf::from(r"C:\Windows")
        }
        fn registry_present(&self) -> bool {
            false
        }
        fn driver_service_present(&self, _: &Path) -> bool {
            self.service
        }
        fn probe_dll(&self, path: &Path) -> DllProbe {
            let present = self.available.get() && (!self.wpcap_only || path.ends_with("wpcap.dll"));
            DllProbe {
                path: path.into(),
                exists: present,
                is_file: present,
                under_system32: true,
                machine: Some(super::super::pe::IMAGE_FILE_MACHINE_AMD64),
                loaded: present,
                has_packet_exports: present,
            }
        }
        fn download(&self, _: &str, dest: &Path) -> Result<(), String> {
            self.downloads.set(self.downloads.get() + 1);
            if self.offline {
                return Err("offline".into());
            }
            std::fs::write(dest, b"test-payload").map_err(|e| e.to_string())
        }
        fn authenticode_publisher(&self, _: &Path) -> Result<String, String> {
            if self.signed {
                Ok("Authenticode-verified".into())
            } else {
                Err("invalid signature".into())
            }
        }
        fn run_installer(&self, path: &Path, args: &[String]) -> Result<i32, String> {
            assert!(path.is_file());
            assert!(!args.iter().any(|a| a == "/S"));
            self.runs.set(self.runs.get() + 1);
            self.available.set(self.installed);
            Ok(self.exit)
        }
    }
    fn exercise(
        host: &Host,
        hash: &str,
        cancelled: &dyn Fn() -> bool,
        report: &mut dyn FnMut(DriverProgress),
    ) -> (String, bool, String) {
        let cache = std::env::temp_dir().join(format!(
            "drcom-driver-test-{}",
            super::super::identity::new_install_id()
        ));
        let opts = InstallOptions {
            install_npcap_if_missing: false,
            ..Default::default()
        };
        let result = ensure_with_hash(host, &opts, &cache, hash, report, cancelled);
        assert!(
            !cache.join(NPCAP_FILE_NAME).exists(),
            "never leave executable downloads behind"
        );
        let _ = std::fs::remove_dir(&cache);
        result
    }
    fn run(host: &Host) -> (String, bool, String) {
        exercise(host, &sha256_hex(b"test-payload"), &|| false, &mut |_| {})
    }
    #[test]
    fn compatible_driver_skips_all_installation_work() {
        let h = Host {
            available: Cell::new(true),
            ..Default::default()
        };
        assert_eq!(run(&h).0, "available");
        assert_eq!(h.downloads.get(), 0);
        assert_eq!(h.runs.get(), 0);
    }
    #[test]
    fn wpcap_alone_and_orphan_packet_dll_are_not_ready() {
        let h = Host {
            available: Cell::new(true),
            wpcap_only: true,
            ..Default::default()
        };
        assert_ne!(detect_with(&h), DriverStatus::Available);
        let h = Host {
            available: Cell::new(true),
            service: false,
            ..Default::default()
        };
        assert_ne!(detect_with(&h), DriverStatus::Available);
    }
    #[test]
    fn missing_driver_is_automatic_and_rechecked_even_with_old_checkbox_false() {
        let h = Host::default();
        let mut progress = Vec::new();
        let result = exercise(&h, &sha256_hex(b"test-payload"), &|| false, &mut |p| {
            progress.push(p.percent)
        });
        assert_eq!(result.0, "installed");
        assert_eq!(h.downloads.get(), 1);
        assert_eq!(h.runs.get(), 1);
        assert!(progress.contains(&72) && progress.contains(&88) && progress.contains(&97));
    }
    #[test]
    fn successful_exit_without_working_driver_never_reports_ready() {
        let h = Host {
            installed: false,
            ..Default::default()
        };
        let result = run(&h);
        assert_eq!(result.0, "not-ready");
        assert!(!ready(&result.0));
    }
    #[test]
    fn hash_or_signature_failure_never_executes_download() {
        let h = Host::default();
        assert_eq!(
            exercise(&h, "bad-hash", &|| false, &mut |_| {}).0,
            "download-failed"
        );
        assert_eq!(h.runs.get(), 0);
        let h = Host {
            signed: false,
            ..Default::default()
        };
        assert_eq!(run(&h).0, "verification-failed");
        assert_eq!(h.runs.get(), 0);
    }
    #[test]
    fn offline_falls_back_then_offers_retry() {
        let h = Host {
            offline: true,
            ..Default::default()
        };
        let result = run(&h);
        assert_eq!(result.0, "download-failed");
        assert!(result.2.contains("重试驱动"));
        assert_eq!(h.downloads.get(), NPCAP_URLS.len());
        assert_eq!(h.runs.get(), 0);
    }
    #[test]
    fn cancellation_before_and_during_download_prevents_execution() {
        let h = Host::default();
        assert_eq!(
            exercise(&h, &sha256_hex(b"test-payload"), &|| true, &mut |_| {}).0,
            "cancelled"
        );
        assert_eq!(h.downloads.get(), 0);
        let cancelled = Cell::new(false);
        assert_eq!(
            exercise(
                &h,
                &sha256_hex(b"test-payload"),
                &|| cancelled.get(),
                &mut |p| {
                    if p.percent == 88 {
                        cancelled.set(true);
                    }
                }
            )
            .0,
            "cancelled"
        );
        assert_eq!(h.runs.get(), 0);
    }
    #[test]
    fn reboot_results_are_not_launchable_even_when_dll_is_present() {
        for code in [3010, 350] {
            let h = Host {
                exit: code,
                ..Default::default()
            };
            let result = run(&h);
            assert!(result.1);
            assert!(!ready(&result.0));
        }
    }
    #[test]
    fn installer_cancel_and_busy_remain_retryable() {
        for (exit, expected) in [
            (1223, "cancelled"),
            (1618, "busy"),
            (1633, "unsupported-os"),
        ] {
            let h = Host {
                exit,
                installed: false,
                ..Default::default()
            };
            assert_eq!(run(&h).0, expected);
        }
    }
}

pub fn ready(key: &str) -> bool {
    matches!(key, "available" | "installed")
}

pub fn ensure_driver<H: DriverHost>(
    host: &H,
    opts: &InstallOptions,
    cache: &Path,
    report: &mut dyn FnMut(DriverProgress),
    cancelled: &dyn Fn() -> bool,
) -> (String, bool, String) {
    ensure_with_hash(host, opts, cache, NPCAP_SHA256, report, cancelled)
}

fn ensure_with_hash<H: DriverHost>(
    host: &H,
    opts: &InstallOptions,
    cache: &Path,
    expected_hash: &str,
    report: &mut dyn FnMut(DriverProgress),
    cancelled: &dyn Fn() -> bool,
) -> (String, bool, String) {
    report(DriverProgress {
        percent: 70,
        message: "正在检测 Npcap / WinPcap 驱动…".into(),
    });
    if detect_with(host) == DriverStatus::Available {
        return (
            "available".into(),
            false,
            "已检测到兼容驱动，无需重复安装。".into(),
        );
    }
    let cancel = || {
        (
            "cancelled".into(),
            false,
            "应用文件已安装，驱动安装已取消。可稍后重试。".into(),
        )
    };
    if cancelled() {
        return cancel();
    }
    if let Err(e) = std::fs::create_dir_all(cache) {
        return (
            "download-failed".into(),
            false,
            format!("无法准备驱动下载目录：{e}"),
        );
    }
    let mut downloaded = false;
    let dest = cache.join(NPCAP_FILE_NAME);
    let (installer, hash, args) = if let Some(oem) = opts.oem_silent_npcap.as_deref() {
        let Some(hash) = opts.oem_sha256.as_deref() else {
            return (
                "oem-rejected".into(),
                false,
                "OEM 安装包缺少已核验的哈希。".into(),
            );
        };
        (oem, hash, oem_silent_args())
    } else {
        let mut error = String::new();
        for url in NPCAP_URLS {
            if cancelled() {
                let _ = std::fs::remove_file(&dest);
                return cancel();
            }
            let result = host.download_with_progress(url, &dest, &mut |received, total| {
                let percent = total
                    .filter(|n| *n > 0)
                    .map(|n| 72 + (received.saturating_mul(16) / n).min(16) as u32)
                    .unwrap_or(72);
                let amount = total
                    .map(|n| {
                        format!(
                            "{:.1} / {:.1} MB",
                            received as f64 / 1_000_000.0,
                            n as f64 / 1_000_000.0
                        )
                    })
                    .unwrap_or_else(|| format!("{:.1} MB", received as f64 / 1_000_000.0));
                report(DriverProgress {
                    percent,
                    message: format!("正在从官网下载 Npcap… {amount}"),
                });
                !cancelled()
            });
            if cancelled() {
                let _ = std::fs::remove_file(&dest);
                return cancel();
            }
            match result {
                Ok(()) => match verify_downloaded_file(&dest, expected_hash) {
                    Ok(()) => {
                        downloaded = true;
                        break;
                    }
                    Err(e) => error = e,
                },
                Err(e) => error = e,
            }
            let _ = std::fs::remove_file(&dest);
        }
        if !downloaded {
            return (
                "download-failed".into(),
                false,
                format!("Npcap 下载未完成：{error}\n请检查网络后点击“重试驱动”。应用文件已保留。"),
            );
        }
        (dest.as_path(), expected_hash, interactive_args())
    };
    report(DriverProgress {
        percent: 89,
        message: "正在校验 Npcap 安装包…".into(),
    });
    let verified = verify_downloaded_file(installer, hash)
        .and_then(|_| host.authenticode_publisher(installer))
        .and_then(|publisher| {
            // WinVerifyTrust + the pinned official SHA identify the exact signed payload.
            if publisher_trusted(&publisher) || publisher == "Authenticode-verified" {
                Ok(())
            } else {
                Err(format!("安装包发布者未通过核验：{publisher}"))
            }
        });
    if let Err(e) = verified {
        if downloaded {
            let _ = std::fs::remove_file(installer);
        }
        return ("verification-failed".into(), false, e);
    }
    if cancelled() {
        if downloaded {
            let _ = std::fs::remove_file(installer);
        }
        return cancel();
    }
    report(DriverProgress {
        percent: 92,
        message: "正在安装 Npcap，请在官方窗口中完成许可确认和安装。".into(),
    });
    let execution = host.run_installer(installer, &args);
    if downloaded {
        let _ = std::fs::remove_file(installer);
    }
    let code = match execution {
        Ok(code) => code,
        Err(e) => return ("failed".into(), false, e),
    };
    report(DriverProgress {
        percent: 97,
        message: "正在重新检测驱动是否可用…".into(),
    });
    let available = detect_with(host) == DriverStatus::Available;
    let exit = DriverExit::from_code(code);
    if exit.needs_reboot() {
        let key = if exit.is_success() {
            "reboot-required"
        } else {
            "reboot-retry"
        };
        return (key.into(), true, exit.message());
    }
    if available {
        return (
            "installed".into(),
            false,
            "Npcap 已安装并通过可用性检测。".into(),
        );
    }
    match exit {
        DriverExit::Success => (
            "not-ready".into(),
            false,
            "官方安装程序已结束，但还未检测到可用的 x64 驱动。请重试驱动安装。".into(),
        ),
        DriverExit::Cancelled => cancel(),
        DriverExit::AnotherInstallRunning => ("busy".into(), false, exit.message()),
        DriverExit::UnsupportedOs => ("unsupported-os".into(), false, exit.message()),
        _ => ("failed".into(), false, exit.message()),
    }
}
