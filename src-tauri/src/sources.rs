#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Codex,
    KimiCode,
    WorkBuddy,
    CommandCode,
    ChatGptWeb,
    Unknown,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadSignal {
    Trusted,
    Inferred,
    Unsupported,
}

pub const CODEX: &str = "codex";
pub const KIMI_CODE: &str = "kimi-code";
pub const WORKBUDDY: &str = "workbuddy";
pub const ALL: [&str; 3] = [CODEX, KIMI_CODE, WORKBUDDY];

pub fn kind(source: &str) -> SourceKind {
    match source {
        "desktop" | "codex" => SourceKind::Codex,
        KIMI_CODE => SourceKind::KimiCode,
        WORKBUDDY => SourceKind::WorkBuddy,
        "command-code" => SourceKind::CommandCode,
        "chatgpt-web" => SourceKind::ChatGptWeb,
        _ => SourceKind::Unknown,
    }
}

pub fn canonical(source: &str) -> &str {
    match kind(source) {
        SourceKind::Codex => CODEX,
        SourceKind::KimiCode => KIMI_CODE,
        SourceKind::WorkBuddy => WORKBUDDY,
        _ => source,
    }
}

pub fn label(source: &str) -> &'static str {
    match kind(source) {
        SourceKind::Codex => "ChatGPT",
        SourceKind::KimiCode => "Kimi Code",
        SourceKind::WorkBuddy => "WorkBuddy",
        SourceKind::CommandCode => "Command Code",
        SourceKind::ChatGptWeb => "ChatGPT",
        SourceKind::Unknown => "Unknown",
    }
}

pub fn description(source: &str) -> &'static str {
    match kind(source) {
        SourceKind::Codex => "通过本地 hooks 接收 ChatGPT 桌面端的进行中状态、工作时间和完成结果。",
        SourceKind::KimiCode => "接收 Kimi Code 的任务状态、工作时间和完成结果；在原应用查看后通过真实未读状态清除提醒。",
        SourceKind::WorkBuddy => "接收 WorkBuddy 的任务状态、工作时间和完成结果；在原应用查看后通过真实未读状态清除提醒。",
        SourceKind::CommandCode => "Command Code 完成事件仍处于验证阶段。",
        SourceKind::ChatGptWeb => "ChatGPT 网页来源仍处于评估阶段。",
        SourceKind::Unknown => "未知来源。",
    }
}

pub fn read_signal(source: &str) -> ReadSignal {
    match kind(source) {
        SourceKind::Codex => ReadSignal::Trusted,
        SourceKind::KimiCode | SourceKind::WorkBuddy => ReadSignal::Trusted,
        SourceKind::CommandCode | SourceKind::ChatGptWeb | SourceKind::Unknown => {
            ReadSignal::Unsupported
        }
    }
}

pub fn accepts_events(source: &str) -> bool {
    matches!(
        kind(source),
        SourceKind::Codex | SourceKind::KimiCode | SourceKind::WorkBuddy
    )
}

pub fn supports_open(source: &str) -> bool {
    matches!(kind(source), SourceKind::Codex)
}

pub fn open_url(source: &str, session_id: &str) -> Option<String> {
    match kind(source) {
        SourceKind::Codex => Some(format!("codex://threads/{}", encode_component(session_id))),
        _ => None,
    }
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        let allowed = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if allowed {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_signals_match_source_capabilities() {
        assert_eq!(read_signal("desktop"), ReadSignal::Trusted);
        assert_eq!(read_signal("kimi-code"), ReadSignal::Trusted);
        assert_eq!(read_signal("workbuddy"), ReadSignal::Trusted);
        assert_eq!(read_signal("command-code"), ReadSignal::Unsupported);
        assert_eq!(read_signal("other"), ReadSignal::Unsupported);
    }

    #[test]
    fn only_supported_sources_accept_events_and_open_results() {
        assert!(accepts_events("desktop"));
        assert!(accepts_events("kimi-code"));
        assert!(accepts_events("workbuddy"));
        assert!(!accepts_events("command-code"));
        assert!(supports_open("codex"));
        assert!(!supports_open("kimi-code"));
        assert!(!supports_open("workbuddy"));
        assert!(!supports_open("command-code"));
    }

    #[test]
    fn codex_has_a_verified_open_target() {
        assert_eq!(
            open_url("desktop", "abc-123"),
            Some("codex://threads/abc-123".to_string())
        );
        assert_eq!(open_url("kimi-code", "abc-123"), None);
        assert_eq!(open_url("workbuddy", "abc-123"), None);
        assert_eq!(open_url("command-code", "abc-123"), None);
    }

    #[test]
    fn labels_and_legacy_sources_are_normalized() {
        assert_eq!(label("desktop"), "ChatGPT");
        assert_eq!(canonical("desktop"), "codex");
        assert_eq!(label("kimi-code"), "Kimi Code");
        assert_eq!(label("workbuddy"), "WorkBuddy");
        assert_eq!(label("command-code"), "Command Code");
        assert_eq!(label("chatgpt-web"), "ChatGPT");
    }
}
