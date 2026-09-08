//! PE 头解析：只读文件字节，不加载镜像。

pub const IMAGE_FILE_MACHINE_I386: u16 = 0x014c;
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;

pub fn pe_machine(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 0x40 {
        return None;
    }
    if bytes[0] != b'M' || bytes[1] != b'Z' {
        return None;
    }
    let e_lfanew = u32::from_le_bytes(bytes[0x3c..0x40].try_into().ok()?) as usize;
    if e_lfanew.checked_add(6)? > bytes.len() {
        return None;
    }
    if &bytes[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes(
        bytes[e_lfanew + 4..e_lfanew + 6].try_into().ok()?,
    ))
}

pub fn is_amd64_pe(bytes: &[u8]) -> bool {
    pe_machine(bytes) == Some(IMAGE_FILE_MACHINE_AMD64)
}

pub fn is_i386_pe(bytes: &[u8]) -> bool {
    pe_machine(bytes) == Some(IMAGE_FILE_MACHINE_I386)
}

/// 最小合法 PE 字节，供测试使用。
pub fn synthetic_pe(machine: u16) -> Vec<u8> {
    let mut b = vec![0u8; 0x80];
    b[0] = b'M';
    b[1] = b'Z';
    b[0x3c] = 0x40;
    b[0x40] = b'P';
    b[0x41] = b'E';
    b[0x44] = machine as u8;
    b[0x45] = (machine >> 8) as u8;
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_and_rejects_garbage() {
        assert!(is_amd64_pe(&synthetic_pe(IMAGE_FILE_MACHINE_AMD64)));
        assert!(is_i386_pe(&synthetic_pe(IMAGE_FILE_MACHINE_I386)));
        assert!(!is_amd64_pe(b"MZ"));
        assert!(!is_amd64_pe(b""));
        assert!(!is_amd64_pe(&synthetic_pe(IMAGE_FILE_MACHINE_I386)));
    }

    #[test]
    fn embedded_core_is_amd64() {
        let bytes = crate::coreproc::CORE_BYTES;
        assert!(is_amd64_pe(bytes), "嵌入核心必须是 x64 PE");
    }
}
