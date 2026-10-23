use crate::{
    git::Git,
    ids, lock, paths,
    root::{self, Root},
    snapshot::{self, Meta},
};
use anyhow::{Context, Result, bail};
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
    time::Duration,
};

pub fn restore(cwd: &Path, id: &str, paths: &[PathBuf]) -> Result<()> {
    perform(cwd, Some(id), paths)
}
pub fn undo(cwd: &Path) -> Result<()> {
    perform(cwd, None, &[])
}
fn perform(cwd: &Path, id: Option<&str>, selected: &[PathBuf]) -> Result<()> {
    let root = root::require(cwd)?;
    let repo = paths::repo_for(&root.path);
    paths::private_repo(&repo)?;
    let _guard = lock::acquire(&repo, Duration::from_secs(10))?;
    snapshot::ensure_repo(&root, &repo)?;
    let explicit = id.map(|id| ids::resolve(&root, &repo, id)).transpose()?;
    let selected = relative_paths(&root.path, cwd, selected)?;
    let meta = Meta {
        trigger: "pre-restore".into(),
        cmd: Some(id.map_or_else(|| "undo".into(), |id| format!("undo restore {id}"))),
        cwd: cwd.into(),
    };
    let (cur, _) = snapshot::snap_locked(&root, &repo, &meta)?;
    let commits = snapshot::history(&root, &repo)?;
    let target = match explicit {
        Some(id) => commits.iter().find(|c| c.id == id),
        None => commits.iter().find(|c| c.tree != cur),
    };
    let Some(target) = target else {
        println!("nothing to undo");
        return Ok(());
    };
    let n = commits.iter().position(|c| c.id == target.id).unwrap();
    let label = id.map(String::from).unwrap_or_else(|| format!("@{n}"));
    let changes = changes(&root, &repo, &cur, &target.tree, &selected)?;
    if changes.is_empty() {
        println!("already matches {label}");
        return Ok(());
    }
    let mut done = Vec::new();
    let tmp = repo.join(format!("restore-index-{}", std::process::id()));
    let stage = root
        .path
        .join(format!(".undo-restore-{}", std::process::id()));
    let mut stage_created = false;
    let result = (|| -> Result<()> {
        for (status, p) in &changes {
            if *status != 'D' {
                continue;
            }
            safe_parents(&root.path, p)?;
            let full = root.path.join(p);
            fs::remove_file(&full).with_context(|| format!("removing {}", p.display()))?;
            done.push((*status, p.to_string_lossy().into_owned()));
            let mut parent = full.parent();
            while let Some(dir) = parent {
                if dir == root.path {
                    break;
                }
                match fs::remove_dir(dir) {
                    Ok(()) => parent = dir.parent(),
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
                        ) =>
                    {
                        break;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        let g = Git::shadow(&root, &repo).index(&tmp);
        g.run(&["read-tree", &target.tree])?;
        fs::create_dir(&stage)?;
        stage_created = true;
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))?;
        let mut prefix = OsString::from("--prefix=");
        prefix.push(stage.as_os_str());
        prefix.push("/");
        for (status, p) in &changes {
            if *status == 'D' {
                continue;
            }
            safe_parents(&root.path, p)?;
            let mut input = p.as_os_str().as_encoded_bytes().to_vec();
            input.push(0);
            g.run_os_stdin(
                &[
                    "checkout-index".into(),
                    "-f".into(),
                    "-z".into(),
                    "--stdin".into(),
                    prefix.clone(),
                ],
                &input,
            )
            .with_context(|| format!("writing {}", p.display()))?;
            let staged = stage.join(p);
            if fs::symlink_metadata(&staged)?.is_file() {
                let executable = fs::metadata(&staged)?.permissions().mode() & 0o111;
                fs::set_permissions(&staged, fs::Permissions::from_mode(0o600 | executable))?;
            }
            let full = root.path.join(p);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent)?;
            }
            safe_parents(&root.path, p)?;
            fs::rename(&staged, &full).with_context(|| format!("replacing {}", p.display()))?;
            done.push((*status, p.to_string_lossy().into_owned()));
        }
        Ok(())
    })();
    let cleanup = if tmp.exists() {
        fs::remove_file(&tmp).map_err(anyhow::Error::from)
    } else {
        Ok(())
    };
    let stage_cleanup = if stage_created {
        fs::remove_dir_all(&stage).map_err(anyhow::Error::from)
    } else {
        Ok(())
    };
    if let Err(e) = result.and(cleanup).and(stage_cleanup) {
        bail!(
            "restore stopped: {e:#}\n{}\nundo restore @0 returns to the pre-restore state",
            summarize(&done)
        );
    }
    let before = n
        .checked_sub(1)
        .map(|i| commits[i].field("undo-cmd: "))
        .filter(|s| !s.is_empty());
    let context = before.map(|s| format!(", before: {s}")).unwrap_or_default();
    println!(
        "restored {label} {} ({}{})\n{}\nrun `undo` again to go back",
        &target.id[..8],
        crate::view::age(paths::now().saturating_sub(target.ct)),
        context,
        summarize(&done)
    );
    Ok(())
}
fn relative_paths(root: &Path, cwd: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let cwd = fs::canonicalize(cwd)?;
    paths
        .iter()
        .map(|p| {
            let p = cwd.join(p);
            let mut clean = PathBuf::new();
            for c in p.components() {
                match c {
                    Component::ParentDir => {
                        clean.pop();
                    }
                    Component::CurDir => {}
                    _ => clean.push(c.as_os_str()),
                }
            }
            let rel = clean
                .strip_prefix(root)
                .context("restore path is outside protected root")?
                .to_path_buf();
            if rel.components().any(|c| c.as_os_str() == ".git") {
                bail!("cannot restore .git");
            }
            safe_parents(root, &rel)?;
            Ok(if rel.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                rel
            })
        })
        .collect()
}
fn safe_parents(root: &Path, rel: &Path) -> Result<()> {
    let mut full = root.to_path_buf();
    let components: Vec<_> = rel.components().collect();
    for (i, c) in components.iter().enumerate() {
        if !matches!(c, Component::Normal(_) | Component::CurDir) || c.as_os_str() == ".git" {
            bail!("unsafe restore path: {}", rel.display());
        }
        full.push(c.as_os_str());
        if i + 1 < components.len()
            && fs::symlink_metadata(&full).is_ok_and(|m| m.file_type().is_symlink())
        {
            bail!("restore path crosses symlink: {}", full.display());
        }
    }
    Ok(())
}
pub fn changes(
    root: &Root,
    repo: &Path,
    cur: &str,
    target: &str,
    selected: &[PathBuf],
) -> Result<Vec<(char, PathBuf)>> {
    let g = Git::shadow(root, repo);
    let data = g.bytes_paths(
        &[
            "--literal-pathspecs",
            "diff-tree",
            "-r",
            "-z",
            "--no-renames",
            "--name-status",
            cur,
            target,
            "--",
        ],
        selected,
    )?;
    let mut result = Vec::new();
    let fields: Vec<_> = snapshot::nul_paths(&data).collect();
    for f in fields.as_chunks::<2>().0 {
        let status = char::from(f[0][0]);
        if !matches!(status, 'A' | 'M' | 'T' | 'D') {
            bail!("unsupported diff status {status}");
        }
        let path = snapshot::path(f[1]);
        if !snapshot::excluded_current(root, repo, path)? {
            result.push((status, path.to_path_buf()));
        }
    }
    Ok(result)
}
pub fn symbol(status: char) -> char {
    match status {
        'A' => '+',
        'D' => '-',
        _ => '~',
    }
}
fn summarize(changes: &[(char, String)]) -> String {
    let added = changes.iter().filter(|(s, _)| *s == 'A').count();
    let removed = changes.iter().filter(|(s, _)| *s == 'D').count();
    let mut out = format!(
        "  recreated {added}, reverted {}, removed {removed}",
        changes.len() - added - removed
    );
    for (s, p) in changes.iter().take(10) {
        out.push_str(&format!("\n  {} {}", symbol(*s), p.escape_debug()));
    }
    if changes.len() > 10 {
        out.push_str(&format!("\n  … {} more", changes.len() - 10));
    }
    out
}
#[cfg(test)]
mod tests {
    #[test]
    fn summary_counts_and_caps_paths() {
        let mut c = vec![
            ('A', "new".into()),
            ('M', "edit".into()),
            ('T', "type".into()),
            ('D', "gone".into()),
        ];
        c.extend((0..10).map(|i| ('A', format!("extra{i}"))));
        let s = super::summarize(&c);
        assert!(s.contains("recreated 11, reverted 2, removed 1"));
        assert!(s.contains("… 4 more"));
        assert!(!s.contains("extra9"));
        assert!(s.contains("+ new"));
        assert!(s.contains("~ type"));
    }
}
