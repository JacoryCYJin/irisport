use crate::paths;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use toml_edit::{value, ArrayOfTables, DocumentMut, Item, Table};

const MARKER: &str = "irisport-hook.sh";
const BASE_EVENTS: [&str; 3] = ["TurnStarted", "Stop", "Notification"];
const RULES: [(&str, Option<&str>); 5] = [
    ("TurnStarted", None),
    ("Stop", None),
    ("StopFailure", None),
    ("Interrupt", None),
    ("Notification", Some("task\\.completed")),
];

pub fn config_path() -> PathBuf {
    paths::kimi_code_config_path()
}

pub fn install(command: &str) -> Result<(), String> {
    install_at(&config_path(), command)
}

pub fn uninstall() -> Result<usize, String> {
    uninstall_at(&config_path())
}

pub fn expected_entries() -> usize {
    RULES.len()
}

pub fn configured() -> Result<(bool, usize), String> {
    configured_at(&config_path())
}

fn install_at(path: &Path, command: &str) -> Result<(), String> {
    let mode = file_mode(path)?;
    let mut document = load(path)?;
    let hooks = hooks_mut(&mut document)?;
    let mut added = 0;

    for (event, matcher) in RULES {
        let exists = hooks.iter().any(|table| {
            table.get("event").and_then(|item| item.as_str()) == Some(event)
                && table
                    .get("command")
                    .and_then(|item| item.as_str())
                    .map(|command| command.contains(MARKER))
                    .unwrap_or(false)
        });
        if exists {
            continue;
        }
        let mut table = Table::new();
        table["event"] = value(event);
        if let Some(matcher) = matcher {
            table["matcher"] = value(matcher);
        }
        table["command"] = value(command);
        table["timeout"] = value(5);
        hooks.push(table);
        added += 1;
    }

    if added > 0 {
        backup(path)?;
        write_atomic(path, &document, mode)?;
    }
    Ok(())
}

fn uninstall_at(path: &Path) -> Result<usize, String> {
    if !path.exists() {
        return Ok(0);
    }
    let mode = file_mode(path)?;
    let mut document = load(path)?;
    let Some(hooks) = document
        .as_table_mut()
        .get_mut("hooks")
        .and_then(|item| item.as_array_of_tables_mut())
    else {
        return Ok(0);
    };

    let mut removed = 0;
    let mut index = 0;
    while index < hooks.len() {
        let owned = hooks
            .get(index)
            .and_then(|table| table.get("command"))
            .and_then(|item| item.as_str())
            .map(|command| command.contains(MARKER))
            .unwrap_or(false);
        if owned {
            hooks.remove(index);
            removed += 1;
        } else {
            index += 1;
        }
    }
    if removed > 0 {
        write_atomic(path, &document, mode)?;
    }
    Ok(removed)
}

fn configured_at(path: &Path) -> Result<(bool, usize), String> {
    let document = load(path)?;
    let Some(hooks) = document
        .as_table()
        .get("hooks")
        .and_then(|item| item.as_array_of_tables())
    else {
        return Ok((false, 0));
    };

    let mut entries = 0;
    let mut base_entries = 0;
    for (event, _) in RULES {
        if hooks.iter().any(|table| {
            table.get("event").and_then(|item| item.as_str()) == Some(event)
                && table
                    .get("command")
                    .and_then(|item| item.as_str())
                    .map(|command| command.contains(MARKER))
                    .unwrap_or(false)
        }) {
            entries += 1;
            if BASE_EVENTS.contains(&event) {
                base_entries += 1;
            }
        }
    }
    Ok((base_entries == BASE_EVENTS.len(), entries))
}

fn hooks_mut(document: &mut DocumentMut) -> Result<&mut ArrayOfTables, String> {
    let item = document
        .as_table_mut()
        .entry("hooks")
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()));
    item.as_array_of_tables_mut()
        .ok_or_else(|| "config.toml 中的 hooks 不是 [[hooks]] 数组，已拒绝修改".to_string())
}

fn load(path: &Path) -> Result<DocumentMut, String> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    if text.trim().is_empty() {
        return Ok(DocumentMut::new());
    }
    text.parse::<DocumentMut>()
        .map_err(|error| format!("Kimi Code config.toml 不是合法 TOML，已拒绝修改: {error}"))
}

fn file_mode(path: &Path) -> Result<u32, String> {
    if !path.exists() {
        return Ok(0o600);
    }
    fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o777)
        .map_err(|error| error.to_string())
}

fn backup(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let target = path.with_file_name(format!("config.toml.irisport-backup-{}", now_secs()));
    fs::copy(path, target).map_err(|error| format!("备份 Kimi Code config.toml 失败: {error}"))?;
    Ok(())
}

fn write_atomic(path: &Path, document: &DocumentMut, mode: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp = path.with_file_name(format!(".config-{}.toml.tmp", std::process::id()));
    fs::write(&temp, document.to_string()).map_err(|error| error.to_string())?;
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
            "irisport-kimi-{name}-{}-{}.toml",
            std::process::id(),
            now_secs()
        ))
    }

    #[test]
    fn install_preserves_existing_hooks_and_uninstalls_only_irisport() {
        let path = temp_path("preserve");
        fs::write(
            &path,
            "# user config\n[[hooks]]\nevent = \"Stop\"\ncommand = \"echo done\"\n",
        )
        .unwrap();
        install_at(&path, "/bin/sh /tmp/irisport-hook.sh --source kimi-code").unwrap();
        install_at(&path, "/bin/sh /tmp/irisport-hook.sh --source kimi-code").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# user config"));
        assert_eq!(text.matches("echo done").count(), 1);
        assert_eq!(text.matches("TurnStarted").count(), 1);
        assert_eq!(text.matches("task\\.completed").count(), 1);
        assert_eq!(configured_at(&path).unwrap(), (true, RULES.len()));

        assert_eq!(uninstall_at(&path).unwrap(), RULES.len());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("echo done"));
        assert!(!text.contains(MARKER));
        assert_eq!(configured_at(&path).unwrap(), (false, 0));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn base_rules_are_connected_without_optional_rules() {
        let path = temp_path("base-rules");
        fs::write(
            &path,
            "[[hooks]]\nevent = \"TurnStarted\"\ncommand = \"/tmp/irisport-hook.sh\"\n\n[[hooks]]\nevent = \"Stop\"\ncommand = \"/tmp/irisport-hook.sh\"\n\n[[hooks]]\nevent = \"Notification\"\nmatcher = \"task\\\\.completed\"\ncommand = \"/tmp/irisport-hook.sh\"\n",
        )
        .unwrap();
        assert_eq!(configured_at(&path).unwrap(), (true, BASE_EVENTS.len()));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn invalid_hooks_shape_is_not_modified() {
        let path = temp_path("invalid");
        fs::write(&path, "hooks = \"invalid\"\n").unwrap();
        assert!(install_at(&path, "/bin/sh /tmp/irisport-hook.sh").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "hooks = \"invalid\"\n");
        let _ = fs::remove_file(path);
    }
}
