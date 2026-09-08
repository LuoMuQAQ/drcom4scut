//! 网卡枚举与选择。
//!
//! 对齐 .NET 版 `Services.cs:273-333`（`AdapterService`）：
//! - [`enumerate`] ↔ `GetAdapters`（枚举 + 过滤 + 展示排序；不含「自动选择」虚拟项，该语义由调用方决定）；
//! - [`select`] ↔ `SelectDefault`（保存的 Id > 保存的 MAC > 唯一 Up+网关 > 自动项）。
//!
//! 实现说明：工程 `Cargo.toml`（工作流锁定，不可修改）仅启用 `Win32_NetworkManagement_IpHelper`
//! feature，而 windows 0.61 生成的 `GetAdaptersAddresses` / `IP_ADAPTER_ADDRESSES_LH`
//! 还额外要求 `Win32_NetworkManagement_Ndis` 与 `Win32_Networking_WinSock`
//! （二者未启用，对应符号被 `#[cfg]` 排除在编译之外）。因此本模块参照 windows crate
//! 自身 `windows-link` 的做法，以 raw-dylib 方式自行声明 iphlpapi.dll 的 FFI，
//! 并用 `#[repr(C)]` 镜像所需结构体前缀（与 iptypes.h 定义逐字段一致，已对照
//! windows-0.61.3 生成代码核对；二进制仅面向 x86_64）。

use crate::model::{normalize_mac, Adapter};

/// Preserve the selected adapter identity, but consult its current link state.
pub fn connection_available(selected: Option<&Adapter>, current: &[Adapter]) -> bool {
    let Some(chosen) = selected else {
        return current.iter().any(|a| a.is_up);
    };
    current
        .iter()
        .find(|a| a.id.eq_ignore_ascii_case(&chosen.id))
        .or_else(|| {
            current.iter().find(|a| {
                !chosen.mac.is_empty() && normalize_mac(&a.mac) == normalize_mac(&chosen.mac)
            })
        })
        .is_some_and(|a| a.is_up)
}

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
    GAA_FLAG_SKIP_MULTICAST, GET_ADAPTERS_ADDRESSES_FLAGS,
};

/// GetAdaptersAddresses 的 family 参数：仅枚举启用 IPv4 的接口（AF_INET = 2），
/// 即任务要求的 AF_INET2 过滤（.NET 版全量枚举后按类型过滤，此处由 API 直接限定）。
const AF_INET: u32 = 2;

/// Winsock 地址族（SOCKADDR.sa_family 字段取值）。
const AF_INET_SOCK: u16 = 2;

/// NDIS `IF_OPER_STATUS`（RFC 2863）：接口处于 Up。
/// windows crate 的 `IfOperStatusUp` 常量位于未启用的 Ndis 模块，按 ABI 取值本地声明。
const IF_OPER_STATUS_UP: i32 = 1;

/// IFTYPE 隧道接口（.NET 版 Services.cs:276 过滤 `NetworkInterfaceType.Tunnel`）。
const IF_TYPE_TUNNEL: u32 = 131;

/// 与 .NET 版一致的查询标记：含网关、跳过任意播/多播/DNS 服务器列表。
/// （windows crate 的 `BitOr` 实现非常量函数，故在常量求值中直接组合 `.0`。）
const GAA_FLAGS: GET_ADAPTERS_ADDRESSES_FLAGS = GET_ADAPTERS_ADDRESSES_FLAGS(
    GAA_FLAG_INCLUDE_GATEWAYS.0
        | GAA_FLAG_SKIP_ANYCAST.0
        | GAA_FLAG_SKIP_MULTICAST.0
        | GAA_FLAG_SKIP_DNS_SERVER.0,
);

// ---------------------------------------------------------------------------
// FFI 声明与结构体镜像（raw-dylib，无需 import library）
// ---------------------------------------------------------------------------

// SAFETY: 以 windows crate 同款 raw-dylib 方式链接 iphlpapi.dll；
// GetAdaptersAddresses 为 Win32 文档化导出函数。
#[link(name = "iphlpapi.dll", kind = "raw-dylib", modifiers = "+verbatim")]
extern "system" {
    fn GetAdaptersAddresses(
        family: u32,
        flags: GET_ADAPTERS_ADDRESSES_FLAGS,
        reserved: *const core::ffi::c_void,
        adapter_addresses: *mut RawAdapterAddresses,
        size_pointer: *mut u32,
    ) -> u32;
}

/// `IP_ADAPTER_ADDRESSES_LH` 前缀镜像：只声明到 `FirstGatewayAddress` 为止的字段，
/// 字段顺序与 C 定义一致，缓冲大小由 API 自行查询，故截断尾部字段不影响前缀读取。
#[repr(C)]
struct RawAdapterAddresses {
    /// union { ULONGLONG Alignment; { Length, IfIndex } }
    _alignment: u64,
    next: *mut RawAdapterAddresses,
    /// PCHAR，适配器 GUID 字符串（与 .NET `NetworkInterface.Id` 同源）
    adapter_name: *const u8,
    first_unicast_address: *mut RawAddressEntry,
    _first_anycast_address: *mut core::ffi::c_void,
    _first_multicast_address: *mut core::ffi::c_void,
    _first_dns_server_address: *mut core::ffi::c_void,
    _dns_suffix: *const u16,
    _description: *const u16,
    /// PWCHAR，连接名（如「以太网」「WLAN」）
    friendly_name: *const u16,
    /// BYTE[MAX_ADAPTER_ADDRESS_LENGTH=8]
    physical_address: [u8; 8],
    physical_address_length: u32,
    /// union { ULONG Flags; 位域 }
    _flags: u32,
    _mtu: u32,
    /// IFTYPE（回环=24、隧道=131 等）
    if_type: u32,
    /// IF_OPER_STATUS（NDIS/RFC 2863 取值，1 = Up）
    oper_status: i32,
    _ipv6_if_index: u32,
    _zone_indices: [u32; 16],
    _first_prefix: *mut core::ffi::c_void,
    _transmit_link_speed: u64,
    _receive_link_speed: u64,
    _first_wins_server_address: *mut core::ffi::c_void,
    first_gateway_address: *mut RawAddressEntry,
}

/// `SOCKET_ADDRESS` 镜像。
#[repr(C)]
struct RawSocketAddress {
    sockaddr: *mut RawSockaddr,
    _length: i32,
}

/// `SOCKADDR` 镜像：`sa_family` 之后是 14 字节负载；对 `SOCKADDR_IN` 而言
/// `data[0..2]` 为端口、`data[2..6]` 为 IPv4 地址。
#[repr(C)]
struct RawSockaddr {
    family: u16,
    data: [u8; 14],
}

/// `IP_ADAPTER_UNICAST_ADDRESS_LH` / `IP_ADAPTER_GATEWAY_ADDRESS_LH`
/// 的公共前缀镜像（{ Alignment } union + `Next` + `Address`，二者布局一致）。
#[repr(C)]
struct RawAddressEntry {
    _alignment: u64,
    next: *mut RawAddressEntry,
    address: RawSocketAddress,
}

// ---------------------------------------------------------------------------
// 枚举
// ---------------------------------------------------------------------------

/// 枚举本机启用 IPv4 的网卡，按 .NET 版 `Services.cs:280-282` 的顺序排序展示。
/// FFI 失败（含无 IPv4 接口的暂态）一律返回空列表，不让 GUI 崩溃。
pub fn enumerate() -> Vec<Adapter> {
    // SAFETY: GetAdaptersAddresses 按 Win32 契约调用：首次传空缓冲取得所需字节数，
    // 再传入按 8 字节对齐分配的缓冲二次调用；除该调用外不触及其他系统状态。
    unsafe {
        let mut size: u32 = 0;
        let rc = GetAdaptersAddresses(
            AF_INET,
            GAA_FLAGS,
            core::ptr::null(),
            core::ptr::null_mut(),
            &mut size,
        );
        if rc != ERROR_BUFFER_OVERFLOW.0 || size == 0 {
            // 典型失败：ERROR_NO_DATA / ERROR_ADDRESS_NOT_ASSOCIATED（无 IPv4 接口的暂态）。
            return Vec::new();
        }
        // 以 u64 为元素分配，保证链表节点要求的 8 字节对齐。
        let mut buffer = vec![0u64; size.div_ceil(8) as usize];
        let head = buffer.as_mut_ptr() as *mut RawAdapterAddresses;
        let rc = GetAdaptersAddresses(AF_INET, GAA_FLAGS, core::ptr::null(), head, &mut size);
        if rc != ERROR_SUCCESS.0 {
            // 缓冲在两次调用之间失效等暂态失败：返回空列表。
            return Vec::new();
        }
        // SAFETY: head 指向刚由 GetAdaptersAddresses 填充完成、尚未释放的缓冲。
        let mut adapters = parse_adapters(head);
        sort_adapters(&mut adapters);
        adapters
    }
}

/// 解析 GetAdaptersAddresses 返回的适配器链表。
///
/// 过滤规则（对齐 .NET `Services.cs:276-279`）：
/// - 隧道接口（IfType = 131）；
/// - 无 MAC 的接口（回环接口物理地址为空，顺带被排除）；
/// - 无 IPv4 单播地址的接口（AF_INET 过滤下理论上不会出现，防御性保留）。
///
/// # Safety
/// `head` 必须指向刚由 GetAdaptersAddresses 填充完成、且仍存活（未释放）的缓冲；
/// 链表节点与其中的字符串指针均在该缓冲内。
unsafe fn parse_adapters(head: *const RawAdapterAddresses) -> Vec<Adapter> {
    let mut adapters = Vec::new();
    let mut node = head;
    while !node.is_null() {
        // SAFETY: node 指向缓冲内合法节点；链表由 API 契约保证以空指针结尾。
        let raw = &*node;
        node = raw.next;

        if raw.if_type == IF_TYPE_TUNNEL {
            continue;
        }
        // 物理地址 → 十六进制串 → 规范化 MAC；空 MAC 视为回环等虚拟接口，跳过。
        let mac_len = raw
            .physical_address_length
            .min(raw.physical_address.len() as u32) as usize;
        let mac_hex: String = raw.physical_address[..mac_len]
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect();
        let mac = normalize_mac(&mac_hex);
        if mac.is_empty() {
            continue;
        }
        // SAFETY: friendly_name / adapter_name 由 iphlpapi 填写，指向缓冲内的 NUL 结尾字符串。
        let name = if raw.friendly_name.is_null() {
            String::new()
        } else {
            wide_to_string(raw.friendly_name)
        };
        let id = if raw.adapter_name.is_null() {
            String::new()
        } else {
            ansi_to_string(raw.adapter_name)
        };
        // 首个 IPv4 单播地址：仅用于确认接口确有 IPv4 地址（Adapter 模型与
        // .NET AdapterInfo 均不保存 IP 本身）。
        if !first_unicast_is_ipv4(raw.first_unicast_address) {
            continue;
        }
        let has_gateway = has_real_ipv4_gateway(raw.first_gateway_address);
        adapters.push(Adapter {
            id,
            mac,
            name,
            is_up: raw.oper_status == IF_OPER_STATUS_UP,
            has_ipv4_gateway: has_gateway,
        });
    }
    adapters
}

/// 展示排序（对齐 .NET `Services.cs:280-282`）：Up 优先，其次有 IPv4 网关，
/// 再按名称忽略大小写升序。稳定排序，同键条目保持原相对顺序。
fn sort_adapters(adapters: &mut [Adapter]) {
    adapters.sort_by(|a, b| {
        b.is_up
            .cmp(&a.is_up)
            .then_with(|| b.has_ipv4_gateway.cmp(&a.has_ipv4_gateway))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// 网关判定：链表中存在「IPv4 且非 0.0.0.0」的网关才算有网关。
/// 对齐 .NET `Services.cs:326-328`（排除 `IPAddress.Any`，0.0.0.0 仅为 on-link 路由）。
///
/// # Safety
/// `entry` 必须指向缓冲内的网关地址链表，或为空。
unsafe fn has_real_ipv4_gateway(entry: *const RawAddressEntry) -> bool {
    let mut node = entry;
    while !node.is_null() {
        // SAFETY: node 指向缓冲内合法节点；sockaddr 指向系统侧合法的 SOCKADDR。
        let sa = (*node).address.sockaddr;
        if !sa.is_null() && (*sa).family == AF_INET_SOCK {
            // 用 addr_of + read_unaligned 按值读网关字节，避免对裸指针解引用的隐式取引用。
            let data = unsafe {
                std::ptr::addr_of!((*sa).data)
                    .cast::<[u8; 14]>()
                    .read_unaligned()
            };
            if data[2..6] != [0, 0, 0, 0] {
                return true;
            }
        }
        node = (*node).next;
    }
    false
}

/// 首个单播地址是否为 IPv4（AF_INET 过滤下恒真，防御性校验）。
///
/// # Safety
/// `entry` 必须指向缓冲内的单播地址链表节点，或为空。
unsafe fn first_unicast_is_ipv4(entry: *const RawAddressEntry) -> bool {
    if entry.is_null() {
        return false;
    }
    // SAFETY: entry 指向缓冲内合法节点；sockaddr 指向合法的 SOCKADDR。
    let sa = (*entry).address.sockaddr;
    !sa.is_null() && (*sa).family == AF_INET_SOCK
}

/// 读取 NUL 结尾的 UTF-16 字符串（FriendlyName）。
///
/// # Safety
/// `ptr` 必须指向合法的 NUL 结尾 UTF-16 缓冲。
unsafe fn wide_to_string(ptr: *const u16) -> String {
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(core::slice::from_raw_parts(ptr, len))
}

/// 读取 NUL 结尾的 ANSI 字符串（AdapterName 的 GUID 串，实际为 ASCII）。
///
/// # Safety
/// `ptr` 必须指向合法的 NUL 结尾字节缓冲。
unsafe fn ansi_to_string(ptr: *const u8) -> String {
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf8_lossy(core::slice::from_raw_parts(ptr, len)).into_owned()
}

// ---------------------------------------------------------------------------
// 选择（纯函数，可单元测试）
// ---------------------------------------------------------------------------

/// 按优先级挑选适配器（对齐 .NET `SelectDefault`，Services.cs:288-302）：
/// 1. `saved_adapter_id` 不区分大小写精确命中；
/// 2. `normalize_mac(saved_mac)` 不区分大小写命中；
/// 3. 「Up 且有 IPv4 网关」的适配器恰好唯一时取之；
/// 4. 否则返回 `None`（.NET 的「自动选择」项语义由调用方决定）。
pub fn select(saved_adapter_id: &str, saved_mac: &str, adapters: &[Adapter]) -> Option<usize> {
    if let Some(index) = adapters
        .iter()
        .position(|a| a.id.eq_ignore_ascii_case(saved_adapter_id))
    {
        return Some(index);
    }
    let mac = normalize_mac(saved_mac);
    if let Some(index) = adapters
        .iter()
        .position(|a| normalize_mac(&a.mac) == mac && !mac.is_empty())
    {
        return Some(index);
    }
    let candidates: Vec<usize> = adapters
        .iter()
        .enumerate()
        .filter(|(_, a)| a.is_up && a.has_ipv4_gateway)
        .map(|(i, _)| i)
        .collect();
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造合成网卡条目（不触碰真实网络 API）。
    fn adapter(id: &str, mac: &str, name: &str, is_up: bool, gateway: bool) -> Adapter {
        Adapter {
            id: id.to_string(),
            mac: mac.to_string(),
            name: name.to_string(),
            is_up,
            has_ipv4_gateway: gateway,
        }
    }

    #[test]
    fn select_prefers_saved_id_hit() {
        let adapters = [
            adapter("{AAA}", "aa:bb:cc:dd:ee:01", "以太网", true, true),
            adapter("{BBB}", "aa:bb:cc:dd:ee:02", "WLAN", true, true),
        ];
        // 存在多个「Up+网关」候选时，保存的 Id 依然优先。
        assert_eq!(select("{BBB}", "", &adapters), Some(1));
    }

    #[test]
    fn reconnect_uses_live_link_state_without_switching_selected_adapter() {
        let old = adapter("wifi", "aa:bb:cc:dd:ee:01", "WLAN", false, true);
        let mut live = old.clone();
        live.is_up = true;
        assert!(connection_available(Some(&old), &[live.clone()]));
        live.is_up = false;
        let other = adapter("ethernet", "aa:bb:cc:dd:ee:02", "LAN", true, true);
        assert!(!connection_available(Some(&old), &[live, other.clone()]));
        assert!(!connection_available(Some(&old), &[other.clone()]));
        assert!(connection_available(None, &[other]));
    }

    #[test]
    fn select_id_hit_is_case_insensitive() {
        let adapters = [adapter(
            "{AAA-1}",
            "aa:bb:cc:dd:ee:01",
            "以太网",
            false,
            false,
        )];
        // GUID 大小写不敏感（对齐 .NET OrdinalIgnoreCase）。
        assert_eq!(select("{aaa-1}", "", &adapters), Some(0));
    }

    #[test]
    fn select_id_hit_beats_mac_hit() {
        let adapters = [
            adapter("{AAA}", "aa:bb:cc:dd:ee:01", "以太网", true, true),
            adapter("{BBB}", "aa:bb:cc:dd:ee:02", "WLAN", true, true),
        ];
        // Id 与 MAC 各命中不同条目时，Id 优先。
        assert_eq!(select("{BBB}", "AA-BB-CC-DD-EE-01", &adapters), Some(1));
    }

    #[test]
    fn select_falls_back_to_saved_mac_hit() {
        let adapters = [
            adapter("{AAA}", "aa:bb:cc:dd:ee:01", "以太网", true, true),
            adapter("{BBB}", "AA-BB-CC-DD-EE-02", "WLAN", true, true),
        ];
        // Id 未命中时按 normalize_mac 后的 MAC 匹配（分隔符/大小写无关）。
        assert_eq!(select("{XXX}", "aabbccddee02", &adapters), Some(1));
        assert_eq!(select("{XXX}", "AA:BB:CC:DD:EE:02", &adapters), Some(1));
    }

    #[test]
    fn select_picks_unique_up_adapter_with_gateway() {
        let adapters = [
            adapter("{AAA}", "aa:bb:cc:dd:ee:01", "以太网", true, false),
            adapter("{BBB}", "aa:bb:cc:dd:ee:02", "WLAN", true, true),
            adapter("{CCC}", "aa:bb:cc:dd:ee:03", "蓝牙", false, true),
        ];
        // 无保存信息时，「Up 且有网关」恰好唯一 → 取该条目。
        assert_eq!(select("", "", &adapters), Some(1));
    }

    #[test]
    fn select_returns_none_when_multiple_up_with_gateway() {
        let adapters = [
            adapter("{AAA}", "aa:bb:cc:dd:ee:01", "以太网", true, true),
            adapter("{BBB}", "aa:bb:cc:dd:ee:02", "WLAN", true, true),
        ];
        // 多个「Up+网关」候选无法自动裁决 → None（自动项语义由调用方决定）。
        assert_eq!(select("", "", &adapters), None);
    }

    #[test]
    fn select_returns_none_when_nothing_matches() {
        let adapters = [
            adapter("{AAA}", "aa:bb:cc:dd:ee:01", "以太网", false, true),
            adapter("{BBB}", "aa:bb:cc:dd:ee:02", "WLAN", false, false),
        ];
        // Id/MAC 均未命中且无唯一 Up+网关候选 → None。
        assert_eq!(select("", "", &adapters), None);
        assert_eq!(select("{ZZZ}", "11:22:33:44:55:66", &adapters), None);
    }

    #[test]
    fn sort_adapters_orders_up_gateway_then_name() {
        let mut adapters = vec![
            adapter("{D}", "d", "Down+网关", false, true),
            adapter("{C}", "c", "Up无网关", true, false),
            adapter("{B}", "b", "以太网", true, true),
            adapter("{A}", "a", "WLAN", true, true),
        ];
        sort_adapters(&mut adapters);
        let names: Vec<&str> = adapters.iter().map(|a| a.name.as_str()).collect();
        // Up 优先 → 其次有网关 → 名称忽略大小写升序（拉丁字母排在 CJK 前）。
        assert_eq!(names, ["WLAN", "以太网", "Up无网关", "Down+网关"]);
    }
}
