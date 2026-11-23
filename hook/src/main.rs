use serde_json::json;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_INPUT: u64 = 1_000_000;
const PROMPT_LIMIT: usize = 200;
static FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceArg {
    Codex,
    KimiCode,
    WorkBuddy,
}

#[derive(Debug)]
struct NormalizedEvent {
    event: String,
    session_id: String,
    turn_id: String,
    source: String,
    prompt: Option<String>,
    session_title: Option<String>,
    cwd: Option<String>,
    client_type: Option<String>,
    evidence: String,
}

fn main() {
    let _ = run();
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let probe = args.iter().any(|arg| arg == "--probe");
    let source_arg = args
        .windows(2)
        .find(|pair| pair[0] == "--source")
        .and_then(|pair| match pair[1].as_str() {
            "codex" => Some(SourceArg::Codex),
            "kimi-code" => Some(SourceArg::KimiCode),
            "workbuddy" => Some(SourceArg::WorkBuddy),
            _ => None,
        })
        .unwrap_or(SourceArg::Codex);

    let mut input = String::new();
    std::io::stdin()
        .take(MAX_INPUT)
        .read_to_string(&mut input)?;
    let payload: serde_json::Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };
    let now = now_ms();
    let Some(event) = normalize_event(&payload, source_arg, now) else {
        return Ok(());
    };
    let home = support_home();

    if probe {
        let record = json!({
            "t": now,
            "event": event.event,
            "session_id": event.session_id,
            "turn_id": event.turn_id,
            "source": event.source,
            "evidence": event.evidence,
            "deep_link": deep_link(&event.source, &event.session_id),
        });
        append_json(&home.join("probe/events.jsonl"), &record)?;
        return Ok(());
    }

    let mut record = json!({
        "t": now,
        "event": event.event,
        "session_id": event.session_id,
        "turn_id": event.turn_id,
        "source": event.source,
    });
    if let Some(prompt) = event.prompt {
        record["prompt"] = json!(prompt);
    }
    if let Some(title) = event.session_title {
        record["session_title"] = json!(title);
    }
    if let Some(cwd) = event.cwd {
        record["cwd"] = json!(cwd);
    }
    if let Some(client_type) = event.client_type {
        record["client_type"] = json!(client_type);
    }
    write_inbox_event(&home, &record)?;
    Ok(())
}

fn normalize_event(
    payload: &serde_json::Value,
    source_arg: SourceArg,
    now: i64,
) -> Option<NormalizedEvent> {
    let event = string_field(payload, "hook_event_name")?;
    let session_id = string_field(payload, "session_id")?;
    if session_id.is_empty() {
        return None;
    }

    let (event, turn_id, source, evidence) = match source_arg {
        SourceArg::Codex => {
            let event = match event.as_str() {
                "UserPromptSubmit" | "Stop" | "Interrupt" | "PreModelRequest" => event,
                "PostModelRequest"
                    if matches!(
                        string_field(payload, "status").as_deref(),
                        Some("failed" | "cancelled")
                    ) =>
                {
                    "ModelRequestFailed".to_string()
                }
                _ => return None,
            };
            let source = classify_source(payload);
            let turn_id = event_turn_id(payload, &event, now);
            (event, turn_id, source.code.to_string(), source.evidence)
        }
        SourceArg::KimiCode => {
            let canonical_event = match event.as_str() {
                "TurnStarted" | "UserPromptSubmit" => "TurnStarted",
                "Stop" => "Stop",
                "Notification" => "Notification",
                "StopFailure" => "StopFailure",
                "Interrupt" => "Interrupt",
                _ => return None,
            };
            let turn_id = event_turn_id(payload, canonical_event, now);
            (
                canonical_event.to_string(),
                turn_id,
                "kimi-code".to_string(),
                "explicit:kimi-code".to_string(),
            )
        }
        SourceArg::WorkBuddy => {
            if !matches!(event.as_str(), "UserPromptSubmit" | "Stop") {
                return None;
            }
            if event == "Stop"
                && payload
                    .get("stop_hook_active")
                    .and_then(|value| value.as_bool())
                    == Some(true)
            {
                return None;
            }
            let turn_id = event_turn_id(payload, &event, now);
            (
                event.to_string(),
                turn_id,
                "workbuddy".to_string(),
                "explicit:workbuddy".to_string(),
            )
        }
    };

    let prompt = if event == "TurnStarted" || event == "UserPromptSubmit" {
        string_field(payload, "prompt").map(|value| excerpt(&value))
    } else {
        None
    };
    let client_type = string_field(payload, "client_type")
        .or_else(|| (source_arg == SourceArg::WorkBuddy).then(|| "workbuddy_desktop".to_string()));
    Some(NormalizedEvent {
        event,
        session_id,
        turn_id,
        source,
        prompt,
        session_title: string_field(payload, "session_title"),
        cwd: string_field(payload, "cwd"),
        client_type,
        evidence,
    })
}

fn event_turn_id(payload: &serde_json::Value, event: &str, now: i64) -> String {
    let turn_id = string_field(payload, "turn_id")
        .or_else(|| string_field(payload, "task_id"))
        .unwrap_or_else(|| format!("{}-{now}", event.to_lowercase()));
    turn_id
}

fn string_field(payload: &serde_json::Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
}

struct Source {
    code: &'static str,
    evidence: String,
}

fn classify_source(payload: &serde_json::Value) -> Source {
    let chain = ancestor_commands();

    if chain
        .iter()
        .any(|command| command.contains("/Applications/ChatGPT.app/"))
    {
        return Source {
            code: "codex",
            evidence: "ancestor:ChatGPT.app".to_string(),
        };
    }
    if chain.iter().any(|command| {
        command.contains("/Visual Studio Code.app/") || command.contains("Code Helper")
    }) {
        return Source {
            code: "ide",
            evidence: "ancestor:VisualStudioCode".to_string(),
        };
    }

    if let Some((originator, transcript_source)) = transcript_markers(payload) {
        if originator == "Codex Desktop" || originator == "codex_work_desktop" {
            return Source {
                code: "codex",
                evidence: format!("transcript:{originator}"),
            };
        }
        if originator == "codex-tui" || transcript_source == "cli" {
            return Source {
                code: "cli",
                evidence: format!("transcript:{originator}"),
            };
        }
    }

    if chain
        .iter()
        .any(|command| first_token_basename(command) == "codex")
    {
        return Source {
            code: "cli",
            evidence: "ancestor:codex-cli".to_string(),
        };
    }

    Source {
        code: "unknown",
        evidence: "no-match".to_string(),
    }
}

fn ancestor_commands() -> Vec<String> {
    let output = match Command::new("/bin/ps")
        .args(["-ww", "-axo", "pid=,ppid=,command="])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut table: HashMap<u32, (u32, String)> = HashMap::new();
    for line in text.lines() {
        let (pid, rest) = take_token(line);
        let (ppid, command) = take_token(rest);
        if let (Ok(pid), Ok(ppid)) = (pid.parse::<u32>(), ppid.parse::<u32>()) {
            table.insert(pid, (ppid, command.trim().to_string()));
        }
    }

    let mut chain = Vec::new();
    let mut current = std::process::id();
    for _ in 0..10 {
        let Some((ppid, command)) = table.get(&current) else {
            break;
        };
        chain.push(command.clone());
        if *ppid <= 1 {
            break;
        }
        current = *ppid;
    }
    chain
}

fn take_token(value: &str) -> (&str, &str) {
    let value = value.trim_start();
    match value.find(char::is_whitespace) {
        Some(index) => (&value[..index], &value[index..]),
        None => (value, ""),
    }
}

fn first_token_basename(command: &str) -> &str {
    let token = command.split_whitespace().next().unwrap_or("");
    token.rsplit('/').next().unwrap_or(token)
}

fn transcript_markers(payload: &serde_json::Value) -> Option<(String, String)> {
    let path = payload.get("transcript_path")?.as_str()?;
    let file = fs::File::open(path).ok()?;
    let mut line = String::new();
    std::io::BufReader::new(file).read_line(&mut line).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&line).ok()?;
    let originator = meta
        .pointer("/payload/originator")
        .and_then(|value| value.as_str())?
        .to_string();
    let source = meta
        .pointer("/payload/source")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    Some((originator, source))
}

fn excerpt(prompt: &str) -> String {
    let normalized = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(PROMPT_LIMIT).collect()
}

fn deep_link(source: &str, session_id: &str) -> Option<String> {
    if source != "codex" {
        return None;
    }
    Some(format!("codex://threads/{}", encode_component(session_id)))
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn support_home() -> PathBuf {
    if let Some(home) = std::env::var_os("IRISPORT_HOME") {
        return PathBuf::from(home);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join("Library/Application Support/Irisport"),
        None => PathBuf::from("/tmp/irisport"),
    }
}

fn write_inbox_event(home: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    let dir = home.join("events/inbox");
    fs::create_dir_all(&dir)?;
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));

    let mut line = serde_json::to_string(value).unwrap_or_default();
    line.push('\n');
    let unique = FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let base = format!("{}-{}-{unique}", now_ms(), std::process::id());
    let final_path = dir.join(format!("{base}.json"));
    let temp_path = dir.join(format!(".{base}.tmp"));

    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)?;
    let _ = fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600));
    file.write_all(line.as_bytes())?;
    drop(file);
    fs::rename(&temp_path, &final_path)?;
    Ok(())
}

fn append_json(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }

    let mut line = serde_json::to_string(value).unwrap_or_default();
    line.push('\n');

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    file.write_all(line.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kimi_turn_and_stop_keep_session_metadata() {
        let turn = normalize_event(
            &json!({
                "hook_event_name": "TurnStarted",
                "session_id": "session-1",
                "turn_id": "turn-1",
                "session_title": "修复设置页",
                "client_type": "kimi_code_desktop",
                "cwd": "/tmp/project",
                "prompt": "修复  设置页\n并补齐测试"
            }),
            SourceArg::KimiCode,
            100,
        )
        .unwrap();
        assert_eq!(turn.event, "TurnStarted");
        assert_eq!(turn.source, "kimi-code");
        assert_eq!(turn.session_title.as_deref(), Some("修复设置页"));
        assert_eq!(turn.cwd.as_deref(), Some("/tmp/project"));
        assert_eq!(turn.prompt.as_deref(), Some("修复 设置页 并补齐测试"));

        let stop = normalize_event(
            &json!({
                "hook_event_name": "Stop",
                "session_id": "session-1",
                "turn_id": "turn-1"
            }),
            SourceArg::KimiCode,
            200,
        )
        .unwrap();
        assert_eq!(stop.event, "Stop");
        assert_eq!(stop.turn_id, "turn-1");
    }

    #[test]
    fn kimi_background_completion_uses_task_id() {
        let event = normalize_event(
            &json!({
                "hook_event_name": "Notification",
                "session_id": "session-1",
                "task_id": "task-9",
                "session_title": "整理资料"
            }),
            SourceArg::KimiCode,
            300,
        )
        .unwrap();
        assert_eq!(event.event, "Notification");
        assert_eq!(event.turn_id, "task-9");
        assert!(event.prompt.is_none());
    }

    #[test]
    fn workbuddy_prompt_and_stop_are_normalized() {
        let prompt = normalize_event(
            &json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "wb-session",
                "prompt": "整理今日工作结果",
                "cwd": "/tmp/work"
            }),
            SourceArg::WorkBuddy,
            100,
        )
        .unwrap();
        assert_eq!(prompt.event, "UserPromptSubmit");
        assert_eq!(prompt.source, "workbuddy");
        assert_eq!(prompt.prompt.as_deref(), Some("整理今日工作结果"));
        assert_eq!(prompt.client_type.as_deref(), Some("workbuddy_desktop"));

        let stop = normalize_event(
            &json!({"hook_event_name":"Stop","session_id":"wb-session"}),
            SourceArg::WorkBuddy,
            200,
        )
        .unwrap();
        assert_eq!(stop.event, "Stop");
        assert!(!stop.turn_id.is_empty());
    }

    #[test]
    fn workbuddy_continuation_stop_is_ignored() {
        assert!(normalize_event(
            &json!({
                "hook_event_name":"Stop",
                "session_id":"wb-session",
                "stop_hook_active":true
            }),
            SourceArg::WorkBuddy,
            200,
        )
        .is_none());
    }

    #[test]
    fn codex_model_failure_is_normalized_without_accepting_success() {
        let failed = normalize_event(
            &json!({
                "hook_event_name": "PostModelRequest",
                "session_id": "s1",
                "turn_id": "t1",
                "status": "failed"
            }),
            SourceArg::Codex,
            200,
        )
        .unwrap();
        assert_eq!(failed.event, "ModelRequestFailed");
        assert_eq!(failed.turn_id, "t1");

        assert!(normalize_event(
            &json!({
                "hook_event_name": "PostModelRequest",
                "session_id": "s1",
                "turn_id": "t1",
                "status": "completed"
            }),
            SourceArg::Codex,
            300,
        )
        .is_none());
    }

    #[test]
    fn codex_model_attempt_start_is_preserved() {
        let event = normalize_event(
            &json!({
                "hook_event_name": "PreModelRequest",
                "session_id": "s1",
                "turn_id": "t1"
            }),
            SourceArg::Codex,
            100,
        )
        .unwrap();
        assert_eq!(event.event, "PreModelRequest");
        assert_eq!(event.turn_id, "t1");
    }

    #[test]
    fn unrelated_events_are_ignored() {
        for name in ["PostToolUse", "SessionStart"] {
            assert!(normalize_event(
                &json!({"hook_event_name": name, "session_id": "s1"}),
                SourceArg::KimiCode,
                100,
            )
            .is_none());
        }
    }

    #[test]
    fn missing_session_id_is_ignored() {
        assert!(normalize_event(
            &json!({"hook_event_name": "Stop", "turn_id": "t1"}),
            SourceArg::KimiCode,
            100,
        )
        .is_none());
    }
}
