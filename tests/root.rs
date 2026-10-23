mod common;
use common::{Sandbox, success};
#[test]
fn git_root_from_subdirectory() {
    let s = Sandbox::git_root();
    let o = s
        .command(env!("CARGO_BIN_EXE_undo"))
        .arg("status")
        .current_dir(s.root.join("src"))
        .output()
        .unwrap();
    assert!(success(o).contains(s.root.to_str().unwrap()));
}
#[test]
fn protected_non_git() {
    let s = Sandbox::new();
    success(s.undo(&["protect", "."]));
    assert!(success(s.undo(&["status"])).contains("(protected)"));
}
#[test]
fn protected_inside_git_wins() {
    let s = Sandbox::git_root();
    success(s.undo(&["protect", "src"]));
    let o = s
        .command(env!("CARGO_BIN_EXE_undo"))
        .arg("status")
        .current_dir(s.root.join("src"))
        .output()
        .unwrap();
    let text = success(o);
    assert!(text.contains(s.root.join("src").to_str().unwrap()));
    assert!(text.contains("(protected)"));
}
#[test]
fn home_refused() {
    let s = Sandbox::new();
    let o = s.undo(&["protect", s.home.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("too broad"));
}
#[test]
fn outside_has_no_root() {
    let s = Sandbox::new();
    assert!(!success(s.undo(&["status"])).contains("root:"));
}
