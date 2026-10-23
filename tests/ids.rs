mod common;
use common::{Sandbox, success};
use undo_cli::{ids, root::Root};
#[test]
fn prefix_resolves_only_snapshot_commits() {
    let s = Sandbox::git_root();
    success(s.undo(&["snap"]));
    let repo = s.repo();
    let r = Root {
        path: s.root.clone(),
        is_git: true,
        git_common_dir: Some(s.root.join(".git")),
    };
    let sha = success(
        s.command("git")
            .arg("--git-dir")
            .arg(&repo)
            .args(["rev-parse", "refs/heads/snapshots"])
            .output()
            .unwrap(),
    );
    let sha = sha.trim();
    assert_eq!(ids::resolve(&r, &repo, &sha[..8]).unwrap(), sha);
    assert_eq!(ids::resolve(&r, &repo, "@0").unwrap(), sha);
    assert!(ids::resolve(&r, &repo, "@1").is_err());
    assert!(ids::resolve(&r, &repo, &sha[..3]).is_err());
    let unrelated = success(
        s.command("git")
            .arg("--git-dir")
            .arg(&repo)
            .args(["hash-object", "-w", "--stdin"])
            .output()
            .unwrap(),
    );
    assert!(ids::resolve(&r, &repo, unrelated.trim()).is_err());
}
