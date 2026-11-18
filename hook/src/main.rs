use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_INPUT: u64 = 1_000_000;
const PROMPT_LIMIT: usize = 200;

fn main() {
    let _ = run();
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let probe = std::env::args().any(|arg| arg == "--probe");

    let mut input = String::new();
    std::io::stdin().take(MAX_INPUT).read_to_string(&mut input)?;
    let payload: serde_json::Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };

    let event = payload
        .get("hook_event_name")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if event != "UserPromptSubmit" && event != "Stop" {
        return Ok(());
    }

    let session_id = payload
        .get("session_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if session_id.is_empty() {
        return Ok(());
    }

    let turn_id = payload
        .get("turn_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let source = classify_source(&payload);
    let now = now_ms();
    let home = support_home();

    if probe {
        let record = serde_json::json!({
            "t": now,
            "event": event,
            "session_id": session_id,
            "turn_id": turn_id,
            "source": source.code,
            "evidence": source.evidence,
            "deep_link": deep_link(session_id),
        });
        append_json(&home.join("probe/events.jsonl"), &record)?;
        return Ok(());
    }

    if source.code != "desktop" {
        let record = serde_json::json!({ "t": now, "source": source.code });
        append_json(&home.join("events/rejected.jsonl"), &record)?;
        return Ok(());
    }

    let mut record = serde_json::json!({
        "t": now,
        "event": event,
        "session_id": session_id,
        "turn_id": turn_id,
        "source": source.code,
    });
    if event == "UserPromptSubmit" {
        if let Some(prompt) = payload.get("prompt").and_then(|value| value.as_str()) {
            record["prompt"] = serde_json::Value::String(excerpt(prompt));
        }
    }
    append_json(&home.join("events/queue.jsonl"), &record)?;
    Ok(())
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
            code: "desktop",
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
                code: "desktop",
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

fn deep_link(session_id: &str) -> String {
    format!("codex://threads/{}", encode_component(session_id))
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        let allowed = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~');
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
