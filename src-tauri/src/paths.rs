use std::path::PathBuf;

pub fn support_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("IRISPORT_HOME") {
        return PathBuf::from(home);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join("Library/Application Support/Irisport"),
        None => PathBuf::from("/tmp/irisport"),
    }
}

pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(support_dir().join("events/inbox"))?;
    std::fs::create_dir_all(support_dir().join("logs"))?;
    std::fs::create_dir_all(hook_bin_dir())?;
    Ok(())
}

pub fn db_path() -> PathBuf {
    support_dir().join("irisport.sqlite3")
}

pub fn inbox_dir() -> PathBuf {
    support_dir().join("events/inbox")
}

pub fn legacy_queue_path() -> PathBuf {
    support_dir().join("events/queue.jsonl")
}

pub fn legacy_rejected_path() -> PathBuf {
    support_dir().join("events/rejected.jsonl")
}

pub fn probe_dir() -> PathBuf {
    support_dir().join("probe")
}

pub fn latency_path() -> PathBuf {
    support_dir().join("logs/latency.jsonl")
}

pub fn codex_home() -> PathBuf {
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        return PathBuf::from(home);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".codex"),
        None => PathBuf::from(".codex"),
    }
}

pub fn kimi_code_home() -> PathBuf {
    if let Some(home) = std::env::var_os("KIMI_CODE_HOME") {
        return PathBuf::from(home);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".kimi-code"),
        None => PathBuf::from(".kimi-code"),
    }
}

pub fn kimi_code_config_path() -> PathBuf {
    kimi_code_home().join("config.toml")
}

pub fn kimi_code_app_home() -> PathBuf {
    if let Some(home) = std::env::var_os("KIMI_CODE_APP_HOME") {
        return PathBuf::from(home);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join("Library/Application Support/kimi-code-app"),
        None => PathBuf::from("kimi-code-app"),
    }
}

pub fn workbuddy_home() -> PathBuf {
    if let Some(home) = std::env::var_os("WORKBUDDY_HOME") {
        return PathBuf::from(home);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".workbuddy"),
        None => PathBuf::from(".workbuddy"),
    }
}

pub fn codex_state_path() -> Option<PathBuf> {
    let home = codex_home();
    let preferred = home.join("state_5.sqlite");
    if preferred.is_file() {
        return Some(preferred);
    }

    let mut candidates: Vec<(u32, PathBuf)> = std::fs::read_dir(&home)
        .ok()?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            let version = name
                .strip_prefix("state_")?
                .strip_suffix(".sqlite")?
                .parse::<u32>()
                .ok()?;
            Some((version, path))
        })
        .collect();
    candidates.sort_by_key(|(version, _)| *version);
    candidates.pop().map(|(_, path)| path)
}

pub fn hook_bin_dir() -> PathBuf {
    support_dir().join("bin")
}

pub fn hook_binary() -> PathBuf {
    hook_bin_dir().join("irisport-hook")
}

pub fn hook_wrapper() -> PathBuf {
    hook_bin_dir().join("irisport-hook.sh")
}
