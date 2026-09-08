//! 内嵌核心的提取、SHA-256 校验、子进程与 Job Object 管理。
//!
//! 行为对齐 .NET 版 `Services.cs` 的 `CoreResources.EnsureInstalled`（核心释放）
//! 与 `ClientController` / `JobObject`（启动、停止、同名进程检测）。

use std::ffi::OsStr;
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    CreateProcessW, ResumeThread, SetPriorityClass, TerminateProcess, WaitForSingleObject,
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, HIGH_PRIORITY_CLASS,
    PROCESS_INFORMATION, STARTUPINFOW,
};

use crate::paths;

/// 嵌入核心的 SHA-256（十六进制小写）。
pub const CORE_SHA256: &str = "ce79e117d14d172cb172a7d2a8adb2c638eb32d952db28d8ce04602eb5445ec2";

/// 嵌入的核心可执行文件字节（编译期校验哈希，见 `ensure_core_extracted_to`）。
pub const CORE_BYTES: &[u8] = include_bytes!("../resources/drcom4scut.exe");

/// 核心进程文件名，用于同名进程检测（对齐 .NET 版 `Process.GetProcessesByName("drcom4scut")`）。
pub const CORE_EXE_NAME: &str = "drcom4scut.exe";

/// 停止核心时等待其退出的时限（毫秒），对齐 .NET 版 `ClientController.Dispose` 中的 3 秒。
pub const CORE_STOP_TIMEOUT_MS: u32 = 3000;

// ---------------------------------------------------------------------------
// 内嵌核心提取
// ---------------------------------------------------------------------------

/// 计算核心字节的 SHA-256 十六进制串（小写）。
fn core_bytes_sha256() -> String {
    let digest = Sha256::digest(CORE_BYTES);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// 校验嵌入资源哈希；不符说明嵌入资源被篡换，属构建期错误，直接 panic。
fn verify_embedded_hash() {
    let actual = core_bytes_sha256();
    assert_eq!(
        actual, CORE_SHA256,
        "嵌入核心资源校验失败：期望 {CORE_SHA256}，实际 {actual}。嵌入资源被篡换，请重新构建。"
    );
}

/// 把核心释放到 `<runtime_root>/<CORE_SHA256>/drcom4scut.exe`。
///
/// 已存在且长度一致则跳过；否则以「临时文件 + rename」原子写入（Windows 上
/// `fs::rename` 等价 `MoveFileEx` + `REPLACE_EXISTING`，可覆盖旧文件）。
/// 返回目标完整路径。
pub fn ensure_core_extracted_to(runtime_root: &Path) -> io::Result<PathBuf> {
    verify_embedded_hash();
    let target = runtime_root.join(CORE_SHA256).join(CORE_EXE_NAME);
    if let Ok(metadata) = std::fs::metadata(&target) {
        if metadata.is_file() && metadata.len() == CORE_BYTES.len() as u64 {
            return Ok(target);
        }
    }
    write_file_atomically(&target, CORE_BYTES)?;
    Ok(target)
}

/// 薄封装：便携版释放到数据根 `runtime\<sha>\`；安装版只校验受保护核心，不尝试写入。
pub fn ensure_core_extracted() -> io::Result<PathBuf> {
    if crate::paths::is_installed() {
        let path = paths::core_exe(CORE_SHA256)
            .ok_or_else(|| io::Error::other("无法确定安装目录内的核心路径"))?;
        return verify_installed_core(&path).map(|()| path);
    }
    let runtime_root = paths::runtime_dir(CORE_SHA256)
        .and_then(|dir| dir.parent().map(Path::to_path_buf))
        .ok_or_else(|| io::Error::other("无法确定 runtime 目录"))?;
    ensure_core_extracted_to(&runtime_root)
}

fn verify_installed_core(path: &Path) -> io::Result<()> {
    let data = std::fs::read(path).map_err(|e| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("核心缺失或无法读取，请重新运行安装包修复。{e}"),
        )
    })?;
    let actual = {
        let digest = Sha256::digest(&data);
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    };
    if actual != CORE_SHA256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "核心文件损坏或被替换，请重新运行安装包修复。",
        ));
    }
    Ok(())
}

/// 原子写入：同目录临时文件 → `sync_all` → rename 覆盖目标。
/// 失败时清理残留临时文件。
fn write_file_atomically(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| io::Error::other(format!("目标路径缺少父目录：{}", target.display())))?;
    std::fs::create_dir_all(dir)?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result: io::Result<()> = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

// ---------------------------------------------------------------------------
// 命令行与环境块组装（纯函数，便于单元测试）
// ---------------------------------------------------------------------------

/// 组装命令行：`"核心路径" --config "配置路径"`。路径一律加引号以防空格。
/// 凭据绝不进入命令行，只经环境变量传递。
fn build_command_line(core_path: &Path, config_path: &Path) -> String {
    build_command_line_with_extra(core_path, config_path, &[])
}

/// 带附加参数的命令行构建（如 `--mac <MAC>`，对齐 .NET `Services.cs:545-557`）。
fn build_command_line_with_extra(core_path: &Path, config_path: &Path, extra: &[String]) -> String {
    let mut line = format!(
        "\"{}\" --config \"{}\"",
        core_path.display(),
        config_path.display()
    );
    for arg in extra {
        line.push(' ');
        line.push_str(arg);
    }
    line
}

/// 组建 UTF-16 环境块：继承 `base` 全部变量并覆盖 `DRCOM_USERNAME` / `DRCOM_PASSWORD`。
/// 键统一大写后按名称排序（Windows 环境块惯例），双 NUL 结尾。
fn build_env_block(
    username: &str,
    password: &str,
    base: impl IntoIterator<Item = (String, String)>,
) -> Vec<u16> {
    use std::collections::BTreeMap;
    // Windows 环境变量名不区分大小写，统一大写键以便覆盖同名变量。
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in base {
        vars.insert(key.to_uppercase(), value);
    }
    vars.insert("DRCOM_USERNAME".to_string(), username.to_string());
    vars.insert("DRCOM_PASSWORD".to_string(), password.to_string());
    let system_root = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
    let path_now = vars.get("PATH").cloned().unwrap_or_default();
    vars.insert(
        "PATH".to_string(),
        crate::install::driver::prepend_npcap_path(&path_now, &system_root),
    );

    let mut block = Vec::new();
    for (key, value) in &vars {
        block.extend(format!("{key}={value}").encode_utf16());
        block.push(0);
    }
    block.push(0); // 整块以空 NUL 结束
    block
}

/// 转换为 NUL 结尾的 UTF-16 字符串。
fn to_wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// 核心进程管理
// ---------------------------------------------------------------------------

/// 受管核心进程：持有 Job Object 与进程句柄，Drop 时关闭句柄。
/// Job 设置了 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，句柄关闭即兜底终止核心。
pub struct OwnedCore {
    job_handle: HANDLE,
    process_handle: HANDLE,
    pid: u32,
}

impl OwnedCore {
    /// 核心进程 PID。
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// 核心进程句柄（供监控模块限时等待等用途；所有权仍归本对象）。
    pub fn process_handle(&self) -> HANDLE {
        self.process_handle
    }

    /// 核心是否仍在运行（以 0 毫秒超时探测进程状态）。
    pub fn is_running(&self) -> bool {
        // SAFETY: 仅使用自身持有的有效进程句柄，等待 0 毫秒无阻塞副作用。
        unsafe { WaitForSingleObject(self.process_handle, 0) == WAIT_TIMEOUT }
    }

    /// 终止核心及其全部子进程（退出码 0），并限时等待退出。
    /// 返回 true 表示已确认退出；false 表示终止调用失败或等待超时
    /// （Drop 时的 KILL_ON_JOB_CLOSE 仍会兜底终止）。
    pub fn stop(&mut self) -> bool {
        // SAFETY: job_handle 为本对象创建并持有的有效 Job 句柄。
        if unsafe { TerminateJobObject(self.job_handle, 0) }.is_err() {
            return false;
        }
        // SAFETY: process_handle 为本对象持有的有效进程句柄，限时等待退出。
        unsafe { WaitForSingleObject(self.process_handle, CORE_STOP_TIMEOUT_MS) == WAIT_OBJECT_0 }
    }
}

impl Drop for OwnedCore {
    fn drop(&mut self) {
        // SAFETY: 两个句柄均由本对象创建/接收，且只会在此关闭一次。
        unsafe {
            let _ = CloseHandle(self.process_handle);
            // 关闭最后一个 Job 句柄时，KILL_ON_JOB_CLOSE 终止核心全部进程。
            let _ = CloseHandle(self.job_handle);
        }
    }
}

/// 创建带 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 限制的 Job Object
/// （对齐 .NET 版 `JobObject` 构造函数）。
fn create_kill_on_close_job() -> windows::core::Result<HANDLE> {
    // SAFETY: 匿名 Job、无安全属性，返回新建句柄。
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: 指针指向栈上完整结构体，长度与结构体一致。
    let result = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if let Err(e) = result {
        // SAFETY: 关闭刚创建、尚未交出的 Job 句柄。
        unsafe {
            let _ = CloseHandle(job);
        }
        return Err(e);
    }
    Ok(job)
}

/// 启动中途失败的清理：终止刚创建的挂起进程并关闭全部相关句柄。
///
/// # 前提
/// `info` 的句柄必须来自本次成功的 `CreateProcessW` 且尚未被关闭。
fn abort_launch(job: Option<HANDLE>, info: &PROCESS_INFORMATION) {
    // SAFETY: 句柄由刚成功的 CreateProcessW 返回，调用方保证尚未关闭、只处理一次。
    unsafe {
        let _ = TerminateProcess(info.hProcess, 1);
        let _ = CloseHandle(info.hThread);
        let _ = CloseHandle(info.hProcess);
        if let Some(job) = job {
            let _ = CloseHandle(job);
        }
    }
}

/// 启动核心：
///
/// 1. `CreateProcessW(CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW)`；
/// 2. 创建 KILL_ON_JOB_CLOSE 的 Job 并 `AssignProcessToJobObject`；
/// 3. `ResumeThread` 放行主线程，并提升为 High 优先级（对齐 .NET 版，失败忽略）。
///
/// 命令行为 `"核心路径" --config "配置路径"`；凭据仅通过环境变量
/// `DRCOM_USERNAME` / `DRCOM_PASSWORD` 传递，绝不写入命令行或磁盘。
pub fn spawn(
    core_path: &Path,
    config_path: &Path,
    username: &str,
    password: &str,
) -> io::Result<OwnedCore> {
    spawn_with_args(core_path, config_path, username, password, &[])
}

/// 同 [`spawn`]，但允许附加命令行参数（如 `--mac XX:XX:...`）。
pub fn spawn_with_args(
    core_path: &Path,
    config_path: &Path,
    username: &str,
    password: &str,
    extra_args: &[String],
) -> io::Result<OwnedCore> {
    let command_line = build_command_line_with_extra(core_path, config_path, extra_args);
    // CreateProcessW 可能就地改写命令行缓冲，必须提供可写缓冲。
    let mut cmd_wide: Vec<u16> = command_line
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let app_wide = to_wide_null(core_path.as_os_str());
    let env_block = build_env_block(
        username,
        password,
        std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))),
    );

    // 工作目录固定为用户数据根，使核心相对路径 `./logs` 落到对应用户的 logs 目录。
    let cwd_wide = crate::paths::root()
        .map(|p| to_wide_null(p.as_os_str()))
        .unwrap_or_else(|| vec![0]);

    let mut startup = STARTUPINFOW::default();
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut info = PROCESS_INFORMATION::default();

    // SAFETY: 应用名/命令行/环境块指针均指向存活至调用结束的有效缓冲区；
    // CREATE_SUSPENDED 保证进程在加入 Job 之前不会执行任何代码。
    unsafe {
        CreateProcessW(
            PCWSTR(app_wide.as_ptr()),
            Some(PWSTR(cmd_wide.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            Some(env_block.as_ptr().cast()),
            PCWSTR(cwd_wide.as_ptr()),
            &startup,
            &mut info,
        )
        .map_err(|e| io::Error::other(format!("启动核心进程失败：{e}")))?;
    }

    let job = match create_kill_on_close_job() {
        Ok(job) => job,
        Err(e) => {
            abort_launch(None, &info);
            return Err(io::Error::other(format!("创建 Job Object 失败：{e}")));
        }
    };

    // SAFETY: job 与 info.hProcess 均为刚创建的有效句柄。
    if let Err(e) = unsafe { AssignProcessToJobObject(job, info.hProcess) } {
        abort_launch(Some(job), &info);
        return Err(io::Error::other(format!("核心进程加入作业失败：{e}")));
    }

    // SAFETY: info.hThread 为 CreateProcessW 返回的挂起主线程句柄。
    if unsafe { ResumeThread(info.hThread) } == u32::MAX {
        abort_launch(Some(job), &info);
        return Err(io::Error::other("恢复核心主线程失败"));
    }
    // SAFETY: 主线程句柄用完即关，此后仅通过进程/作业句柄管理核心。
    let _ = unsafe { CloseHandle(info.hThread) };

    // 对齐 .NET 版 process.PriorityClass = High（失败忽略）。
    // SAFETY: info.hProcess 为有效进程句柄。
    let _ = unsafe { SetPriorityClass(info.hProcess, HIGH_PRIORITY_CLASS) };

    Ok(OwnedCore {
        job_handle: job,
        process_handle: info.hProcess,
        pid: info.dwProcessId,
    })
}

// ---------------------------------------------------------------------------
// 同名进程检测
// ---------------------------------------------------------------------------

/// 是否存在与核心同名的其它进程（对齐 .NET 版启动前的碰撞检测：
/// 发现同名进程时调用方应拒绝启动并显示原因）。
/// 快照创建失败按「无同名进程」处理，避免误拦启动。
pub fn same_name_running() -> bool {
    // SAFETY: 快照句柄在函数内创建并关闭；entry 由本函数初始化并维护 dwSize。
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(handle) => handle,
            Err(_) => return false,
        };
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                if process_name_matches(&entry.szExeFile) {
                    found = true;
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        found
    }
}

/// 判断 Toolhelp 条目中的进程名（NUL 结尾 UTF-16）是否为核心名（大小写不敏感）。
fn process_name_matches(name: &[u16]) -> bool {
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let name = String::from_utf16_lossy(&name[..len]);
    name.eq_ignore_ascii_case(CORE_EXE_NAME)
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::JobObjects::{
        JobObjectBasicProcessIdList, QueryInformationJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    };

    /// 测试用临时目录，Drop 时整体删除（即使断言失败也清理）。
    struct TempDirGuard(PathBuf);

    impl TempDirGuard {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "drcom4scut-coreproc-{tag}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            hex.push_str(&format!("{byte:02x}"));
        }
        hex
    }

    #[test]
    fn extract_creates_file_with_matching_hash() {
        let guard = TempDirGuard::new("extract");
        let exe = ensure_core_extracted_to(&guard.0).unwrap();
        assert_eq!(exe, guard.0.join(CORE_SHA256).join(CORE_EXE_NAME));
        let data = std::fs::read(&exe).unwrap();
        assert_eq!(data.len(), CORE_BYTES.len());
        assert_eq!(sha256_hex(&data), CORE_SHA256);
    }

    #[test]
    fn extract_is_idempotent() {
        let guard = TempDirGuard::new("idempotent");
        let first = ensure_core_extracted_to(&guard.0).unwrap();
        let mtime1 = std::fs::metadata(&first).unwrap().modified().unwrap();
        let second = ensure_core_extracted_to(&guard.0).unwrap();
        let mtime2 = std::fs::metadata(&second).unwrap().modified().unwrap();
        assert_eq!(first, second);
        // 第二次调用应命中「已存在且长度一致」而跳过重写，修改时间不变。
        assert_eq!(mtime1, mtime2);
        assert_eq!(sha256_hex(&std::fs::read(&second).unwrap()), CORE_SHA256);
    }

    #[test]
    fn extract_repairs_corrupted_file() {
        let guard = TempDirGuard::new("repair");
        let first = ensure_core_extracted_to(&guard.0).unwrap();
        std::fs::write(&first, b"corrupted").unwrap();
        let repaired = ensure_core_extracted_to(&guard.0).unwrap();
        assert_eq!(first, repaired);
        let data = std::fs::read(&repaired).unwrap();
        assert_eq!(data.len(), CORE_BYTES.len());
        assert_eq!(sha256_hex(&data), CORE_SHA256);
    }

    #[test]
    fn command_line_is_quoted_and_carries_config() {
        let line = build_command_line(
            &PathBuf::from(r"C:\Program Files\drcom4scut.exe"),
            &PathBuf::from(r"C:\Users\me\config y.yml"),
        );
        assert_eq!(
            line,
            r#""C:\Program Files\drcom4scut.exe" --config "C:\Users\me\config y.yml""#
        );
    }

    #[test]
    fn env_block_overrides_credentials_and_inherits_base() {
        let block = build_env_block(
            "stu@example",
            "secret=pass",
            vec![
                ("PATH".to_string(), r"C:\Windows".to_string()),
                ("drcom_username".to_string(), "old".to_string()),
            ],
        );
        let text = String::from_utf16(&block).unwrap();
        let entries: Vec<&str> = text.split('\0').filter(|s| !s.is_empty()).collect();
        assert!(entries
            .iter()
            .any(|e| e.starts_with("PATH=") && e.contains(r"C:\Windows")));
        assert!(entries.contains(&"DRCOM_USERNAME=stu@example"));
        assert!(entries.contains(&"DRCOM_PASSWORD=secret=pass"));
        // 同名（不区分大小写）变量被凭据覆盖。
        assert!(!entries.iter().any(|e| e.contains("old")));
        // 双 NUL 结尾，且按变量名排序。
        assert_eq!(&block[block.len() - 2..], &[0u16, 0]);
        let mut sorted = entries.clone();
        sorted.sort();
        assert_eq!(entries, sorted);
    }

    #[test]
    fn process_name_match_rules() {
        let wide = |s: &str| {
            let mut v: Vec<u16> = s.encode_utf16().collect();
            v.resize(260, 0);
            v
        };
        assert!(process_name_matches(&wide("drcom4scut.exe")));
        assert!(process_name_matches(&wide("DRCOM4SCUT.EXE")));
        assert!(!process_name_matches(&wide("drcom4scutGUI.exe")));
        assert!(!process_name_matches(&wide("drcom4scut.exe.bak")));
        assert!(!process_name_matches(&wide("")));
    }

    #[test]
    fn same_name_running_smoke() {
        // 结果取决于运行环境，只要求可安全调用并返回 bool。
        let _ = same_name_running();
    }

    #[test]
    fn kill_on_close_job_smoke() {
        let job = create_kill_on_close_job().expect("创建 KILL_ON_JOB_CLOSE 作业应成功");
        // 不把当前进程 assign 进去（KILL_ON_JOB_CLOSE 会杀死测试进程自身），
        // 改为查询作业内进程列表，确认当前没有任何子进程。
        let mut list = JOBOBJECT_BASIC_PROCESS_ID_LIST {
            NumberOfAssignedProcesses: 0,
            NumberOfProcessIdsInList: 0,
            ProcessIdList: [0],
        };
        // SAFETY: 缓冲区为栈上完整结构体，足以容纳 0 个 PID 的查询结果。
        unsafe {
            QueryInformationJobObject(
                Some(job),
                JobObjectBasicProcessIdList,
                std::ptr::from_mut(&mut list).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() as u32,
                None,
            )
            .expect("查询作业进程列表应成功");
        }
        assert_eq!(list.NumberOfAssignedProcesses, 0);
        assert_eq!(list.NumberOfProcessIdsInList, 0);
        // SAFETY: 关闭本测试创建的 Job 句柄（作业内无进程，无终止副作用）。
        unsafe {
            let _ = CloseHandle(job);
        }
    }
}
