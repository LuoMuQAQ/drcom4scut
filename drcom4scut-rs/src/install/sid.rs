//! 当前用户 SID 与令牌查询。

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// 返回当前进程用户的 SID 字符串（如 `S-1-5-21-...`）。
pub fn current_user_sid() -> Option<String> {
    sid_from_process(unsafe { GetCurrentProcess() })
}

pub fn sid_from_process(process: HANDLE) -> Option<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let sid = sid_from_token(token);
        let _ = CloseHandle(token);
        sid
    }
}

pub fn sid_from_token(token: HANDLE) -> Option<String> {
    unsafe {
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        if needed == 0 {
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        sid_to_string(user.User.Sid)
    }
}

pub fn sid_to_string(sid: windows::Win32::Security::PSID) -> Option<String> {
    unsafe {
        let mut raw = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut raw).ok()?;
        if raw.is_null() {
            return None;
        }
        let s = raw.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(raw.as_ptr() as *mut _)));
        s
    }
}

pub fn is_plausible_sid(sid: &str) -> bool {
    if !sid.starts_with("S-1-") {
        return false;
    }
    sid.bytes()
        .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
        && sid.matches('-').count() >= 3
        && sid.len() < 256
}

/// 宽字符串便利转换。
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn pcw(v: &[u16]) -> PCWSTR {
    PCWSTR(v.as_ptr())
}

pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::TOKEN_ELEVATION;
    use windows::Win32::Security::{GetTokenInformation, TokenElevation};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut needed = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elev as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut needed,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_sid_looks_valid() {
        let sid = current_user_sid().expect("应能读取当前用户 SID");
        assert!(is_plausible_sid(&sid), "sid={sid}");
        assert!(sid.starts_with("S-1-5-"));
    }

    #[test]
    fn sid_parser_rejects_garbage() {
        assert!(!is_plausible_sid(""));
        assert!(!is_plausible_sid("S-1"));
        assert!(!is_plausible_sid("../S-1-5-18"));
        assert!(!is_plausible_sid("S-1-5-21-hello"));
        assert!(is_plausible_sid("S-1-5-18"));
        assert!(is_plausible_sid("S-1-5-21-1-2-3-1001"));
    }
}
