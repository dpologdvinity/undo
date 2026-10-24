mod common;
use common::{Sandbox, success};
use std::{fs, io::Write, process::Stdio};
#[test]
fn claude_merge_backup_idempotent() {
    let s = Sandbox::new();
    fs::create_dir(s.home.join(".claude")).unwrap();
    let path = s.home.join(".claude/settings.json");
    let original = r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"command":"other"}]}]}}"#;
    fs::write(&path, original).unwrap();
    assert!(success(s.undo(&["init", "claude"])).contains("installed Claude Code hook"));
    let first = fs::read(&path).unwrap();
    assert!(success(s.undo(&["init", "claude"])).contains("already installed"));
    assert_eq!(first, fs::read(&path).unwrap());
    let backups: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("undo-backup-")
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read_to_string(&backups[0]).unwrap(), original);
}
#[test]
fn invalid_settings_untouched() {
    let s = Sandbox::new();
    fs::create_dir(s.home.join(".claude")).unwrap();
    let p = s.home.join(".claude/settings.json");
    fs::write(&p, "invalid").unwrap();
    assert_eq!(s.undo(&["init", "claude"]).status.code(), Some(1));
    assert_eq!(fs::read_to_string(p).unwrap(), "invalid");
}
#[test]
fn installer_refuses_existing_temporary_symlink() {
    let s = Sandbox::new();
    let settings = s.home.join(".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, "{}").unwrap();
    let victim = s.home.join("unrelated.json");
    fs::write(&victim, "keep this file").unwrap();
    let path = s.git_shim(
        "ln -s \"$UNDO_TEST_VICTIM\" \"$HOME/.claude/settings.json.tmp-$PPID\" || exit 1",
    );
    let output = s
        .command(env!("CARGO_BIN_EXE_undo"))
        .args(["init", "claude"])
        .env("PATH", path)
        .env("UNDO_TEST_VICTIM", victim.as_os_str())
        .output()
        .unwrap();

    assert_eq!(fs::read_to_string(victim).unwrap(), "keep this file");
    assert_eq!(fs::read_to_string(settings).unwrap(), "{}");
    assert!(!output.status.success());
}
#[test]
fn shell_syntax() {
    let s = Sandbox::new();
    for shell in ["bash", "zsh", "fish"] {
        if s.command(shell).arg("--version").output().is_err() {
            eprintln!("skip {shell}: unavailable");
            continue;
        }
        let script = success(s.undo(&["init", shell]));
        let mut child = s
            .command(shell)
            .arg("-n")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        success(child.wait_with_output().unwrap());
    }
}
#[test]
fn bash_interactive_snap() {
    let s = Sandbox::git_root();
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_undo"))
        .parent()
        .unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    success(
        s.command("bash")
            .env("PATH", path)
            .args([
                "--norc",
                "-i",
                "-c",
                r#"eval "$(undo init bash)"; __undo_precmd; sleep 1"#,
            ])
            .output()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut found = false;
    while std::time::Instant::now() < deadline {
        if success(s.undo(&["list"])).contains("shell") {
            found = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(found, "list did not contain 'shell' within timeout");
}
#[test]
fn bash_array_and_string_hooks_preserved_once() {
    let s = Sandbox::new();
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_undo"))
        .parent()
        .unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    for setup in [
        "PROMPT_COMMAND=existing",
        "PROMPT_COMMAND=(existing second)",
    ] {
        let script = format!(
            r#"{setup}; eval "$(undo init bash)"; eval "$(undo init bash)"; declare -p PROMPT_COMMAND"#
        );
        let text = success(
            s.command("bash")
                .env("PATH", &path)
                .args(["--norc", "-c", &script])
                .output()
                .unwrap(),
        );
        assert_eq!(text.matches("__undo_precmd").count(), 1);
        assert!(text.contains("existing"));
    }
}
