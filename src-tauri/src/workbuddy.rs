use crate::paths;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MARKER: &str = "irisport-hook.sh";
const EVENTS: [&str; 2] = ["UserPromptSubmit", "Stop"];

pub fn config_path() -> PathBuf {
    paths::workbuddy_home().join("settings.json")
}

pub fn install(command: &str) -> Result<(), String> {
    install_at(&config_path(), command)
}

pub fn uninstall() -> Result<usize, String> {
    uninstall_at(&config_path())
}

pub fn expected_entries() -> usize {
    EVENTS.len()
}

pub fn configured() -> Result<(bool, usize), String> {
    configured_at(&config_path())
}

fn install_at(path: &Path, command: &str) -> Result<(), String> {
    let exists = path.exists();
    let mode = file_mode(path)?;
    let mut root = load(path)?;
    if exists {
        let backup = path.with_file_name(format!("settings.json.irisport-backup-{}", now_secs()));
        fs::copy(path, backup)
            .map_err(|error| format!("备份 WorkBuddy settings.json 失败: {error}"))?;
    }

    let object = root
        .as_object_mut()
        .ok_or_else(|| "WorkBuddy settings.json 顶层不是对象".to_string())?;
    let hooks_value = object
        .entry("hooks".to_string())
        .or_insert_with(|| json!({}));
    let hooks = hooks_value
        .as_object_mut()
        .ok_or_else(|| "WorkBuddy settings.json 中 hooks 不是对象".to_string())?;

    let mut added = 0;
    for event in EVENTS {
        let entry = hooks
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let groups = entry
            .as_array_mut()
            .ok_or_else(|| format!("WorkBuddy hooks.{event} 不是数组"))?;
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
        write_atomic(path, &root, mode)?;
    }
    Ok(())
}

fn uninstall_at(path: &Path) -> Result<usize, String> {
    if !path.exists() {
        return Ok(0);
    }
    let mode = file_mode(path)?;
    let mut root = load(path)?;
    let Some(object) = root.as_object_mut() else {
        return Err("WorkBuddy settings.json 顶层不是对象".to_string());
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
        write_atomic(path, &root, mode)?;
    }
    Ok(removed)
}

fn configured_at(path: &Path) -> Result<(bool, usize), String> {
    let root = load(path)?;
    let Some(hooks) = root.get("hooks").and_then(|value| value.as_object()) else {
        return Ok((false, 0));
    };
    let mut entries = 0;
    for event in EVENTS {
        if let Some(groups) = hooks.get(event).and_then(|value| value.as_array()) {
            entries += count_marker(groups);
        }
    }
    Ok((entries == EVENTS.len(), entries))
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

fn load(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("WorkBuddy settings.json 不是合法 JSON，已拒绝修改: {error}"))?;
    if !value.is_object() {
        return Err("WorkBuddy settings.json 顶层不是对象".to_string());
    }
    Ok(value)
}

fn file_mode(path: &Path) -> Result<u32, String> {
    if !path.exists() {
        return Ok(0o600);
    }
    fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o777)
        .map_err(|error| error.to_string())
}

fn write_atomic(path: &Path, value: &Value, mode: u32) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp = path.with_file_name(format!(".settings-{}.json.tmp", std::process::id()));
    fs::write(&temp, format!("{text}\n")).map_err(|error| error.to_string())?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(mode))
        .map_err(|error| error.to_string())?;
    fs::rename(&temp, path).map_err(|error| error.to_string())?;
    Ok(())
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

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "irisport-workbuddy-{name}-{}-{}.json",
            std::process::id(),
            now_secs()
        ))
    }

    #[test]
    fn install_preserves_settings_and_other_hooks() {
        let path = temp_path("preserve");
        fs::write(
            &path,
            r#"{
  "theme": "dark",
  "hooks": {
    "Stop": [{"hooks": [{"type": "command", "command": "echo other"}]}]
  }
}"#,
        )
        .unwrap();
        install_at(&path, "/bin/sh /tmp/irisport-hook.sh --source workbuddy").unwrap();
        install_at(&path, "/bin/sh /tmp/irisport-hook.sh --source workbuddy").unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value.get("theme").and_then(Value::as_str), Some("dark"));
        assert_eq!(configured_at(&path).unwrap(), (true, 2));
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("echo other").count(), 1);
        assert_eq!(text.matches("UserPromptSubmit").count(), 1);

        assert_eq!(uninstall_at(&path).unwrap(), 2);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("echo other"));
        assert!(!text.contains(MARKER));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn invalid_hooks_shape_is_not_modified() {
        let path = temp_path("invalid");
        fs::write(&path, r#"{"hooks":"invalid"}"#).unwrap();
        assert!(install_at(&path, "/bin/sh /tmp/irisport-hook.sh").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"hooks":"invalid"}"#);
        let _ = fs::remove_file(path);
    }
}
