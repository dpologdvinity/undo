mod common;

use common::{Sandbox, success};
use std::{
    fs,
    time::{Duration, Instant},
};
use undo_cli::{
    git::{self, Git},
    root::Root,
};

#[test]
fn snapshot_can_retry_after_git_timeout() {
    let s = Sandbox::git_root();
    let original_index = fs::read(s.root.join(".git/index")).unwrap();
    let repo = s.data().join("timeout.git");
    success(
        s.command("git")
            .args(["init", "--bare", "-q"])
            .arg(&repo)
            .output()
            .unwrap(),
    );
    let root = Root {
        path: s.root.clone(),
        is_git: true,
        git_common_dir: Some(s.root.join(".git")),
    };
    let block = s.dir.path().join("block");
    let started = s.dir.path().join("started");
    fs::write(&block, "").unwrap();
    let path = s.git_shim(
        "if [ -f \"$UNDO_TEST_BLOCK\" ]; then\n  : > \"$GIT_INDEX_FILE.lock\"\n  : > \"$UNDO_TEST_STARTED\"\n  exec sleep 30\nfi",
    );
    let g = Git::shadow(&root, &repo)
        .env("PATH", path)
        .env("HOME", s.home.as_os_str())
        .env("UNDO_HOME", s.data().as_os_str())
        .env("UNDO_TEST_BLOCK", block.as_os_str())
        .env("UNDO_TEST_STARTED", started.as_os_str());
    let deadline = git::set_deadline(Instant::now() + Duration::from_secs(1));

    let error = g.run(&["add", "-A"]).unwrap_err();
    drop(deadline);
    assert!(format!("{error:#}").contains("timed out"));
    assert!(
        started.exists(),
        "Git shim must create its lock before timing out"
    );
    assert!(!repo.join("undo-index.lock").exists());
    fs::remove_file(block).unwrap();
    g.run(&["add", "-A"]).unwrap();
    assert!(!g.run(&["write-tree"]).unwrap().is_empty());
    assert_eq!(fs::read(s.root.join(".git/index")).unwrap(), original_index);
}
