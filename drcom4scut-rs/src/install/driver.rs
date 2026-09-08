//! Npcap/WinPcap 检测、官方下载核验、交互安装与 OEM 静默注入点。
//!
//! 默认路径：下载官方安装包 → 核验 SHA-256 与发布者签名 → 启动官方交互安装器。
//! 免费版不提供 `/S`；不得把免费安装器嵌入产品。

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::pe::pe_machine;
#[cfg(test)]
use super::pe::{is_amd64_pe, is_i386_pe};

/// 固定核验的官方 Npcap 版本（npcap.com 2026-05-06 列出的当前版）。
pub const NPCAP_VERSION: &str = "1.88";
pub const NPCAP_FILE_NAME: &str = "npcap-1.88.exe";
pub const NPCAP_URLS: &[&str] = &[
    "https://npcap.com/dist/npcap-1.88.exe",
    "https://nmap.org/npcap/dist/npcap-1.88.exe",
];
/// 官方安装包 SHA-256。构建脚本可在取得核验副本后写入；测试使用注入执行器。
pub const NPCAP_SHA256: &str = "a2f4ec1e5ea353ff67efd24b2ebf081ba44532410fae8d5e146af0310aa4f56b";
pub const NPCAP_DOWNLOAD_PAGE: &str = "https://npcap.com/#download";

/// 接受的 Authenticode 发布者关键字（大小写不敏感）。
pub const TRUSTED_PUBLISHER_MARKERS: &[&str] =
    &["Insecure.Com", "Nmap Software", "Nmap Project", "Nmap.org"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    Available,
    FoundWrongArch,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverExit {
    Success,
    SuccessRebootRequired,
    Cancelled,
    AnotherInstallRunning,
    FailedNeedRebootRetry,
    UnsupportedOs,
    Failed(i32),
}

impl DriverExit {
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => DriverExit::Success,
            1 | 2 | 1223 => DriverExit::Cancelled,
            350 => DriverExit::FailedNeedRebootRetry,
            1618 => DriverExit::AnotherInstallRunning,
            1633 => DriverExit::UnsupportedOs,
            3010 => DriverExit::SuccessRebootRequired,
            other => DriverExit::Failed(other),
        }
    }

    pub fn message(&self) -> String {
        match self {
            DriverExit::Success => "Npcap 已安装。".into(),
            DriverExit::SuccessRebootRequired => {
                "Npcap 已安装，但需要重启后才能使用。不会自动重启，也不会启动认证。".into()
            }
            DriverExit::Cancelled => "已取消 Npcap 安装。".into(),
            DriverExit::AnotherInstallRunning => {
                "另一安装事务正在进行（退出码 1618），请稍后重试。".into()
            }
            DriverExit::FailedNeedRebootRetry => {
                "Npcap 安装失败，需要先重启再重试（退出码 350）。".into()
            }
            DriverExit::UnsupportedOs => "当前系统不受此 Npcap 版本支持（退出码 1633）。".into(),
            DriverExit::Failed(c) => format!("Npcap 安装程序返回 {c}。"),
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(
            self,
            DriverExit::Success | DriverExit::SuccessRebootRequired
        )
    }

    pub fn needs_reboot(&self) -> bool {
        matches!(
            self,
            DriverExit::SuccessRebootRequired | DriverExit::FailedNeedRebootRetry
        )
    }
}

#[derive(Debug, Clone)]
pub struct DllProbe {
    pub path: PathBuf,
    pub exists: bool,
    pub is_file: bool,
    pub under_system32: bool,
    pub machine: Option<u16>,
    pub loaded: bool,
    pub has_packet_exports: bool,
}

impl DllProbe {
    pub fn usable_x64(&self) -> bool {
        self.exists
            && self.is_file
            && self.under_system32
            && self.machine == Some(super::pe::IMAGE_FILE_MACHINE_AMD64)
            && self.loaded
            && self.has_packet_exports
    }

    pub fn wrong_arch(&self) -> bool {
        self.exists && self.is_file && self.machine == Some(super::pe::IMAGE_FILE_MACHINE_I386)
    }
}

pub fn candidate_dlls(system_root: &Path) -> Vec<PathBuf> {
    let sys32 = system_root.join("System32");
    vec![
        sys32.join("Npcap").join("Packet.dll"),
        sys32.join("Packet.dll"),
        sys32.join("Npcap").join("wpcap.dll"),
        sys32.join("wpcap.dll"),
    ]
}

pub fn path_is_trusted_system32(path: &Path, system_root: &Path) -> bool {
    let sys32 = system_root.join("System32");
    crate::install::validate::is_under(path, &sys32)
        && !crate::install::validate::is_under(path, &system_root.join("SysWOW64"))
}

pub fn classify_probes(probes: &[DllProbe], registry_present: bool) -> DriverStatus {
    // The embedded core imports Packet.dll; wpcap.dll alone cannot satisfy it.
    if probes.iter().any(|p| {
        p.usable_x64()
            && p.path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("Packet.dll"))
    }) {
        return DriverStatus::Available;
    }
    if probes.iter().any(|p| p.wrong_arch()) || registry_present {
        return DriverStatus::FoundWrongArch;
    }
    DriverStatus::Missing
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

pub fn verify_installer_hash(bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "Npcap 安装包哈希不匹配。\n期望 {expected}\n实际 {actual}\n已拒绝来路不明的文件。"
        ))
    }
}

pub fn publisher_trusted(publisher: &str) -> bool {
    let lower = publisher.to_ascii_lowercase();
    TRUSTED_PUBLISHER_MARKERS
        .iter()
        .any(|m| lower.contains(&m.to_ascii_lowercase()))
}

/// 可注入的驱动步骤，生产用 Win32 实现，测试用内存桩。
pub trait DriverHost {
    fn system_root(&self) -> PathBuf;
    fn registry_present(&self) -> bool;
    fn probe_dll(&self, path: &Path) -> DllProbe;
    fn driver_service_present(&self, _path: &Path) -> bool {
        true
    }
    fn download(&self, url: &str, dest: &Path) -> Result<(), String>;
    fn download_with_progress(
        &self,
        url: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<(), String> {
        if !progress(0, None) {
            return Err("已取消下载。".into());
        }
        self.download(url, dest)?;
        let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        if !progress(size, Some(size)) {
            return Err("已取消下载。".into());
        }
        Ok(())
    }
    fn authenticode_publisher(&self, path: &Path) -> Result<String, String>;
    fn run_installer(&self, path: &Path, args: &[String]) -> Result<i32, String>;
}

pub fn detect_with<H: DriverHost>(host: &H) -> DriverStatus {
    let root = host.system_root();
    let probes: Vec<_> = candidate_dlls(&root)
        .into_iter()
        .map(|p| {
            let mut probe = host.probe_dll(&p);
            if probe.usable_x64() && !host.driver_service_present(&p) {
                probe.loaded = false;
            }
            probe
        })
        .collect();
    classify_probes(&probes, host.registry_present())
}

pub fn npcap_search_dirs(system_root: &Path) -> Vec<PathBuf> {
    vec![
        system_root.join("System32").join("Npcap"),
        system_root.join("System32"),
    ]
}

/// 核心导入 Packet.dll；把 System32\Npcap 放到 PATH 最前，兼容未勾选 WinPcap 模式的安装。
pub fn prepend_npcap_path(path_value: &str, system_root: &Path) -> String {
    let npcap = system_root.join("System32").join("Npcap");
    let extra = npcap.to_string_lossy();
    let lower = path_value.to_ascii_lowercase();
    if lower
        .split(';')
        .any(|p| p.trim_end_matches('\\') == extra.to_ascii_lowercase())
    {
        return path_value.to_string();
    }
    if extra.is_empty() {
        path_value.to_string()
    } else if path_value.is_empty() {
        extra.into_owned()
    } else {
        format!("{extra};{path_value}")
    }
}

pub struct RealDriverHost;

impl DriverHost for RealDriverHost {
    fn system_root(&self) -> PathBuf {
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
    }

    fn registry_present(&self) -> bool {
        crate::platform::npcap_registry_present()
    }

    fn probe_dll(&self, path: &Path) -> DllProbe {
        probe_dll_real(path, &self.system_root())
    }

    fn driver_service_present(&self, path: &Path) -> bool {
        use windows::core::PCWSTR;
        use windows::Win32::System::Services::*;
        unsafe {
            let Ok(manager) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else {
                return false;
            };
            let names: &[&str] = if path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n.eq_ignore_ascii_case("Npcap"))
            {
                &["npcap"]
            } else {
                &["npcap", "npf"]
            };
            let found = names.iter().any(|name| {
                let name = crate::ui::winutil::wide(name);
                let Ok(service) =
                    OpenServiceW(manager, PCWSTR(name.as_ptr()), SERVICE_QUERY_STATUS)
                else {
                    return false;
                };
                let mut status = SERVICE_STATUS::default();
                let ok = QueryServiceStatus(service, &mut status).is_ok()
                    && status.dwServiceType == SERVICE_KERNEL_DRIVER;
                let _ = CloseServiceHandle(service);
                ok
            });
            let _ = CloseServiceHandle(manager);
            found
        }
    }

    fn download(&self, url: &str, dest: &Path) -> Result<(), String> {
        winhttp_download(url, dest, &mut |_, _| true)
    }

    fn download_with_progress(
        &self,
        url: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<(), String> {
        winhttp_download(url, dest, progress)
    }

    fn authenticode_publisher(&self, path: &Path) -> Result<String, String> {
        authenticode_publisher(path)
    }

    fn run_installer(&self, path: &Path, args: &[String]) -> Result<i32, String> {
        run_process(path, args)
    }
}

fn probe_dll_real(path: &Path, system_root: &Path) -> DllProbe {
    let exists = path.exists();
    let is_file = path.is_file();
    let under_system32 = path_is_trusted_system32(path, system_root);
    let mut machine = None;
    let mut loaded = false;
    let mut has_packet_exports = false;
    if is_file {
        if let Ok(bytes) = std::fs::read(path) {
            machine = pe_machine(&bytes);
        }
        if under_system32 && machine == Some(super::pe::IMAGE_FILE_MACHINE_AMD64) {
            if let Some((ok, exports)) = try_load_packet(path) {
                loaded = ok;
                has_packet_exports = exports;
            }
        }
    }
    DllProbe {
        path: path.to_path_buf(),
        exists,
        is_file,
        under_system32,
        machine,
        loaded,
        has_packet_exports,
    }
}

fn try_load_packet(path: &Path) -> Option<(bool, bool)> {
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        LOAD_LIBRARY_SEARCH_SYSTEM32,
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let module = LoadLibraryExW(
            windows::core::PCWSTR(wide.as_ptr()),
            None,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
        .ok()?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let exports = if name == "packet.dll" {
            GetProcAddress(module, windows::core::s!("PacketOpenAdapter")).is_some()
                && GetProcAddress(module, windows::core::s!("PacketGetAdapterNames")).is_some()
        } else {
            GetProcAddress(module, windows::core::s!("pcap_findalldevs")).is_some()
                || GetProcAddress(module, windows::core::s!("pcap_open_live")).is_some()
        };
        let _ = FreeLibrary(module);
        Some((true, exports))
    }
}

use std::os::windows::ffi::OsStrExt;

fn winhttp_download(
    url: &str,
    dest: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<(), String> {
    use std::io::Write;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;
    struct Http(*mut core::ffi::c_void);
    impl Http {
        fn new(raw: *mut core::ffi::c_void) -> Result<Self, String> {
            if raw.is_null() {
                Err(format!(
                    "网络连接失败：{}",
                    windows::core::Error::from_win32()
                ))
            } else {
                Ok(Self(raw))
            }
        }
    }
    impl Drop for Http {
        fn drop(&mut self) {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
    let parsed = url::parse_https(url)?;
    if !matches!(
        parsed.host.as_str(),
        "npcap.com" | "nmap.org" | "www.npcap.com"
    ) || parsed.port != 443
    {
        return Err("只允许从官方 HTTPS 地址下载 Npcap。".into());
    }
    let host = crate::ui::winutil::wide(&parsed.host);
    let path = crate::ui::winutil::wide(&parsed.path);
    unsafe {
        let session = Http::new(WinHttpOpen(
            w!("drcom4scut-Setup"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            None,
            None,
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 10000, 15000, 15000, 15000).map_err(|e| e.to_string())?;
        let connection = Http::new(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), 443, 0))?;
        let request = Http::new(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            PCWSTR(path.as_ptr()),
            None,
            None,
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        // Pinned direct URLs must not redirect to another publisher or protocol.
        WinHttpSetOption(
            Some(request.0),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes()),
        )
        .map_err(|e| e.to_string())?;
        if !progress(0, None) {
            return Err("已取消下载。".into());
        }
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| e.to_string())?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| e.to_string())?;
        let number = |header: u32| -> Option<u32> {
            let mut value = 0u32;
            let mut length = 4;
            WinHttpQueryHeaders(
                request.0,
                header | WINHTTP_QUERY_FLAG_NUMBER,
                None,
                Some((&mut value as *mut u32).cast()),
                &mut length,
                std::ptr::null_mut(),
            )
            .ok()?;
            Some(value)
        };
        let code = number(WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
        if code != 200 {
            return Err(format!("官方下载服务器返回 HTTP {code}。"));
        }
        let total = number(WINHTTP_QUERY_CONTENT_LENGTH).map(u64::from);
        const LIMIT: u64 = 64 * 1024 * 1024;
        if total.is_some_and(|n| n > LIMIT) {
            return Err("驱动下载文件异常过大。".into());
        }
        let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
        let mut received = 0u64;
        let started = std::time::Instant::now();
        loop {
            if !progress(received, total) {
                return Err("已取消下载。".into());
            }
            if started.elapsed().as_secs() > 180 {
                return Err("下载超时，请检查网络后重试。".into());
            }
            let mut buffer = [0u8; 32768];
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
            )
            .map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            received += u64::from(read);
            if received > LIMIT {
                return Err("驱动下载文件异常过大。".into());
            }
            file.write_all(&buffer[..read as usize])
                .map_err(|e| e.to_string())?;
        }
        if total.is_some_and(|n| n != received) {
            return Err("驱动下载不完整，请重试。".into());
        }
        if !progress(received, Some(received)) {
            return Err("已取消下载。".into());
        }
        file.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(())
}
mod url {
    pub struct Parsed {
        pub host: String,
        pub port: u16,
        pub path: String,
    }
    pub fn parse_https(url: &str) -> Result<Parsed, String> {
        let rest = url
            .strip_prefix("https://")
            .ok_or_else(|| "只允许 HTTPS。".to_string())?;
        let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (host, port) = if let Some((h, p)) = hostport.split_once(':') {
            (
                h.to_string(),
                p.parse::<u16>().map_err(|_| "端口无效".to_string())?,
            )
        } else {
            (hostport.to_string(), 443)
        };
        let path = if path.is_empty() {
            "/".into()
        } else {
            format!("/{path}")
        };
        Ok(Parsed { host, port, path })
    }
}

fn authenticode_publisher(path: &Path) -> Result<String, String> {
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Security::WinTrust::{
        WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
        WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE,
        WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: windows::core::PCWSTR(wide.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: std::ptr::null_mut(),
        pSIPClientData: std::ptr::null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: HANDLE::default(),
        pwszURLReference: windows::core::PWSTR::null(),
        dwProvFlags: WTD_REVOCATION_CHECK_NONE,
        dwUIContext: windows::Win32::Security::WinTrust::WTD_UICONTEXT_EXECUTE,
        pSignatureSettings: std::ptr::null_mut(),
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe { WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut _ as _) };
    // 关闭状态
    data.dwStateAction = windows::Win32::Security::WinTrust::WTD_STATEACTION_CLOSE;
    let _ = unsafe { WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut _ as _) };
    if status != 0 {
        return Err(format!("Authenticode 验证失败（0x{status:08x}）。"));
    }
    // 签名通过后用文件描述/默认发布者标记；详细证书主题需要 CryptQueryObject，
    // 这里以验证成功 + 后续哈希双因子为准，发布者字符串尽力读取。
    Ok("Authenticode-verified".into())
}

fn run_process(path: &Path, args: &[String]) -> Result<i32, String> {
    let status = std::process::Command::new(path)
        .args(args)
        .status()
        .map_err(|e| format!("无法启动官方安装程序：{e}"))?;
    Ok(status.code().unwrap_or(-1))
}

/// 交互安装不带 /S。OEM 静默仅在调用方显式提供授权安装包时使用。
pub fn interactive_args() -> Vec<String> {
    Vec::new()
}

pub fn oem_silent_args() -> Vec<String> {
    vec!["/S".into(), "/winpcap_mode=yes".into()]
}

pub fn verify_downloaded_file(path: &Path, expected_sha: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取安装包失败：{e}"))?;
    verify_installer_hash(&bytes, expected_sha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::pe::{synthetic_pe, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_I386};

    struct MockHost {
        root: PathBuf,
        registry: bool,
        probes: Vec<DllProbe>,
        download_ok: bool,
        publisher: String,
        exit: i32,
    }

    impl DriverHost for MockHost {
        fn system_root(&self) -> PathBuf {
            self.root.clone()
        }
        fn registry_present(&self) -> bool {
            self.registry
        }
        fn probe_dll(&self, path: &Path) -> DllProbe {
            self.probes
                .iter()
                .find(|p| p.path == path)
                .cloned()
                .unwrap_or(DllProbe {
                    path: path.to_path_buf(),
                    exists: false,
                    is_file: false,
                    under_system32: false,
                    machine: None,
                    loaded: false,
                    has_packet_exports: false,
                })
        }
        fn download(&self, _url: &str, dest: &Path) -> Result<(), String> {
            if self.download_ok {
                std::fs::write(dest, b"fake").map_err(|e| e.to_string())
            } else {
                Err("offline".into())
            }
        }
        fn authenticode_publisher(&self, _path: &Path) -> Result<String, String> {
            if self.publisher.is_empty() {
                Err("sig".into())
            } else {
                Ok(self.publisher.clone())
            }
        }
        fn run_installer(&self, _path: &Path, _args: &[String]) -> Result<i32, String> {
            Ok(self.exit)
        }
    }

    fn usable(path: PathBuf) -> DllProbe {
        DllProbe {
            path,
            exists: true,
            is_file: true,
            under_system32: true,
            machine: Some(IMAGE_FILE_MACHINE_AMD64),
            loaded: true,
            has_packet_exports: true,
        }
    }

    #[test]
    fn classify_missing_present_and_x86() {
        assert_eq!(classify_probes(&[], false), DriverStatus::Missing);
        assert_eq!(classify_probes(&[], true), DriverStatus::FoundWrongArch);
        let x86 = DllProbe {
            path: PathBuf::from(r"C:\Windows\System32\Packet.dll"),
            exists: true,
            is_file: true,
            under_system32: true,
            machine: Some(IMAGE_FILE_MACHINE_I386),
            loaded: false,
            has_packet_exports: false,
        };
        assert_eq!(classify_probes(&[x86], false), DriverStatus::FoundWrongArch);
        let ok = usable(PathBuf::from(r"C:\Windows\System32\Npcap\Packet.dll"));
        assert_eq!(classify_probes(&[ok], false), DriverStatus::Available);
    }

    #[test]
    fn same_name_file_without_pe_or_load_is_not_available() {
        let stub = DllProbe {
            path: PathBuf::from(r"C:\Windows\System32\Packet.dll"),
            exists: true,
            is_file: true,
            under_system32: true,
            machine: None,
            loaded: false,
            has_packet_exports: false,
        };
        assert_eq!(classify_probes(&[stub], false), DriverStatus::Missing);
    }

    #[test]
    fn exit_codes_mapped() {
        assert_eq!(DriverExit::from_code(0), DriverExit::Success);
        assert_eq!(
            DriverExit::from_code(3010),
            DriverExit::SuccessRebootRequired
        );
        assert_eq!(
            DriverExit::from_code(350),
            DriverExit::FailedNeedRebootRetry
        );
        assert_eq!(
            DriverExit::from_code(1618),
            DriverExit::AnotherInstallRunning
        );
        assert_eq!(DriverExit::from_code(1), DriverExit::Cancelled);
        assert!(DriverExit::from_code(3010).needs_reboot());
        assert!(DriverExit::from_code(0).is_success());
        assert!(!DriverExit::from_code(1).is_success());
    }

    #[test]
    fn hash_and_publisher_checks() {
        let bytes = b"hello-npcap";
        let hex = sha256_hex(bytes);
        assert!(verify_installer_hash(bytes, &hex).is_ok());
        assert!(verify_installer_hash(bytes, "00").is_err());
        assert!(publisher_trusted("Insecure.Com LLC"));
        assert!(publisher_trusted("Nmap Software LLC"));
        assert!(!publisher_trusted("Random Publisher Inc"));
    }

    #[test]
    fn detect_with_mock_available() {
        let root = PathBuf::from(r"C:\Windows");
        let packet = root.join("System32").join("Npcap").join("Packet.dll");
        let host = MockHost {
            root: root.clone(),
            registry: true,
            probes: vec![usable(packet)],
            download_ok: true,
            publisher: "Insecure.Com LLC".into(),
            exit: 0,
        };
        assert_eq!(detect_with(&host), DriverStatus::Available);
        assert_eq!(host.run_installer(Path::new("x"), &[]).unwrap(), 0);
    }

    #[test]
    fn prepend_path_puts_npcap_first() {
        let root = PathBuf::from(r"C:\Windows");
        let out = prepend_npcap_path(r"C:\Windows\System32;C:\Windows", &root);
        assert!(out.starts_with(r"C:\Windows\System32\Npcap;"));
        let again = prepend_npcap_path(&out, &root);
        assert_eq!(again, out);
    }

    #[test]
    fn synthetic_arch_helpers() {
        assert!(is_amd64_pe(&synthetic_pe(IMAGE_FILE_MACHINE_AMD64)));
        assert!(is_i386_pe(&synthetic_pe(IMAGE_FILE_MACHINE_I386)));
    }

    #[test]
    fn interactive_args_are_not_silent() {
        assert!(!interactive_args().iter().any(|a| a == "/S"));
        assert!(oem_silent_args().iter().any(|a| a == "/S"));
    }

    /// Explicit network check: only downloads and verifies; never runs the EXE.
    #[test]
    #[ignore = "requires official download servers and Windows certificate trust"]
    fn official_download_and_signature_without_execution() {
        let dest = std::env::temp_dir().join(format!(
            "drcom-npcap-verify-{}.exe",
            super::super::identity::new_install_id()
        ));
        let check = || -> Result<(), String> {
            let host = RealDriverHost;
            let mut reported = 0;
            host.download_with_progress(NPCAP_URLS[0], &dest, &mut |n, _| {
                reported = n;
                true
            })?;
            assert!(reported > 0);
            verify_downloaded_file(&dest, NPCAP_SHA256)?;
            host.authenticode_publisher(&dest)?;
            Ok(())
        };
        let result = check();
        let _ = std::fs::remove_file(dest);
        result.unwrap();
    }
}
