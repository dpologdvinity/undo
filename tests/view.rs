mod common;
use common::{Sandbox, success};
#[test]
fn newest_list_has_metadata() {
    let s = Sandbox::git_root();
    success(s.undo(&["snap", "--trigger", "shell", "--cmd", "echo first"]));
    s.write("new", "new");
    success(s.undo(&["snap", "--trigger", "claude", "--cmd", "Edit new"]));
    let text = success(s.undo(&["list"]));
    let line = text.lines().next().unwrap();
    assert!(line.starts_with("@0"));
    assert!(line.contains("claude"));
    assert!(line.contains("Edit new"));
}
#[test]
fn diff_restore_direction() {
    let s = Sandbox::git_root();
    success(s.undo(&["snap"]));
    success(s.sh("rm -rf src"));
    assert!(success(s.undo(&["diff", "@0"])).contains("+ src/main.rs"));
    assert!(success(s.undo(&["diff", "@0", "--patch"])).contains("+fn main()"));
}
#[test]
fn status_limits_and_nested() {
    let s = Sandbox::git_root();
    s.write("vendor/lib/f", "nested");
    success(s.sh("git -C vendor/lib init -q && git -C vendor/lib add . && git -C vendor/lib commit -qm nested"));
    success(s.undo(&["snap"]));
    let text = success(s.undo(&["status"]));
    assert!(text.contains("cannot recover:"));
    assert!(text.contains("vendor/lib (nested git repo)"));
    assert!(text.contains("snapshots:"));
}
#[test]
fn status_outside_shows_hooks_and_limits() {
    let s = Sandbox::new();
    let t = success(s.undo(&["status"]));
    assert!(t.contains("hooks:"));
    assert!(t.contains("cannot recover:"));
    assert!(!t.contains("root:"));
}
