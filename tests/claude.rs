mod common;
use common::{Sandbox, success};
use std::fs;
#[test]
fn claude_snap_disaster_undo() {
    let s = Sandbox::git_root();
    let before = s.tree_hash();
    let json=serde_json::json!({"cwd":s.root,"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"rm -rf src"}}).to_string();
    let out = s.undo_stdin(&["snap", "--trigger", "claude"], &json);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
    success(s.sh("rm -rf src"));
    success(s.undo(&[]));
    assert_eq!(before, s.tree_hash());
    assert!(success(s.undo(&["list"])).contains("rm -rf src"));
}
#[test]
fn invalid_json_silent_and_logged_uses_process_cwd() {
    let s = Sandbox::git_root();
    let out = s.undo_stdin(&["snap", "--trigger", "claude"], "not json");
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
    assert!(
        fs::read_to_string(s.data().join("log"))
            .unwrap()
            .contains("invalid Claude JSON")
    );
    assert!(success(s.undo(&["list"])).contains("claude"));
}
#[test]
fn stdin_cwd_overrides_process_cwd() {
    let s = Sandbox::git_root();
    let json=serde_json::json!({"cwd":s.root,"tool_name":"NotebookEdit","tool_input":{"notebook_path":"book.ipynb"}}).to_string();
    use std::io::Write;
    let mut child = s
        .command(env!("CARGO_BIN_EXE_undo"))
        .args(["snap", "--trigger", "claude"])
        .current_dir(&s.home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();
    success(child.wait_with_output().unwrap());
    assert!(success(s.undo(&["list"])).contains("NotebookEdit book.ipynb"));
}
