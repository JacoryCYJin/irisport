/// Source-agnostic descriptions of where a reminder came from and how it can
/// be acknowledged or opened. Codex-specific behavior belongs in this mapping,
/// not in the panel/store/commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Codex,
    CommandCode,
    ChatGptWeb,
    Unknown,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadSignal {
    /// The source gives a trusted read receipt.
    Trusted,
    /// The browser/source can only infer that the result was seen.
    Inferred,
    /// The user must explicitly acknowledge it.
    Manual,
    /// No reliable signal exists yet.
    Unsupported,
    /// Waiting for an allowed official signal.
    Pending,
}

pub fn kind(source: &str) -> SourceKind {
    match source {
        "desktop" | "codex" => SourceKind::Codex,
        "command-code" => SourceKind::CommandCode,
        "chatgpt-web" => SourceKind::ChatGptWeb,
        _ => SourceKind::Unknown,
    }
}

pub fn label(source: &str) -> &'static str {
    match kind(source) {
        SourceKind::Codex => "Codex",
        SourceKind::CommandCode => "Command Code",
        SourceKind::ChatGptWeb => "ChatGPT",
        SourceKind::Unknown => "Unknown",
    }
}

pub fn read_signal(source: &str) -> ReadSignal {
    match kind(source) {
        SourceKind::Codex => ReadSignal::Trusted,
        SourceKind::CommandCode => ReadSignal::Unsupported,
        SourceKind::ChatGptWeb => ReadSignal::Pending,
        SourceKind::Unknown => ReadSignal::Unsupported,
    }
}

/// The URL/deep link used by a row click. Returning `None` means the source has
/// no reliable way to return to the original conversation yet.
pub fn open_url(source: &str, session_id: &str) -> Option<String> {
    match kind(source) {
        SourceKind::Codex => Some(format!(
            "codex://threads/{}",
            encode_component(session_id)
        )),
        _ => None,
    }
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        let allowed =
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
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
    fn codex_is_the_only_trusted_read_source_for_now() {
        assert_eq!(read_signal("desktop"), ReadSignal::Trusted);
        assert_eq!(read_signal("command-code"), ReadSignal::Unsupported);
        assert_eq!(read_signal("chatgpt-web"), ReadSignal::Pending);
        assert_eq!(read_signal("other"), ReadSignal::Unsupported);
    }

    #[test]
    fn only_codex_has_a_verified_open_target() {
        assert_eq!(
            open_url("desktop", "abc-123"),
            Some("codex://threads/abc-123".to_string())
        );
        assert_eq!(open_url("command-code", "abc-123"), None);
        assert_eq!(open_url("chatgpt-web", "abc-123"), None);
    }

    #[test]
    fn labels_are_source_specific() {
        assert_eq!(label("desktop"), "Codex");
        assert_eq!(label("command-code"), "Command Code");
        assert_eq!(label("chatgpt-web"), "ChatGPT");
    }
}
