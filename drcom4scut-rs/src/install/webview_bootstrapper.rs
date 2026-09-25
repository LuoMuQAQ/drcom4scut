//! 微软官方 Edge WebView2 运行时在线安全补装模块。
//! 遵循原则：征得用户同意、微软官方来源、数字签名 Authenticode 校验、失败安全退出。

use std::path::Path;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONINFORMATION, MB_YESNO};

use crate::install::driver;
use crate::ui::webview::{self, RuntimeStatus};
use crate::ui::winutil::wide;

const OFFICIAL_BOOTSTRAPPER_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// 检查 WebView2 运行时，若缺失则征得用户同意后从微软官方安全补装。
pub fn ensure_runtime_or_install(hwnd: HWND) -> Result<(), String> {
    if matches!(webview::detect_runtime(), RuntimeStatus::Installed { .. }) {
        return Ok(());
    }

    // 1. 弹窗征得用户同意
    let ask_text = wide(
        "检测到您的系统尚未安装 Microsoft Edge WebView2 运行时。\n\
         安装程序需要此组件以呈现现代化安装界面。\n\n\
         是否允许立即从微软官方安全下载并安装 WebView2 运行时？",
    );
    let choice = unsafe {
        MessageBoxW(
            Some(hwnd),
            PCWSTR(ask_text.as_ptr()),
            w!("校园网客户端安装"),
            MB_YESNO | MB_ICONINFORMATION,
        )
    };

    if choice != IDYES {
        return Err("已取消 WebView2 运行时安装，尚未开始安装校园网客户端。".into());
    }

    // 2. 准备安全临时文件路径
    let temp_dir = std::env::temp_dir().join(format!("drcom-wv2-setup-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let installer_path = temp_dir.join("MicrosoftEdgeWebview2Setup.exe");

    // 3. 复用已有 winhttp_download 从微软官方安全下载
    driver::winhttp_download(OFFICIAL_BOOTSTRAPPER_URL, &installer_path, &mut |_, _| true)
        .map_err(|e| format!("下载微软官方安装引导器失败: {e}"))?;

    // 4. 复用已有 authenticode_publisher 校验微软数字签名
    let publisher = driver::authenticode_publisher(&installer_path)
        .map_err(|e| format!("数字签名验证失败: {e}"))?;

    if !publisher.contains("Microsoft Corporation") {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(format!(
            "下载的安装包数字签名未通过微软验证（签署者: {publisher}），已安全终止。"
        ));
    }

    // 5. 启动安装程序并等待完成
    run_bootstrapper(&installer_path)?;

    let _ = std::fs::remove_dir_all(&temp_dir);

    // 6. 重新检查是否安装成功
    if matches!(webview::detect_runtime(), RuntimeStatus::Installed { .. }) {
        Ok(())
    } else {
        Err("WebView2 运行时安装未能完成，请手动安装后重试。".into())
    }
}

fn run_bootstrapper(path: &Path) -> Result<(), String> {
    let wide_exe = wide(&path.to_string_lossy());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpFile: PCWSTR(wide_exe.as_ptr()),
        nShow: 1,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| format!("启动安装程序失败: {e}"))?;
        if info.hProcess.is_invalid() {
            return Err("安装程序未返回有效句柄。".into());
        }
        let wait = WaitForSingleObject(info.hProcess, u32::MAX);
        let mut code = 0;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if wait != WAIT_OBJECT_0 || code != 0 {
            return Err(format!("安装程序退出码异常: {code}"));
        }
    }
    Ok(())
}
