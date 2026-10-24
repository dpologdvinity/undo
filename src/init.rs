use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::os::unix::fs::OpenOptionsExt;
use std::{fs, io::Write, path::PathBuf};
pub const CLAUDE_COMMAND: &str = "undo snap --trigger claude";
pub fn merge_claude_settings(existing: Option<&str>) -> Result<Option<String>> {
    let mut v: Value =
        serde_json::from_str(existing.unwrap_or("{}")).context("invalid Claude settings JSON")?;
    let obj = v
        .as_object_mut()
        .context("Claude settings must be an object")?;
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("hooks must be an object")?;
    let entries = hooks
        .entry("PreToolUse")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("hooks.PreToolUse must be an array")?;
    if entries.iter().any(|entry| {
        entry["hooks"]
            .as_array()
            .is_some_and(|hooks| hooks.iter().any(|h| h["command"] == CLAUDE_COMMAND))
    }) {
        return Ok(None);
    }
    entries.push(json!({"matcher":"Bash|Write|Edit|NotebookEdit","hooks":[{"type":"command","command":CLAUDE_COMMAND}]}));
    Ok(Some(format!("{}\n", serde_json::to_string_pretty(&v)?)))
}
pub fn init(target: &str) -> Result<()> {
    match target {
        "bash" => print!("{}", include_str!("hooks/undo.bash")),
        "zsh" => print!("{}", include_str!("hooks/undo.zsh")),
        "fish" => print!("{}", include_str!("hooks/undo.fish")),
        "claude" => install_claude()?,
        _ => bail!("unknown init target {target}"),
    }
    Ok(())
}
fn install_claude() -> Result<()> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let path = PathBuf::from(home).join(".claude/settings.json");
    let existing = match fs::read_to_string(&path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let Some(merged) = merge_claude_settings(existing.as_deref())? else {
        println!("already installed");
        return Ok(());
    };
    fs::create_dir_all(path.parent().unwrap())?;
    let backup = path.with_file_name(format!("settings.json.undo-backup-{}", crate::paths::now()));
    let permissions = fs::metadata(&path).ok().map(|m| m.permissions());
    if let Some(original) = &existing {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&backup)?;
        file.write_all(original.as_bytes())?;
        file.sync_all()?;
        if let Some(p) = &permissions {
            file.set_permissions(p.clone())?;
        }
    }
    let tmp = path.with_file_name(format!("settings.json.tmp-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    let result = (|| -> Result<()> {
        file.write_all(merged.as_bytes())?;
        if let Some(p) = permissions {
            file.set_permissions(p)?;
        }
        file.sync_all()?;
        fs::rename(&tmp, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    println!(
        "installed Claude Code hook in {} (backup: {})",
        path.display(),
        if existing.is_some() {
            backup.display().to_string()
        } else {
            "none (new settings file)".into()
        }
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn merge_preserves_settings_and_is_idempotent() {
        for input in [
            None,
            Some("{}"),
            Some(
                r#"{"theme":"dark","hooks":{"Stop":[{"x":1}],"PreToolUse":[{"matcher":"Read","hooks":[{"type":"command","command":"existing"}]}]}}"#,
            ),
        ] {
            let result = merge_claude_settings(input).unwrap().unwrap();
            let v: Value = serde_json::from_str(&result).unwrap();
            let entries = v["hooks"]["PreToolUse"].as_array().unwrap();
            assert_eq!(
                entries.last().unwrap()["hooks"][0]["command"],
                "undo snap --trigger claude"
            );
            if input.is_some_and(|s| s.contains("theme")) {
                assert_eq!(v["theme"], "dark");
                assert_eq!(v["hooks"]["Stop"], json!([{"x":1}]));
                assert_eq!(entries[0]["hooks"][0]["command"], "existing");
            }
            assert!(merge_claude_settings(Some(&result)).unwrap().is_none());
        }
        assert!(merge_claude_settings(Some("bad json")).is_err());
        for invalid in ["[]", r#"{"hooks":null}"#, r#"{"hooks":{"PreToolUse":{}}}"#] {
            assert!(merge_claude_settings(Some(invalid)).is_err());
        }
    }
}
