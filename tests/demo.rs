mod common;

use common::{Sandbox, hash_tree, success};
use std::fs;

fn commands() -> String {
    include_str!("../demo.tape")
        .lines()
        .filter_map(|line| line.strip_prefix("Type "))
        .map(|line| {
            line.strip_prefix('`')
                .and_then(|line| line.strip_suffix('`'))
                .map(str::to_owned)
                .unwrap_or_else(|| serde_json::from_str::<String>(line).unwrap())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn demo_command(s: &Sandbox) -> std::process::Command {
    let temporary = s.dir.path().join("temporary files");
    fs::create_dir(&temporary).unwrap();
    let path = std::env::join_paths(
        [std::path::Path::new(env!("CARGO_BIN_EXE_undo"))
            .parent()
            .unwrap()
            .to_path_buf()]
        .into_iter()
        .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut command = s.command("bash");
    command
        .args(["--noprofile", "--norc", "-c", &commands()])
        .env("UNDO_DEMO_ROOT", &s.root)
        .env("TMPDIR", temporary)
        .env("PATH", path);
    command
}

#[test]
fn demo_uses_disposable_home_and_workspace() {
    let s = Sandbox::new();
    fs::create_dir_all(s.home.join("src")).unwrap();
    fs::write(s.home.join("src/keep"), "home work").unwrap();
    fs::write(s.home.join("README.md"), "home readme").unwrap();
    s.write("keep", "workspace work");
    let home = hash_tree(&s.home, false);
    let root = s.tree_hash();
    success(demo_command(&s).output().unwrap());
    assert_eq!(
        hash_tree(&s.home, false),
        home,
        "demo changed incoming HOME"
    );
    assert_eq!(s.tree_hash(), root, "demo changed incoming workspace");
    assert_eq!(
        fs::read_dir(s.dir.path().join("temporary files"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn demo_cannot_modify_inherited_git_checkout() {
    let s = Sandbox::git_root();
    s.write("work.txt", "committed");
    success(s.sh("git add work.txt && git commit -qm work"));
    s.write("README.md", "external readme edit");
    s.write("work.txt", "external work edit");
    let before = s.tree_hash();
    let index = fs::read(s.root.join(".git/index")).unwrap();
    let head = success(s.sh("git rev-parse HEAD"));
    let output = demo_command(&s)
        .env("GIT_DIR", s.root.join(".git"))
        .env("GIT_WORK_TREE", &s.root)
        .env("GIT_INDEX_FILE", s.root.join(".git/index"))
        .output()
        .unwrap();
    assert_eq!(s.tree_hash(), before, "demo changed inherited checkout");
    assert_eq!(fs::read(s.root.join(".git/index")).unwrap(), index);
    assert_eq!(success(s.sh("git rev-parse HEAD")), head);
    success(output);
}
