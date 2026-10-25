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
#[test]
fn large_ignored_file_is_not_kept_in_shadow_index() {
    let s = Sandbox::git_root();
    s.write(".gitignore", "ignored\n");
    s.write("ignored", "small");
    success(s.undo(&["snap"]));
    s.write("ignored", &"x".repeat(2 * 1024 * 1024));
    success(s.undo(&["snap"]));
    assert!(!files(&s).lines().any(|p| p == "ignored"));
}
#[test]
fn project_attributes_do_not_normalize_snapshot_bytes() {
    let s = Sandbox::git_root();
    s.write(".gitattributes", "*.txt text eol=lf\n");
    fs::write(s.root.join("crlf.txt"), b"a\r\nb\r\n").unwrap();
    success(s.undo(&["snap"]));
    success(s.sh("rm crlf.txt"));
    success(s.undo(&[]));
    assert_eq!(fs::read(s.root.join("crlf.txt")).unwrap(), b"a\r\nb\r\n");
}
#[test]
fn shadow_data_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::git_root();
    s.write(".gitignore", ".env\n");
    s.write(".env", "secret");
    fs::set_permissions(s.root.join(".env"), fs::Permissions::from_mode(0o600)).unwrap();
    success(s.undo(&["snap"]));
    for p in [s.data(), s.data().join("repos"), s.repo()] {
        assert_eq!(fs::metadata(p).unwrap().permissions().mode() & 0o077, 0);
    }
}
#[test]
fn existing_shadow_files_becoming_nested_repo_are_removed() {
    let s = Sandbox::git_root();
    s.write("vendor/f", "old");
    success(s.undo(&["snap"]));
    success(
        s.sh("git -C vendor init -q && git -C vendor add . && git -C vendor commit -qm nested"),
    );
    success(s.undo(&["snap"]));
    assert!(!files(&s).lines().any(|p| p == "vendor/f"));
    assert!(
        fs::read_to_string(s.repo().join("undo-nested"))
            .unwrap()
            .contains("vendor")
    );
}
#[test]
fn snapshot_failure_from_subdirectory_is_visible_in_root_status() {
    let s = Sandbox::git_root();
    let path =
        s.git_shim("case \" $* \" in *\" add \"*) printf 'capture-failed\\n' >&2; exit 1;; esac");
    let output = s
        .command(env!("CARGO_BIN_EXE_undo"))
        .arg("snap")
        .current_dir(s.root.join("src"))
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    success(output);
    let status = success(s.undo(&["status"]));
    assert!(status.contains("capture-failed"), "{status}");
}
