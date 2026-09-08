//! 核心日志行分类与连接状态判定。
//!
//! 行为对齐 .NET 版 `Services.cs:347-408` 的 `CoreStatusReader`：
//! - 健康信号：含「Authorization success」「Heartbeat done」「Send Heartbeat」；
//! - 「but ignored」行保持中性（.NET 版直接 continue 跳过）；
//! - 「Fatal error」「Send error」视为错误。
//! 在此基础上补充 `Waiting` 信号，用于识别「本时段禁止上网 / 等待定时重连」类日志行。
//! .NET 版从未把任何日志行映射为 Waiting（该状态只由 GUI 的网络不可用 / 退避计时产生），
//! 此处按核心（vendor/drcom4scut-0.3.2）真实日志补充，属有意增强，详见 [`WAITING_KEYWORDS`]。
//!
//! 本模块为纯逻辑，不做任何文件 I/O；日志文件的读取与 tail 属于 `coreproc` 的职责。

use crate::model::LinkState;

/// 单条日志行的分类结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// 健康信号：认证成功或心跳正常，映射到 [`LinkState::Online`]。
    Healthy,
    /// 中性信号：明确无害但不说明健康（如 "but ignored"），不改变当前状态。
    Neutral,
    /// 错误信号：核心报错（"Fatal error" / "Send error"），映射到 [`LinkState::Error`]。
    Error,
    /// 等待信号：被服务器禁止上网或等待定时重连，映射到 [`LinkState::Waiting`]。
    Waiting,
    /// 其他：启动、收发、登出等流程性日志，不改变当前状态。
    Other,
}

/// 健康关键词，与 .NET 版 `Services.cs:376-379` 的 `IsHealthyLine` 完全一致。
const HEALTHY_KEYWORDS: [&str; 3] = ["Authorization success", "Heartbeat done", "Send Heartbeat"];

/// 错误关键词，与 .NET 版 `Services.cs:359-360` 一致。
const ERROR_KEYWORDS: [&str; 2] = ["Fatal error", "Send error"];

/// 中性关键词："but ignored" 行一律跳过，与 .NET 版 `Services.cs:358` 的 continue 一致。
const NEUTRAL_KEYWORD: &str = "but ignored";

/// 等待关键词，对应核心源码（vendor/drcom4scut-0.3.2）中「本时段禁止上网 / 等待定时重连」：
/// - 服务器通知原文「本时段禁止上网」（eap.rs:417 原样记录到日志）；
/// - ErrCode=16 的英文等价行 "You are not allowed to access the internet now."（eap.rs:437）；
/// - 进入定时休眠后的 "Will try reconnect at the next 7:00."（main.rs:95）。
///
/// 注意两点易混淆行不属于等待：
/// - "Waiting SUCCESS message from EAP."（udp.rs:465）是正常登录流程行；
/// - "Will try reconnect in N second(s)" 是普通失败重试（"in" 而非 "at the next"）。
const WAITING_KEYWORDS: [&str; 4] = [
    "禁止上网",
    "not allowed to access the internet",
    "Will try reconnect at the next",
    "Will try restart UDP heartbeat at the next",
];

/// 判断是否为健康信号行。与 .NET 版 `Services.cs:376-379` 的 `IsHealthyLine` 等价。
pub fn is_healthy_line(line: &str) -> bool {
    HEALTHY_KEYWORDS.iter().any(|k| line.contains(k))
}

/// 对单条日志行分类。
///
/// 匹配均为大小写敏感的子串匹配，与 .NET 版 `StringComparison.Ordinal` 一致；
/// 优先级复刻 .NET 版逐行判定的先后顺序：
/// 1. 含 "but ignored" → 中性（.NET 版最先 continue，即使行内含其他关键词）；
/// 2. 含 "Fatal error" / "Send error" → 错误；
/// 3. 含健康关键词 → 健康；
/// 4. 含等待关键词 → 等待；
/// 5. 其余 → 其他。
pub fn classify(line: &str) -> Signal {
    if line.contains(NEUTRAL_KEYWORD) {
        return Signal::Neutral;
    }
    if ERROR_KEYWORDS.iter().any(|k| line.contains(k)) {
        return Signal::Error;
    }
    if is_healthy_line(line) {
        return Signal::Healthy;
    }
    if WAITING_KEYWORDS.iter().any(|k| line.contains(k)) {
        return Signal::Waiting;
    }
    Signal::Other
}

/// 将信号应用到当前状态，返回新状态（纯函数）。
///
/// 滞回规则：
/// - 显式信号直接决定状态：Healthy→Online、Error→Error、Waiting→Waiting；
///   其中 Error 状态只能由 [`Signal::Error`] 触发，任何其他信号都不会产生 Error。
/// - 中性/其他信号不改变当前状态：日志里大量的流程行与 "but ignored" 行
///   不会把状态冲掉，只有下一条显著信号才能翻转状态。
pub fn apply(state: LinkState, signal: Signal) -> LinkState {
    match signal {
        Signal::Healthy => LinkState::Online,
        Signal::Error => LinkState::Error,
        Signal::Waiting => LinkState::Waiting,
        Signal::Neutral | Signal::Other => state,
    }
}

/// 按时间顺序扫描日志行，折叠出最终连接状态。
///
/// 与 .NET 版 `CoreStatusReader.Read` 的「倒序找最新显著行」语义等价：
/// 正序折叠 + 中性/其他行不改变状态 ⇒ 最后一条显著行决定结果。
///
/// 空输入返回 [`LinkState::Offline`]。与 .NET 版的差异：.NET 在「核心运行中但日志
/// 无任何显著行」时兜底为 Connecting，该兜底需要进程存活信息，由调用方（coreproc）
/// 结合进程状态处理，本函数只负责日志本身。
pub fn scan_lines(lines: impl Iterator<Item = String>) -> LinkState {
    lines.fold(LinkState::Offline, |state, line| {
        apply(state, classify(&line))
    })
}

/// 尾部扫描判定（对齐 .NET `Services.cs:353-373` 的倒序语义）。
///
/// 从最新行往回找第一条显著行（跳过含 "but ignored" 的中性行）：
/// 健康行 → [`TailVerdict::Online`]；错误行 → [`TailVerdict::Error`]；
/// 全部不显著 → [`TailVerdict::Connecting`]（调用方需保证核心在运行才使用该兜底）。
pub fn scan_tail(lines: &[&str]) -> TailVerdict {
    for line in lines.iter().rev() {
        match classify(line) {
            Signal::Healthy => return TailVerdict::Online,
            Signal::Error => return TailVerdict::Error,
            _ => continue,
        }
    }
    TailVerdict::Connecting
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailVerdict {
    Online,
    Error,
    Connecting,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_tail_most_recent_signal_wins() {
        let lines = [
            "[2026-09-07 13:32:19] Send Logoff packet.",
            "[2026-09-07 13:32:20] Fatal error at EAP Process thread!",
            "[2026-09-07 13:32:21] 802.1X Authorization success!",
        ];
        assert_eq!(scan_tail(&lines), TailVerdict::Online);
        let lines_err = &lines[..2];
        assert_eq!(scan_tail(lines_err), TailVerdict::Error);
    }

    #[test]
    fn scan_tail_ignores_neutral_lines() {
        let lines = [
            "[2026-09-07 13:32:19] Start to run...",
            "[2026-09-07 13:32:20] Heartbeat timeout. No packet for 24s, but ignored.",
            "[2026-09-07 13:32:21] Send Alive.",
        ];
        assert_eq!(scan_tail(&lines), TailVerdict::Connecting);
    }

    #[test]
    fn scan_tail_empty_is_connecting() {
        assert_eq!(scan_tail(&[]), TailVerdict::Connecting);
    }

    /// 表驱动：单行分类。样例取自核心（vendor/drcom4scut-0.3.2）真实日志输出。
    #[test]
    fn classify_real_log_samples() {
        let cases: &[(&str, Signal)] = &[
            // —— 健康信号 ——
            ("802.1X Authorization success!", Signal::Healthy),
            (
                "[INFO][EAP-Process][drcom4scut::eap:473] 802.1X Authorization success!",
                Signal::Healthy,
            ),
            ("Heartbeat done.", Signal::Healthy),
            (
                "[INFO][UDP-Process][drcom4scut::udp:432] Heartbeat done.",
                Signal::Healthy,
            ),
            ("Send Heartbeat(Response, Identity) packet.", Signal::Healthy),
            // —— 中性信号：but ignored 保持中性 ——
            ("but ignored", Signal::Neutral),
            (
                "[ERROR][UDP-Heartbeat][drcom4scut::udp:508] Heartbeat timeout. No Misc Heartbeat packet received for 24s, but ignored.",
                Signal::Neutral,
            ),
            // —— 错误信号 ——
            (
                "Fatal error at EAP Process thread! Will try restart in 15 second(s).",
                Signal::Error,
            ),
            (
                "[ERROR] Fatal error at UDP Process thread! Will try restart in 15 second(s).",
                Signal::Error,
            ),
            (
                "Send error: Os { code: 10064, kind: HostUnreachable, message: \"A socket operation was attempted to an unreachable host.\" }",
                Signal::Error,
            ),
            // —— 等待信号：本时段禁止上网 / 等待定时重连 ——
            ("本时段禁止上网", Signal::Waiting),
            (
                "[ERROR] You are not allowed to access the internet now.",
                Signal::Waiting,
            ),
            ("Will try reconnect at the next 7:00.", Signal::Waiting),
            // —— 其他：流程行 ——
            ("Start to run...", Signal::Other),
            ("Create EAP Process.", Signal::Other),
            (
                "[INFO][UDP-Process][drcom4scut::udp:600] Send Alive.",
                Signal::Other,
            ),
            ("Send MiscAlive.", Signal::Other),
            ("Send Logoff packet.", Signal::Other),
            // 易误判样例：含 "Waiting" 字样但属于正常登录流程
            ("Waiting SUCCESS message from EAP.", Signal::Other),
            // 普通重试行（"in N second(s)" 与定时等待的 "at the next" 相区分）
            (
                "Failed at 802.1X Authorization! Will try reconnect in 15 second(s).",
                Signal::Other,
            ),
            ("Will try reconnect in 15 second(s).", Signal::Other),
            ("Server Information: 通知公告", Signal::Other),
            ("", Signal::Other),
        ];
        for (line, expected) in cases {
            assert_eq!(classify(line), *expected, "分类不符: {line}");
        }
    }

    /// 健康行判断与 .NET 版 `IsHealthyLine` 的三个关键词一致。
    #[test]
    fn is_healthy_line_matches_dotnet_keywords() {
        assert!(is_healthy_line("802.1X Authorization success!"));
        assert!(is_healthy_line("Heartbeat done."));
        assert!(is_healthy_line(
            "Send Heartbeat(Response, Identity) packet."
        ));
        assert!(is_healthy_line("Send Heartbeat"));
        assert!(!is_healthy_line(
            "Heartbeat timeout. No Misc Heartbeat packet received for 24s, but ignored."
        ));
        assert!(!is_healthy_line("Send Alive."));
        assert!(!is_healthy_line(""));
    }

    /// 表驱动：apply 的滞回规则。
    #[test]
    fn apply_hysteresis() {
        let cases: &[(LinkState, Signal, LinkState)] = &[
            (LinkState::Offline, Signal::Healthy, LinkState::Online),
            (LinkState::Error, Signal::Healthy, LinkState::Online),
            (LinkState::Online, Signal::Error, LinkState::Error),
            (LinkState::Offline, Signal::Error, LinkState::Error),
            (LinkState::Online, Signal::Waiting, LinkState::Waiting),
            (LinkState::Error, Signal::Waiting, LinkState::Waiting),
            // 中性/其他信号不改变状态（滞回：噪声不冲掉显著状态）
            (LinkState::Online, Signal::Neutral, LinkState::Online),
            (LinkState::Error, Signal::Neutral, LinkState::Error),
            (LinkState::Offline, Signal::Neutral, LinkState::Offline),
            (LinkState::Online, Signal::Other, LinkState::Online),
            (LinkState::Error, Signal::Other, LinkState::Error),
            (LinkState::Waiting, Signal::Other, LinkState::Waiting),
            (LinkState::Offline, Signal::Other, LinkState::Offline),
        ];
        for &(state, signal, expected) in cases {
            assert_eq!(
                apply(state, signal),
                expected,
                "apply({state:?}, {signal:?}) 不符"
            );
        }
    }

    /// 端到端扫描：每条样例的分类已在 classify 表中断言，这里断言整段日志的最终状态。
    #[test]
    fn scan_lines_end_to_end() {
        // 空输入 → Offline
        let empty: Vec<String> = Vec::new();
        assert_eq!(scan_lines(empty.into_iter()), LinkState::Offline);

        // 正常上线流程（含大量"其他"流程行，均不改变状态）
        let happy: Vec<String> = vec![
            "Start to run...".into(),
            "Create EAP Process.".into(),
            "802.1X Authorization success!".into(),
            "[INFO][UDP-Process][drcom4scut::udp:600] Send Alive.".into(),
            "Heartbeat done.".into(),
        ];
        assert_eq!(scan_lines(happy.iter().cloned()), LinkState::Online);

        // 健康之后报错 → Error
        let then_error = happy
            .iter()
            .cloned()
            .chain(["Fatal error at UDP Process thread! Will try restart in 15 second(s).".into()]);
        assert_eq!(scan_lines(then_error), LinkState::Error);

        // "but ignored" 中性行不改变健康状态
        let with_ignored = happy.iter().cloned().chain([
            "Heartbeat timeout. No Misc Heartbeat packet received for 24s, but ignored.".into(),
        ]);
        assert_eq!(scan_lines(with_ignored), LinkState::Online);

        // 禁止上网 → 等待定时重连 → Waiting
        let forbidden: Vec<String> = vec![
            "802.1X Authorization success!".into(),
            "Heartbeat done.".into(),
            "本时段禁止上网".into(),
            "Will try reconnect at the next 7:00.".into(),
        ];
        assert_eq!(scan_lines(forbidden.into_iter()), LinkState::Waiting);

        // 只有流程行 / 登出行，无任何显著信号 → 保持 Offline
        let noise: Vec<String> = vec!["Start to run...".into(), "Send Logoff packet.".into()];
        assert_eq!(scan_lines(noise.into_iter()), LinkState::Offline);

        // 时序敏感：最后一条显著行决定状态（对齐 .NET 倒序扫描语义）
        let recover = [
            "Fatal error at EAP Process thread! Will try restart in 15 second(s).".to_string(),
            "802.1X Authorization success!".to_string(),
        ];
        assert_eq!(scan_lines(recover.into_iter()), LinkState::Online);

        let degrade = [
            "802.1X Authorization success!".to_string(),
            "Fatal error at EAP Process thread! Will try restart in 15 second(s).".to_string(),
        ];
        assert_eq!(scan_lines(degrade.into_iter()), LinkState::Error);
    }
}
