use crate::{
    git::Git,
    ids, lock, paths, restore,
    root::{self, Root},
    snapshot,
};
use anyhow::{Result, bail};
use std::{fs, path::Path, time::Duration};
pub const LIMITS: &str = "cannot recover: commits/branches/HEAD (use git reflog), the staging area,\n  anything outside protected roots, changes made and destroyed inside one\n  command, heavy dirs (node_modules target .venv venv dist build .next .cache\n  __pycache__ when ignored), ignored files > 1 MiB, nested git repos";
pub fn age(secs: u64) -> String {
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}
pub fn list(cwd: &Path, n: usize) -> Result<()> {
    let root = root::require(cwd)?;
    let repo = paths::repo_for(&root.path);
    let g = Git::shadow(&root, &repo);
    let commits = snapshot::history(&root, &repo)?;
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100);
    for (i, c) in commits.iter().enumerate().take(n) {
        let files = if i + 1 == commits.len() {
            g.bytes(&["ls-tree", "-r", "--name-only", "-z", &c.id])?
        } else {
            g.bytes(&[
                "diff-tree",
                "--no-commit-id",
                "-r",
                "--name-only",
                "-z",
                &c.id,
            ])?
        };
        let count = snapshot::nul_paths(&files).count();
        let prefix = format!(
            "@{i}  {}  {:<10} {:<12} {count:>3} {}  ",
            &c.id[..8],
            age(paths::now().saturating_sub(c.ct)),
            c.field("undo-trigger: "),
            if count == 1 { "file " } else { "files" }
        );
        let cmd: String = c
            .field("undo-cmd: ")
            .chars()
            .take(width.saturating_sub(prefix.chars().count()))
            .collect();
        println!("{prefix}{cmd}");
    }
    Ok(())
}
pub fn diff(cwd: &Path, id: &str, patch: bool) -> Result<()> {
    let root = root::require(cwd)?;
    let repo = paths::repo_for(&root.path);
    fs::create_dir_all(&repo)?;
    let _guard = lock::acquire(&repo, Duration::from_secs(10))?;
    let target = ids::resolve(&root, &repo, id)?;
    let cur = snapshot::capture_tree(&root, &repo)?;
    let g = Git::shadow(&root, &repo);
    let target = g.run(&["rev-parse", &format!("{target}^{{tree}}")])?;
    if patch {
        let status = g
            .command()
            .args([
                "-c",
                "core.pager=less -FRX",
                "diff",
                "--color=auto",
                &cur,
                &target,
            ])
            .status()?;
        if !status.success() {
            bail!("git diff failed");
        }
    } else {
        for (s, p) in restore::changes(&root, &repo, &cur, &target, &[])? {
            println!(
                "{} {}",
                restore::symbol(s),
                p.to_string_lossy().escape_debug()
            );
        }
    }
    Ok(())
}
pub fn max_bytes() -> u64 {
    std::env::var("UNDO_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2 * 1024 * 1024 * 1024)
}
pub fn disk(root: &Root, repo: &Path) -> Result<u64> {
    if !repo.join("HEAD").exists() {
        return Ok(0);
    }
    let text = Git::shadow(root, repo).run(&["count-objects", "-v"])?;
    let mut kib = 0u64;
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(": ")
            && matches!(key, "size" | "size-pack")
        {
            kib = kib.saturating_add(value.parse::<u64>()?);
        }
    }
    Ok(kib.saturating_mul(1024))
}
fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}
pub fn status(cwd: &Path) -> Result<()> {
    let root = root::find_root(cwd);
    if let Some(r) = &root {
        let repo = paths::repo_for(&r.path);
        let commits = snapshot::history(r, &repo)?;
        println!(
            "root:       {} ({})",
            r.path.display(),
            if r.is_git { "git" } else { "protected" }
        );
        match (commits.first(), commits.last()) {
            (Some(first), Some(last)) => println!(
                "snapshots:  {} (newest {}, oldest {})",
                commits.len(),
                age(paths::now().saturating_sub(first.ct)),
                age(paths::now().saturating_sub(last.ct))
            ),
            _ => println!("snapshots:  0"),
        }
        println!(
            "disk:       {} (cap {})",
            size(disk(r, &repo)?),
            size(max_bytes())
        );
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    let claude = fs::read_to_string(std::path::PathBuf::from(home).join(".claude/settings.json"))
        .is_ok_and(|s| s.contains("undo snap --trigger claude"));
    let shell = std::env::var("UNDO_SHELL_HOOK").is_ok_and(|s| s == "1");
    println!(
        "hooks:      shell {} · claude {}",
        if shell { "yes" } else { "no" },
        if claude { "yes" } else { "no" }
    );
    if let Some(r) = &root {
        for p in fs::read_to_string(paths::repo_for(&r.path).join("undo-nested"))
            .unwrap_or_default()
            .lines()
        {
            println!("skipped:    {p} (nested git repo)");
        }
    }
    let log = fs::read_to_string(paths::data_home().join("log")).unwrap_or_default();
    let mut errors: Vec<_> = log
        .lines()
        .filter(|line| {
            let Some((time, rest)) = line.split_once(' ') else {
                return false;
            };
            let recent = time
                .parse::<u64>()
                .is_ok_and(|t| paths::now().saturating_sub(t) <= 86400);
            recent
                && root
                    .as_ref()
                    .is_none_or(|r| rest.starts_with(&format!("{} ", r.path.display())))
        })
        .collect();
    if errors.is_empty() {
        println!("errors:     none in last 24h");
    } else {
        let start = errors.len().saturating_sub(5);
        for e in errors.drain(start..) {
            println!("errors:     {e}");
        }
    }
    println!("{LIMITS}");
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn age_boundaries() {
        for (s, want) in [
            (59, "just now"),
            (60, "1m ago"),
            (3599, "59m ago"),
            (3600, "1h ago"),
            (86400, "1d ago"),
        ] {
            assert_eq!(super::age(s), want);
        }
    }
}
