use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MARKER: &str = "irisport-hook.sh";
const BASE_EVENTS: [&str; 2] = ["UserPromptSubmit", "Stop"];
const EVENTS: [&str; 5] = [
    "UserPromptSubmit",
    "Stop",
    "Interrupt",
    "PreModelRequest",
    "PostModelRequest",
];

pub fn hooks_path() -> PathBuf {
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        return PathBuf::from(home).join("hooks.json");
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".codex/hooks.json"),
        None => PathBuf::from(".codex/hooks.json"),
    }
}

pub fn install(command: &str) -> Result<(), String> {
    let path = hooks_path();
    let exists = path.exists();
    let mode = if exists {
        fs::metadata(&path)
            .map(|meta| meta.permissions().mode() & 0o777)
            .unwrap_or(0o600)
    } else {
        0o600
    };

    let mut root = load()?;
    if exists {
        let backup = path.with_file_name(format!("hooks.json.irisport-backup-{}", now_secs()));
        fs::copy(&path, &backup).map_err(|error| format!("备份 hooks.json 失败: {error}"))?;
    }

    let object = root
        .as_object_mut()
        .ok_or_else(|| "hooks.json 顶层不是对象".to_string())?;
    let hooks_value = object
        .entry("hooks".to_string())
        .or_insert_with(|| json!({}));
    let hooks = hooks_value
        .as_object_mut()
        .ok_or_else(|| "hooks 不是对象".to_string())?;

    let mut added = 0;
    for event in EVENTS {
        let entry = hooks
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let groups = entry
            .as_array_mut()
            .ok_or_else(|| format!("hooks.{event} 不是数组"))?;
        if count_marker(groups) > 0 {
            continue;
        }
        groups.push(json!({
            "hooks": [{
                "type": "command",
                "command": command,
                "async": true,
                "timeout": 5
            }]
        }));
        added += 1;
    }

    if added > 0 {
        write_atomic(&path, &root, mode)?;
    }

    Ok(())
}

pub fn uninstall() -> Result<usize, String> {
    let path = hooks_path();
    if !path.exists() {
        return Ok(0);
    }
    let mode = fs::metadata(&path)
        .map(|meta| meta.permissions().mode() & 0o777)
        .unwrap_or(0o600);
    let mut root = load()?;

    let Some(object) = root.as_object_mut() else {
        return Err("hooks.json 顶层不是对象".to_string());
    };
    let Some(Value::Object(mut hooks)) = object.remove("hooks") else {
        return Ok(0);
    };

    let mut removed = 0;
    let keys: Vec<String> = hooks.keys().cloned().collect();
    for key in keys {
        let Some(Value::Array(groups)) = hooks.remove(&key) else {
            continue;
        };
        let mut kept_groups = Vec::new();
        for mut group in groups {
            let Some(handlers) = group
                .get_mut("hooks")
                .and_then(|value| value.as_array_mut())
            else {
                kept_groups.push(group);
                continue;
            };
            let before = handlers.len();
            handlers.retain(|handler| {
                !handler
                    .get("command")
                    .and_then(|value| value.as_str())
                    .map(|command| command.contains(MARKER))
                    .unwrap_or(false)
            });
            removed += before - handlers.len();
            if !handlers.is_empty() {
                kept_groups.push(group);
            }
        }
        if !kept_groups.is_empty() {
            hooks.insert(key, Value::Array(kept_groups));
        }
    }

    if removed > 0 {
        if !hooks.is_empty() {
            object.insert("hooks".to_string(), Value::Object(hooks));
        }
        write_atomic(&path, &root, mode)?;
    }
    Ok(removed)
}

pub fn expected_entries() -> usize {
    EVENTS.len()
}

pub fn configured() -> Result<(bool, usize), String> {
    configuration_state(&load()?)
}

fn configuration_state(root: &Value) -> Result<(bool, usize), String> {
    let Some(hooks) = root.get("hooks").and_then(|value| value.as_object()) else {
        return Ok((false, 0));
    };
    let mut entries = 0;
    let mut base_entries = 0;
    for event in EVENTS {
        if let Some(groups) = hooks.get(event).and_then(|value| value.as_array()) {
            let count = count_marker(groups);
            entries += count;
            if BASE_EVENTS.contains(&event) && count > 0 {
                base_entries += 1;
            }
        }
    }
    Ok((base_entries == BASE_EVENTS.len(), entries))
}

fn load() -> Result<Value, String> {
    let path = hooks_path();
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("hooks.json 不是合法 JSON，已拒绝修改: {error}"))?;
    if !value.is_object() {
        return Err("hooks.json 顶层不是对象".to_string());
    }
    Ok(value)
}

fn write_atomic(path: &Path, value: &Value, mode: u32) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp = path.with_file_name(format!(".hooks-{}.json.tmp", std::process::id()));
    fs::write(&temp, format!("{text}\n")).map_err(|error| error.to_string())?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(mode))
        .map_err(|error| error.to_string())?;
    fs::rename(&temp, path).map_err(|error| error.to_string())?;
    Ok(())
}

fn count_marker(groups: &[Value]) -> usize {
    let mut count = 0;
    for group in groups {
        let Some(handlers) = group.get("hooks").and_then(|value| value.as_array()) else {
            continue;
        };
        for handler in handlers {
            if handler
                .get("command")
                .and_then(|value| value.as_str())
                .map(|command| command.contains(MARKER))
                .unwrap_or(false)
            {
                count += 1;
            }
        }
    }
    count
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_rules_are_connected_without_optional_interrupt() {
        let root = json!({
            "hooks": {
                "UserPromptSubmit": [{"hooks": [{"command": "/tmp/irisport-hook.sh"}]}],
                "Stop": [{"hooks": [{"command": "/tmp/irisport-hook.sh"}]}]
            }
        });
        assert_eq!(configuration_state(&root).unwrap(), (true, 2));
    }

    #[test]
    fn optional_interrupt_does_not_replace_a_base_rule() {
        let root = json!({
            "hooks": {
                "Stop": [{"hooks": [{"command": "/tmp/irisport-hook.sh"}]}],
                "Interrupt": [{"hooks": [{"command": "/tmp/irisport-hook.sh"}]}]
            }
        });
        assert_eq!(configuration_state(&root).unwrap(), (false, 2));
    }
}
