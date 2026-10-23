use crate::{
    git::Git,
    lock, paths,
    root::Root,
    snapshot::{self, Commit},
    view,
};
use anyhow::Result;
use std::{collections::HashMap, fs, time::Duration};
pub fn keep(commits: &[(String, u64)], now: u64) -> Vec<bool> {
    let mut retained = vec![false; commits.len()];
    let mut hours: HashMap<u64, usize> = HashMap::new();
    for (i, (_, ct)) in commits.iter().enumerate() {
        match now.saturating_sub(*ct) {
            0..604800 => retained[i] = true,
            604800..2592000 => {
                let entry = hours.entry(ct / 3600).or_insert(i);
                if *ct >= commits[*entry].1 {
                    *entry = i;
                }
            }
            _ => {}
        }
    }
    for i in hours.into_values() {
        retained[i] = true;
    }
    if let Some(last) = retained.last_mut() {
        *last = true;
    }
    retained
}
fn rewrite(g: &Git, commits: &[Commit]) -> Result<()> {
    let mut parent: Option<String> = None;
    for c in commits {
        let date = format!("@{} +0000", c.ct);
        let dated = g
            .clone()
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date);
        let mut args = vec!["commit-tree", &c.tree];
        if let Some(p) = &parent {
            args.extend(["-p", p]);
        }
        args.extend(["-F", "-"]);
        parent = Some(dated.run_stdin(&args, c.message.as_bytes())?);
    }
    if let Some(parent) = parent {
        g.run(&["update-ref", snapshot::REF, &parent])?;
    }
    Ok(())
}
pub fn prune(root: &Root) -> Result<()> {
    let repo = paths::repo_for(&root.path);
    fs::create_dir_all(&repo)?;
    let _guard = lock::acquire(&repo, Duration::from_secs(10))?;
    fs::write(repo.join("last-prune"), paths::now().to_string())?;
    if !repo.join("HEAD").exists() {
        return Ok(());
    }
    let mut commits = snapshot::history(root, &repo)?;
    commits.reverse();
    let flags = keep(
        &commits
            .iter()
            .map(|c| (c.id.clone(), c.ct))
            .collect::<Vec<_>>(),
        paths::now(),
    );
    let mut retained: Vec<_> = commits
        .iter()
        .zip(&flags)
        .filter(|(_, keep)| **keep)
        .map(|(c, _)| c.clone())
        .collect();
    let g = Git::shadow(root, &repo);
    if retained.len() != commits.len() {
        rewrite(&g, &retained)?;
    }
    loop {
        g.run(&["reflog", "expire", "--expire=now", "--all"])?;
        g.run(&["gc", "--prune=now", "--quiet"])?;
        if view::disk(root, &repo)? <= view::max_bytes() || retained.len() <= 1 {
            break;
        }
        let drop = retained.len().div_ceil(10).min(retained.len() - 1);
        retained.drain(..drop);
        rewrite(&g, &retained)?;
    }
    Ok(())
}
pub fn spawn_if_due(root: &Root, repo: &std::path::Path) {
    let last = fs::read_to_string(repo.join("last-prune"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    if paths::now().saturating_sub(last) < 86400 {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        use std::os::unix::process::CommandExt;
        let _ = std::process::Command::new(exe)
            .args(["prune", "--root"])
            .arg(&root.path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn();
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn retention_boundaries_and_newest() {
        let now = 40 * 86400;
        let dates = [
            now - 31 * 86400,
            now - 30 * 86400,
            now - 8 * 86400 + 100,
            now - 8 * 86400 + 200,
            now - 7 * 86400,
            now - 7 * 86400 + 1,
            now,
        ];
        let c: Vec<_> = dates.iter().map(|t| (String::new(), *t)).collect();
        assert_eq!(
            super::keep(&c, now),
            [false, false, false, true, true, true, true]
        );
        assert_eq!(super::keep(&[("old".into(), 1)], now), [true]);
        assert!(super::keep(&[], now).is_empty());
    }
}
