//! Windows 平台集成：单实例互斥、开机启动计划任务、Npcap 检测。
//!
//! 对齐 .NET 版：
//! - 单实例：`App.xaml.cs:7-76`（`Local\Drcom4scutFluentGui` 互斥锁与 `-Show` 唤起事件）；
//! - 开机启动：`Services.cs:804-865`（schtasks 计划任务 `drcom4scutGUI` + 旧版 Run 键清理）；
//! - Npcap 检测：`Services.cs:142-156`（PacketCaptureRuntime）。
//!
//! 与 .NET 版的差异（有意为之）：
//! - 具名 Mutex/Event 显式携带 DACL 为 NULL 的安全描述符，允许低完整性/提权实例互相打开
//!   （规避调研报告 R7：提权实例创建的对象默认 DACL 会拒绝普通完整性进程）；
//! - windows 0.61 的 CreateMutexExW 包装会丢弃 ERROR_ALREADY_EXISTS，故用「立即等待互斥锁」
//!   判定归属，顺带覆盖了 .NET 版 AbandonedMutexException 重试想处理的「上次实例崩溃」场景；
//! - Npcap 检测按任务契约区分 Missing / FoundButNotX64（.NET 版只返回布尔的 IsInstalled）。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_SUCCESS, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::{
    InitializeSecurityDescriptor, SetSecurityDescriptorDacl, PSECURITY_DESCRIPTOR,
    SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_READ, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_SAM_FLAGS,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexExW, OpenEventW, ReleaseMutex, SetEvent, WaitForSingleObject,
    CREATE_MUTEX_INITIAL_OWNER, CREATE_NO_WINDOW, EVENT_MODIFY_STATE, MUTEX_ALL_ACCESS,
};

// ---------------------------------------------------------------------------
// 常量（与 .NET 版逐一对应）
// ---------------------------------------------------------------------------

/// 单实例互斥锁名。注意：.NET 源码 `App.xaml.cs:7` 用的是 `Drcom4scutFluentGui`（含 Fluent）。
pub const MUTEX_NAME: &str = "Local\\Drcom4scutFluentGui";

/// 唤起事件名（.NET `App.xaml.cs:8` 的 ShowEventName）。
pub const WAKE_EVENT_NAME: &str = "Local\\Drcom4scutFluentGui-Show";

/// 安装器/卸载器请求 GUI 完整退出（保存、停核心、退出，不只是隐藏到托盘）。
pub const EXIT_EVENT_NAME: &str = "Local\\Drcom4scutFluentGui-Exit";

/// 由互斥体名推导唤起事件名：`{mutex}-Show`。
pub fn wake_event_name(mutex_name: &str) -> String {
    format!("{mutex_name}-Show")
}

/// 开机启动计划任务名（.NET `Services.cs:808` 的 TaskName）。
pub const TASK_NAME: &str = "drcom4scutGUI";

/// 旧版自启动残留所在的 Run 键（.NET `Services.cs:806`）。
pub const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// 旧版自启动残留的值名（.NET `Services.cs:807`）。
pub const RUN_VALUE_NAME: &str = "drcom4scutGUI";

/// Npcap 安装信息所在的注册表键（64 位视图）。
pub const NPCAP_REG_KEY: &str = r"SOFTWARE\Npcap";

/// WinPcap 安装信息所在的注册表键。
pub const WINPCAP_REG_KEY: &str = r"SOFTWARE\WOW6432Node\WinPcap";

/// Npcap 官方下载页（.NET `Services.cs:144` 的 DownloadUrl）。
pub const NPCAP_DOWNLOAD_URL: &str = "https://npcap.com/#download";

/// winnt.h 中 `SECURITY_DESCRIPTOR_REVISION` 的取值；windows 0.61 未导出该常量。
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

/// schtasks 超时，对齐 .NET `RunSchtasks` 的 10 秒（`Services.cs:858`）。
const SCHTASKS_TIMEOUT_MS: u64 = 10_000;

// ---------------------------------------------------------------------------
// 单实例
// ---------------------------------------------------------------------------

/// `acquire` 失败的原因。
///
/// 除「已有实例」外还涵盖 Win32 硬失败：单实例锁拿不到时调用方通常一律
/// 唤起旧实例后退出，共用一个错误类型可以简化启动路径的 match。
#[derive(Debug)]
pub enum AlreadyRunning {
    /// 已有实例持有单实例锁，本次启动应立即退出。
    InstanceRunning,
    /// Win32 调用失败（创建互斥锁/事件出错），无法判断实例状态。
    Win32(windows::core::Error),
}

/// 单实例守卫。持有互斥锁与唤起事件的句柄，Drop 时释放。
pub struct Guard {
    mutex: HANDLE,
    event: HANDLE,
    exit_event: HANDLE,
}

/// 获取单实例锁并创建唤起事件。
///
/// 成功返回 [`Guard`]；已有实例在运行时返回 [`AlreadyRunning::InstanceRunning`]，
/// 调用方此时应先 [`Guard::wake_other`] 唤起旧实例再退出（对齐 .NET `App.xaml.cs:12-21`）。
pub fn acquire(name: &str) -> Result<Guard, AlreadyRunning> {
    // DACL 为 NULL 的安全描述符：允许低完整性/提权实例互相打开内核对象（调研报告 R7）。
    let mut sd = SECURITY_DESCRIPTOR::default();
    // SAFETY: sd 是本函数栈上的合法 SECURITY_DESCRIPTOR，两个调用只写入该结构。
    unsafe {
        if let Err(e) = InitializeSecurityDescriptor(
            PSECURITY_DESCRIPTOR(&mut sd as *mut SECURITY_DESCRIPTOR as *mut core::ffi::c_void),
            SECURITY_DESCRIPTOR_REVISION,
        ) {
            return Err(AlreadyRunning::Win32(e));
        }
        // DACL 存在但为 NULL：授予所有人访问权。
        if let Err(e) = SetSecurityDescriptorDacl(
            PSECURITY_DESCRIPTOR(&mut sd as *mut SECURITY_DESCRIPTOR as *mut core::ffi::c_void),
            true,
            None,
            false,
        ) {
            return Err(AlreadyRunning::Win32(e));
        }
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: &mut sd as *mut SECURITY_DESCRIPTOR as *mut core::ffi::c_void,
        bInheritHandle: false.into(),
    };

    let mutex_wide = HSTRING::from(name);
    // 事件必须与互斥体不同名：同名时 CreateEventW 失败并返回 ERROR_INVALID_HANDLE (0x80070006)。
    // 对齐 .NET：Mutex `Local\Drcom4scutFluentGui`，Event `Local\Drcom4scutFluentGui-Show`。
    let event_wide = HSTRING::from(wake_event_name(name));
    // SAFETY: sa 与 wide 均为本函数合法栈变量，调用期间有效；CREATE_MUTEX_INITIAL_OWNER
    // 使「创建即持有」，规避 CreateMutexW 后再请求所有权的竞态（SDK 推荐）。
    let mutex = unsafe {
        CreateMutexExW(
            Some(&sa),
            &mutex_wide,
            CREATE_MUTEX_INITIAL_OWNER,
            MUTEX_ALL_ACCESS.0,
        )
    }
    .map_err(AlreadyRunning::Win32)?;

    // windows 0.61 的包装函数会丢弃 ERROR_ALREADY_EXISTS，改用「立即等待」判定归属：
    // - 拿到（WAIT_OBJECT_0，或接管被遗弃的锁 WAIT_ABANDONED，对应 .NET 版
    //   AbandonedMutexException 重试语义）=> 本实例是首个实例；
    // - 超时（WAIT_TIMEOUT）=> 已有实例持有锁。
    // SAFETY: mutex 是上方刚创建的合法句柄。
    let wait = unsafe { WaitForSingleObject(mutex, 0) };
    if wait == WAIT_TIMEOUT {
        // SAFETY: mutex 是上方刚创建的合法句柄，用完即关。
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return Err(AlreadyRunning::InstanceRunning);
    }
    if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
        // SAFETY: mutex 是上方刚创建的合法句柄，用完即关。
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return Err(AlreadyRunning::Win32(windows::core::Error::from_win32()));
    }

    // 具名自动复位事件：二次启动通过它唤起主窗口（.NET `App.xaml.cs:22`，AutoReset）。
    // SAFETY: sa 与 event_wide 均在调用期间有效。
    let event = match unsafe { CreateEventW(Some(&sa), false, false, &event_wide) } {
        Ok(handle) => handle,
        Err(e) => {
            // SAFETY: mutex 是上方刚创建的合法句柄，失败路径需回收。
            unsafe {
                let _ = CloseHandle(mutex);
            }
            return Err(AlreadyRunning::Win32(e));
        }
    };

    let exit_wide = HSTRING::from(EXIT_EVENT_NAME);
    let exit_event = match unsafe { CreateEventW(Some(&sa), false, false, &exit_wide) } {
        Ok(handle) => handle,
        Err(e) => {
            unsafe {
                let _ = CloseHandle(event);
                let _ = CloseHandle(mutex);
            }
            return Err(AlreadyRunning::Win32(e));
        }
    };

    Ok(Guard {
        mutex,
        event,
        exit_event,
    })
}

impl Guard {
    /// 唤醒已有实例（二次启动时调用，对齐 .NET `App.xaml.cs:68-76` 的 SignalExistingInstance）。
    /// 返回是否成功 SetEvent；事件不存在等错误按 .NET 版静默处理为 false。
    pub fn wake_other(name: &str) -> bool {
        let wide = HSTRING::from(name);
        // SAFETY: wide 在调用期间存活；句柄用完即关，不跨函数持有。
        unsafe {
            let handle = match OpenEventW(EVENT_MODIFY_STATE, false, &wide) {
                Ok(handle) => handle,
                Err(_) => return false,
            };
            let signaled = SetEvent(handle).is_ok();
            let _ = CloseHandle(handle);
            signaled
        }
    }

    /// 在后台线程等待唤起事件，每次触发调用一次回调（对齐 .NET `App.xaml.cs:27-43`
    /// 的 ShowEvent 等待线程，IsBackground 等价于 detached 线程）。
    ///
    /// [`Guard`] 释放后事件句柄关闭，等待返回失败，线程随即退出。
    pub fn on_wake<F>(&self, callback: F)
    where
        F: FnMut() + Send + 'static,
    {
        // HANDLE 含裸指针（!Send），跨线程只传地址数值。
        let event_addr = self.event.0 as usize;
        std::thread::spawn(move || {
            let mut callback = callback;
            // SAFETY: 事件句柄由 Guard 持有，Guard 释放前始终有效；
            // Guard 释放后 WaitForSingleObject 返回失败，循环退出，不再触碰句柄。
            let event = HANDLE(event_addr as *mut core::ffi::c_void);
            loop {
                // SAFETY: event 在 Guard 存活期间是有效句柄。
                let wait = unsafe { WaitForSingleObject(event, u32::MAX) }; // INFINITE = 0xFFFFFFFF
                if wait.0 == 0 {
                    // WAIT_OBJECT_0
                    callback();
                } else {
                    // WAIT_FAILED：句柄已被关闭（应用退出），结束线程。
                    break;
                }
            }
        });
    }

    /// 等待卸载/安装器发出的退出事件，触发完整退出。
    pub fn on_exit<F>(&self, callback: F)
    where
        F: FnMut() + Send + 'static,
    {
        let event_addr = self.exit_event.0 as usize;
        std::thread::spawn(move || {
            let mut callback = callback;
            let event = HANDLE(event_addr as *mut core::ffi::c_void);
            loop {
                let wait = unsafe { WaitForSingleObject(event, u32::MAX) };
                if wait.0 == 0 {
                    callback();
                } else {
                    break;
                }
            }
        });
    }
}

/// 请求已运行的 GUI 完整退出。
pub fn signal_exit() -> bool {
    Guard::wake_other(EXIT_EVENT_NAME)
}

impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: 两个句柄都由 acquire 创建且仅在本结构内使用。
        // 互斥锁可能被本线程以更高计数持有（CREATE_MUTEX_INITIAL_OWNER + 立即等待
        // 各计一次），循环释放直到非持有为止。
        unsafe {
            while ReleaseMutex(self.mutex).is_ok() {}
            let _ = CloseHandle(self.mutex);
            let _ = CloseHandle(self.event);
            let _ = CloseHandle(self.exit_event);
        }
    }
}

// ---------------------------------------------------------------------------
// 开机启动（schtasks 计划任务）
// ---------------------------------------------------------------------------

/// 构建 `schtasks /Create` 参数列表。纯函数，便于测试引号与路径空格处理。
///
/// 与 .NET `CreateLogonTask`（`Services.cs:831-832`）的命令行一致：
/// `/TR` 的值必须用内层引号包裹完整 exe 路径，否则含空格的路径会被截断。
pub fn build_create_command(exe_path: &str) -> Vec<String> {
    vec![
        "/Create".into(),
        "/TN".into(),
        TASK_NAME.into(),
        "/TR".into(),
        format!("\"{exe_path}\""),
        "/SC".into(),
        "ONLOGON".into(),
        "/RL".into(),
        "HIGHEST".into(),
        "/F".into(),
    ]
}

/// schtasks 的一次运行结果。
struct SchtasksResult {
    code: i32,
    stdout: String,
    stderr: String,
}

/// 取 schtasks 的错误详情：stderr 优先，为空时回退 stdout。
fn schtasks_detail(result: &SchtasksResult) -> String {
    let stderr = result.stderr.trim();
    if stderr.is_empty() {
        result.stdout.trim().to_string()
    } else {
        stderr.to_string()
    }
}

/// 构建用户可读的失败信息。与 .NET 版一致，无论具体原因都提示管理员权限
/// （schtasks 的本地化输出无法可靠判断是否为权限问题，见 R7 备注）。
fn schtasks_failure_message(action: &str, code: i32, detail: &str) -> String {
    format!(
        "{action}失败（schtasks 退出码 {code}）：{detail}。创建/删除高优先级开机计划任务需要管理员权限，请以管理员身份运行一次本程序后再试。"
    )
}

/// 判断 schtasks 输出是否表示「任务不存在」（覆盖中英文系统输出）。
fn task_missing_text(output: &str) -> bool {
    let lowered = output.to_ascii_lowercase();
    lowered.contains("does not exist")
        || lowered.contains("cannot find")
        || output.contains("不存在")
        || output.contains("找不到")
}

/// 运行 schtasks.exe 并等待退出。CREATE_NO_WINDOW 避免闪黑框（对齐 .NET CreateNoWindow）。
fn run_schtasks(args: &[String], timeout: Duration) -> Result<SchtasksResult, String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut command = Command::new("schtasks.exe");
    command
        .args(args)
        .creation_flags(CREATE_NO_WINDOW.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动 schtasks.exe：{e}"))?;

    // std 无带超时的 wait，轮询 try_wait；schtasks 输出远小于管道缓冲，不会卡死。
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("schtasks 执行超时。".to_string());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("等待 schtasks.exe 退出失败：{e}"));
            }
        }
    };

    // 进程已退出，管道数据不再增长，读取剩余输出。
    let mut stdout_raw = Vec::new();
    let mut stderr_raw = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout_raw);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr_raw);
    }
    // schtasks 在中文系统输出 GBK 文本，按 UTF-8 宽松解码，仅用于错误展示与关键字判断。
    Ok(SchtasksResult {
        code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout_raw).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_raw).into_owned(),
    })
}

/// 安装（或以 /F 覆盖）开机启动计划任务。权限不足等失败返回含
/// 「需要管理员权限」的用户可读错误。
pub fn install(exe_path: &str) -> Result<(), String> {
    let result = run_schtasks(
        &build_create_command(exe_path),
        Duration::from_millis(SCHTASKS_TIMEOUT_MS),
    )?;
    if result.code == 0 {
        return Ok(());
    }
    Err(schtasks_failure_message(
        "创建开机启动任务",
        result.code,
        &schtasks_detail(&result),
    ))
}

/// 删除开机启动计划任务；任务不存在视为成功。
pub fn remove() -> Result<(), String> {
    if !is_installed() {
        return Ok(());
    }
    let args: Vec<String> = ["/Delete", "/TN", TASK_NAME, "/F"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let result = run_schtasks(&args, Duration::from_millis(SCHTASKS_TIMEOUT_MS))?;
    if result.code == 0 {
        return Ok(());
    }
    let detail = schtasks_detail(&result);
    if task_missing_text(&detail) {
        return Ok(());
    }
    Err(schtasks_failure_message(
        "删除开机启动任务",
        result.code,
        &detail,
    ))
}

/// 查询开机启动计划任务是否已安装（`schtasks /Query` 退出码 0 视为已安装）。
pub fn is_installed() -> bool {
    let args: Vec<String> = ["/Query", "/TN", TASK_NAME]
        .iter()
        .map(|s| s.to_string())
        .collect();
    matches!(
        run_schtasks(&args, Duration::from_millis(SCHTASKS_TIMEOUT_MS)),
        Ok(result) if result.code == 0
    )
}

/// 删除旧版写在 `HKCU\...\Run` 下的自启动值（对齐 .NET `Services.cs:817-825`）。
/// 一切错误静默忽略：值本就可能不存在。
pub fn remove_legacy_run_value() {
    let subkey = HSTRING::from(RUN_KEY_PATH);
    let value = HSTRING::from(RUN_VALUE_NAME);
    // SAFETY: subkey/value 在调用期间存活；key 仅在 RegOpenKeyExW 成功后使用，用完即关。
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &subkey, None, KEY_SET_VALUE, &mut key) != ERROR_SUCCESS
        {
            return;
        }
        let _ = RegDeleteValueW(key, &value);
        let _ = RegCloseKey(key);
    }
}

// ---------------------------------------------------------------------------
// Npcap 检测
// ---------------------------------------------------------------------------

/// Npcap 可用性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcapStatus {
    /// x64 的 wpcap.dll 可用，核心可直接工作。
    Available,
    /// 安装了 Npcap/WinPcap 但缺少 x64 的 wpcap.dll（通常为安装时未勾选
    /// 「WinPcap API 兼容模式」），对应 UI 提示重装并启用兼容模式。
    FoundButNotX64,
    /// 完全未安装，需要引导用户去 NPCAP_DOWNLOAD_URL 下载。
    Missing,
}

/// 检测本机 Npcap/WinPcap 状态（x64 PE + 受信任系统路径加载 + Packet/pcap 导出）。
pub fn npcap_status() -> NpcapStatus {
    use crate::install::driver::{detect_with, DriverStatus, RealDriverHost};
    match detect_with(&RealDriverHost) {
        DriverStatus::Available => NpcapStatus::Available,
        DriverStatus::FoundWrongArch => NpcapStatus::FoundButNotX64,
        DriverStatus::Missing => NpcapStatus::Missing,
    }
}

/// 注册表是否表明安装过 Npcap 或 WinPcap。
pub fn npcap_registry_present() -> bool {
    registry_key_exists(HKEY_LOCAL_MACHINE, NPCAP_REG_KEY, KEY_WOW64_64KEY)
        || registry_key_exists(HKEY_LOCAL_MACHINE, WINPCAP_REG_KEY, REG_SAM_FLAGS(0))
}

/// `%SystemRoot%` 目录（默认 `C:\Windows`）。独立成函数便于替换。
fn system_root() -> PathBuf {
    std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from)
}

/// [`npcap_status`] 的可注入版本：系统根目录由参数给定，便于单元测试。
pub fn npcap_status_with(system_root: &Path) -> NpcapStatus {
    let has_x64_dll = npcap_dll_present(system_root);
    let has_registry = registry_key_exists(HKEY_LOCAL_MACHINE, NPCAP_REG_KEY, KEY_WOW64_64KEY)
        || registry_key_exists(HKEY_LOCAL_MACHINE, WINPCAP_REG_KEY, REG_SAM_FLAGS(0));
    classify_npcap(has_x64_dll, has_registry)
}

/// 纯判定逻辑：x64 的 wpcap.dll 优先；注册表能证明装过则视为缺 x64 DLL；否则未安装。
fn classify_npcap(has_x64_dll: bool, has_registry: bool) -> NpcapStatus {
    if has_x64_dll {
        NpcapStatus::Available
    } else if has_registry {
        NpcapStatus::FoundButNotX64
    } else {
        NpcapStatus::Missing
    }
}

/// 检查 `%SystemRoot%\System32\Npcap\wpcap.dll` 或 `%SystemRoot%\System32\wpcap.dll`。
///
/// 本程序是 64 位进程，`System32` 即真实 64 位系统目录，不涉及 WOW64 重定向。
fn npcap_dll_present(system_root: &Path) -> bool {
    system_root
        .join("System32")
        .join("Npcap")
        .join("wpcap.dll")
        .is_file()
        || system_root.join("System32").join("wpcap.dll").is_file()
}

/// 判断注册表键是否存在。`extra` 可传 `KEY_WOW64_64KEY` 指定 64 位视图。
fn registry_key_exists(hive: HKEY, subkey: &str, extra: REG_SAM_FLAGS) -> bool {
    let subkey = HSTRING::from(subkey);
    // SAFETY: subkey 在调用期间存活；key 仅在打开成功后使用，用完即关。
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(hive, &subkey, None, KEY_READ | extra, &mut key) != ERROR_SUCCESS {
            return false;
        }
        let _ = RegCloseKey(key);
        true
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // ---------- 纯逻辑 ----------

    #[test]
    fn create_command_quotes_exe_path_with_spaces() {
        let cmd = build_create_command(r"C:\Program Files\drcom4scutGUI\drcom4scutGUI.exe");
        assert_eq!(cmd[0], "/Create");
        assert_eq!(cmd[1], "/TN");
        assert_eq!(cmd[2], "drcom4scutGUI");
        assert_eq!(cmd[3], "/TR");
        // 内层引号必须包住含空格的完整路径。
        assert_eq!(
            cmd[4],
            r#""C:\Program Files\drcom4scutGUI\drcom4scutGUI.exe""#
        );
        assert_eq!(&cmd[5..], &["/SC", "ONLOGON", "/RL", "HIGHEST", "/F"]);
    }

    #[test]
    fn create_command_always_wraps_simple_path() {
        // 与 .NET 版一致：无论是否含空格，/TR 值始终带内层引号。
        let cmd = build_create_command(r"C:\Apps\drcom4scutGUI.exe");
        assert_eq!(cmd[4], r#""C:\Apps\drcom4scutGUI.exe""#);
    }

    #[test]
    fn npcap_classification_matrix() {
        assert_eq!(classify_npcap(true, false), NpcapStatus::Available);
        assert_eq!(classify_npcap(true, true), NpcapStatus::Available);
        assert_eq!(classify_npcap(false, true), NpcapStatus::FoundButNotX64);
        assert_eq!(classify_npcap(false, false), NpcapStatus::Missing);
    }

    #[test]
    fn failure_message_mentions_admin_rights() {
        let msg = schtasks_failure_message("创建开机启动任务", 1, "ERROR: Access is denied.");
        assert!(msg.contains("需要管理员权限"), "msg = {msg}");
        assert!(msg.contains("退出码 1"), "msg = {msg}");
        assert!(msg.contains("Access is denied."), "msg = {msg}");
    }

    #[test]
    fn task_missing_detection_covers_locales() {
        assert!(task_missing_text(
            "ERROR: The specified task name \"x\" does not exist in the system."
        ));
        assert!(task_missing_text("错误: 系统中不存在指定的任务名 \"x\"。"));
        assert!(task_missing_text(
            "ERROR: The system cannot find the file specified."
        ));
        assert!(!task_missing_text("成功: 计划任务 \"x\" 已删除。"));
        assert!(!task_missing_text(""));
    }

    // ---------- 文件系统（临时目录 + 注入路径，不依赖真实系统状态） ----------

    fn temp_system_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "drcom4scut-platform-test-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建临时目录失败");
        dir
    }

    #[test]
    fn npcap_dll_detected_in_npcap_subdir() {
        let root = temp_system_root("npcap-subdir");
        let dir = root.join("System32").join("Npcap");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wpcap.dll"), b"stub").unwrap();
        assert!(npcap_dll_present(&root));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn npcap_dll_detected_in_system32() {
        let root = temp_system_root("system32");
        let dir = root.join("System32");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wpcap.dll"), b"stub").unwrap();
        assert!(npcap_dll_present(&root));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn npcap_dll_absent_is_not_present() {
        let root = temp_system_root("absent");
        assert!(!npcap_dll_present(&root));
        // 同名目录不应被误判为 DLL。
        std::fs::create_dir_all(root.join("System32").join("wpcap.dll")).unwrap();
        assert!(!npcap_dll_present(&root));
        std::fs::remove_dir_all(&root).unwrap();
    }

    // ---------- Win32 冒烟（只读/幂等，本机可调用且不 panic） ----------

    #[test]
    fn schtasks_query_smoke() {
        // 仅要求可调用且返回确定的布尔值，不假设本机安装状态。
        let _installed = is_installed();
    }

    #[test]
    fn npcap_status_smoke() {
        // 三种结果都合法，关键是本机可调用且不 panic。
        match npcap_status() {
            NpcapStatus::Available | NpcapStatus::Missing | NpcapStatus::FoundButNotX64 => {}
        }
    }

    #[test]
    fn remove_legacy_run_value_smoke() {
        // 幂等清理：值本就可能不存在，不 panic 即可。
        remove_legacy_run_value();
    }

    #[test]
    fn wake_missing_event_returns_false() {
        assert!(!Guard::wake_other(&format!(
            "Local\\drcom4scutGUI-no-such-event-{}",
            std::process::id()
        )));
    }

    #[test]
    fn acquire_wake_and_release_roundtrip() {
        let name = format!("Local\\drcom4scutGUI-test-{}", std::process::id());
        let guard = match acquire(&name) {
            Ok(guard) => guard,
            Err(e) => panic!("acquire 应成功，实际失败：{e:?}"),
        };
        // 自身创建的唤起事件应可被 SetEvent（事件名是互斥体名 + "-Show"）。
        assert!(
            Guard::wake_other(&wake_event_name(&name)),
            "应能 SetEvent 自身创建的唤起事件"
        );
        drop(guard);
        // 释放后应能重新获取（互斥锁计数被 Drop 完整归零）。
        assert!(acquire(&name).is_ok(), "释放后应能重新获取单实例锁");
    }

    #[test]
    fn production_names_match_dot_net() {
        assert_eq!(wake_event_name(MUTEX_NAME), WAKE_EVENT_NAME);
    }

    #[test]
    fn on_wake_fires_callback() {
        let name = format!("Local\\drcom4scutGUI-test-wake-{}", std::process::id());
        let guard = match acquire(&name) {
            Ok(guard) => guard,
            Err(e) => panic!("acquire 应成功，实际失败：{e:?}"),
        };
        let flag = Arc::new(AtomicBool::new(false));
        let flag_in_thread = flag.clone();
        guard.on_wake(move || flag_in_thread.store(true, Ordering::SeqCst));
        assert!(Guard::wake_other(&wake_event_name(&name)));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !flag.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(flag.load(Ordering::SeqCst), "唤起回调未在 2 秒内触发");
        drop(guard);
    }
}
