mod common;
use common::{Sandbox, success};
use std::fs;
#[test]
fn retention_rewrites_exact_dates() {
    let s = Sandbox::git_root();
    success(s.undo(&["snap"]));
    let repo = s.repo();
    let now = undo_cli::paths::now();
    let h = ((now - 8 * 86400) / 3600) * 3600;
    let tree = success(
        s.command("git")
            .arg("--git-dir")
            .arg(&repo)
            .args(["rev-parse", "refs/heads/snapshots^{tree}"])
            .output()
            .unwrap(),
    );
    let mut parent: Option<String> = None;
    for ct in [now - 31 * 86400, h - 7200, h + 600, h + 1200, now - 3600] {
        let mut c = s.command("git");
        c.arg("--git-dir")
            .arg(&repo)
            .args(["commit-tree", tree.trim(), "-m", &format!("date {ct}")])
            .env("GIT_COMMITTER_DATE", format!("@{ct} +0000"))
            .env("GIT_AUTHOR_DATE", format!("@{ct} +0000"));
        if let Some(p) = &parent {
            c.args(["-p", p]);
        }
        parent = Some(success(c.output().unwrap()).trim().to_string());
    }
    success(
        s.command("git")
            .arg("--git-dir")
            .arg(&repo)
            .args([
                "update-ref",
                "refs/heads/snapshots",
                parent.as_ref().unwrap(),
            ])
            .output()
            .unwrap(),
    );
    success(s.undo(&["prune"]));
    let dates = success(
        s.command("git")
            .arg("--git-dir")
            .arg(&repo)
            .args(["log", "--reverse", "--format=%ct", "refs/heads/snapshots"])
            .output()
            .unwrap(),
    );
    let dates: Vec<u64> = dates.lines().map(|l| l.parse().unwrap()).collect();
    assert_eq!(dates, [h - 7200, h + 1200, now - 3600]);
    assert!(
        fs::read_to_string(repo.join("last-prune"))
            .unwrap()
            .trim()
            .parse::<u64>()
            .is_ok()
    );
}
#[test]
fn size_cap_keeps_newest() {
    let s = Sandbox::git_root();
    success(s.undo(&["snap"]));
    s.write("new", "new");
    success(s.undo(&["snap"]));
    success(
        s.command(env!("CARGO_BIN_EXE_undo"))
            .args(["prune"])
            .env("UNDO_MAX_BYTES", "1")
            .output()
            .unwrap(),
    );
    let t = success(s.undo(&["list"]));
    assert_eq!(t.lines().count(), 1);
    assert!(t.starts_with("@0"));
}
