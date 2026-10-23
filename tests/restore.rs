mod common;
use common::{Sandbox, hash_tree, success};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
fn recover(disaster: &str) {
    let s = Sandbox::git_root();
    s.write("src/a", "x\nx\n");
    s.write("a", "first");
    s.write("b", "second");
    s.write("file", "contents");
    success(s.undo(&["snap"]));
    let before = s.tree_hash();
    success(s.sh(disaster));
    assert_ne!(before, s.tree_hash());
    success(s.undo(&[]));
    assert_eq!(before, s.tree_hash());
}
#[test]
fn rm_src() {
    recover("rm -rf src");
}
#[test]
fn mv_overwrite() {
    recover("mv a b");
}
#[test]
fn sed_edit() {
    recover("sed -i.bak 's/x/y/' src/a && rm src/a.bak");
}
#[test]
fn truncation() {
    recover(": > file");
}
#[test]
fn reset_staged_unstaged() {
    let s = Sandbox::git_root();
    s.write("src/main.rs", "staged\n");
    success(s.sh("git add src/main.rs"));
    s.write("README.md", "unstaged\n");
    let before = s.tree_hash();
    success(s.undo(&["snap"]));
    success(s.sh("git reset --hard"));
    let index = fs::read(s.root.join(".git/index")).unwrap();
    success(s.undo(&[]));
    assert_eq!(before, s.tree_hash());
    assert_eq!(index, fs::read(s.root.join(".git/index")).unwrap());
}
#[test]
fn clean_ignored_env() {
    let s = Sandbox::git_root();
    s.write(".gitignore", ".env\n");
    success(s.sh("git add .gitignore && git commit -qm ignore"));
    s.write(".env", "secret");
    let before = s.tree_hash();
    success(s.undo(&["snap"]));
    success(s.sh("git clean -fdx"));
    success(s.undo(&[]));
    assert_eq!(before, s.tree_hash());
}
#[test]
fn toggle_twice() {
    let s = Sandbox::git_root();
    success(s.undo(&["snap"]));
    success(s.sh("rm -rf src"));
    let after = s.tree_hash();
    success(s.undo(&[]));
    success(s.undo(&[]));
    assert_eq!(after, s.tree_hash());
}
#[test]
fn limited_restore_from_subdirectory() {
    let s = Sandbox::git_root();
    s.write("src/other", "original");
    success(s.undo(&["snap"]));
    s.write("src/main.rs", "changed");
    s.write("src/other", "changed");
    let out = s
        .command(env!("CARGO_BIN_EXE_undo"))
        .args(["restore", "@0", "--", "main.rs"])
        .current_dir(s.root.join("src"))
        .output()
        .unwrap();
    success(out);
    assert_eq!(
        fs::read_to_string(s.root.join("src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    assert_eq!(
        fs::read_to_string(s.root.join("src/other")).unwrap(),
        "changed"
    );
}
#[test]
fn git_and_heavy_untouched() {
    let s = Sandbox::git_root();
    s.write(".gitignore", "node_modules/\nlarge\n");
    s.write("node_modules/pkg", "heavy");
    s.write("large", &"x".repeat(2 * 1024 * 1024));
    success(s.undo(&["snap"]));
    let git = hash_tree(&s.root.join(".git"), false);
    let heavy = hash_tree(&s.root.join("node_modules"), false);
    success(s.sh("rm -rf src"));
    success(s.undo(&[]));
    assert_eq!(git, hash_tree(&s.root.join(".git"), false));
    assert_eq!(heavy, hash_tree(&s.root.join("node_modules"), false));
    assert_eq!(
        fs::metadata(s.root.join("large")).unwrap().len(),
        2 * 1024 * 1024
    );
}
#[test]
fn tracked_build_restored() {
    let s = Sandbox::git_root();
    s.write("build/f", "tracked");
    success(s.sh("git add build && git commit -qm build"));
    success(s.undo(&["snap"]));
    success(s.sh("rm -rf build"));
    success(s.undo(&[]));
    assert_eq!(
        fs::read_to_string(s.root.join("build/f")).unwrap(),
        "tracked"
    );
}
#[test]
fn modes_symlinks_and_unusual_paths() {
    let s = Sandbox::git_root();
    s.write("space\nfile", "yes");
    s.write(":(glob)*", "literal");
    symlink("README.md", s.root.join("link")).unwrap();
    fs::set_permissions(
        s.root.join("src/main.rs"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let before = s.tree_hash();
    success(s.undo(&["snap"]));
    success(s.sh("rm -rf src link"));
    s.write("space\nfile", "no");
    s.write(":(glob)*", "changed");
    success(s.undo(&[]));
    assert_eq!(before, s.tree_hash());
}
#[test]
fn rejects_outside_paths() {
    let s = Sandbox::git_root();
    success(s.undo(&["snap"]));
    assert_eq!(
        s.undo(&["restore", "@0", "--", "../outside"]).status.code(),
        Some(1)
    );
}
#[test]
fn file_directory_transitions() {
    let s = Sandbox::git_root();
    s.write("swap", "file");
    success(s.undo(&["snap"]));
    fs::remove_file(s.root.join("swap")).unwrap();
    s.write("swap/child", "child");
    let after = s.tree_hash();
    success(s.undo(&[]));
    assert_eq!(fs::read_to_string(s.root.join("swap")).unwrap(), "file");
    success(s.undo(&[]));
    assert_eq!(after, s.tree_hash());
}
