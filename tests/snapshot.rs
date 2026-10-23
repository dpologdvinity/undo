mod common;
use common::{Sandbox, success};
use std::fs;
fn files(s: &Sandbox) -> String {
    assert!(
        !s.data().join("log").exists(),
        "{}",
        fs::read_to_string(s.data().join("log")).unwrap_or_default()
    );
    success(
        s.command("git")
            .arg("--git-dir")
            .arg(s.repo())
            .args(["ls-tree", "-r", "--name-only", "refs/heads/snapshots"])
            .output()
            .unwrap(),
    )
}
#[test]
fn no_change_creates_no_commit() {
    let s = Sandbox::git_root();
    success(s.undo(&["snap"]));
    success(s.undo(&["snap"]));
    assert_eq!(
        success(
            s.command("git")
                .arg("--git-dir")
                .arg(s.repo())
                .args(["rev-list", "--count", "refs/heads/snapshots"])
                .output()
                .unwrap()
        )
        .trim(),
        "1"
    );
}
#[test]
fn small_ignored_included_large_excluded() {
    let s = Sandbox::git_root();
    s.write(".gitignore", ".env\nlarge\nnode_modules/\n");
    s.write(".env", "1234567890");
    s.write("large", &"a".repeat(2 * 1024 * 1024));
    s.write("node_modules/pkg", "heavy");
    success(s.undo(&["snap"]));
    let f = files(&s);
    assert!(f.lines().any(|p| p == ".env"));
    assert!(!f.contains("large"));
    assert!(!f.contains("node_modules"));
}
#[test]
fn nested_repo_skipped() {
    let s = Sandbox::git_root();
    s.write("vendor/lib/f", "nested");
    success(s.sh("git -C vendor/lib init -q && git -C vendor/lib add . && git -C vendor/lib commit -qm nested"));
    success(s.undo(&["snap"]));
    assert!(!files(&s).contains("vendor/lib"));
    assert!(
        fs::read_to_string(s.repo().join("undo-nested"))
            .unwrap()
            .contains("vendor/lib")
    );
}
#[test]
fn protected_root_excludes_heavy() {
    let s = Sandbox::new();
    s.write("node_modules/pkg", "heavy");
    s.write("keep", "yes");
    success(s.undo(&["protect", "."]));
    assert!(files(&s).contains("keep"));
    assert!(!files(&s).contains("node_modules"));
}
#[test]
fn tracked_build_kept_even_if_ignored() {
    let s = Sandbox::git_root();
    s.write("build/f", "tracked");
    success(s.sh("git add build/f && git commit -qm build"));
    s.write(".gitignore", "build/\n");
    success(s.undo(&["snap"]));
    assert!(files(&s).contains("build/f"));
}
#[test]
fn outside_silent() {
    let s = Sandbox::new();
    let o = s.undo(&["snap"]);
    assert!(o.status.success());
    assert!(o.stdout.is_empty());
    assert!(o.stderr.is_empty());
}
