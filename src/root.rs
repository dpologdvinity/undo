use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Root {
    pub path: PathBuf,
    pub is_git: bool,
    pub git_common_dir: Option<PathBuf>,
}
pub fn find_root(cwd: &Path) -> Option<Root> {
    let cwd = fs::canonicalize(cwd).ok()?;
    let git = crate::git::Git::plain()
        .at(&cwd)
        .bytes(&["rev-parse", "--show-toplevel"])
        .ok()
        .and_then(|o| path_output(&o))
        .and_then(|p| fs::canonicalize(p).ok());
    let (path, is_git) = pick_innermost(git, &read_roots().ok()?, &cwd)?;
    let git_common_dir = if is_git {
        crate::git::Git::plain()
            .at(&cwd)
            .bytes(&["rev-parse", "--git-common-dir"])
            .ok()
            .and_then(|o| path_output(&o))
            .and_then(|p| fs::canonicalize(cwd.join(p)).ok())
    } else {
        None
    };
    Some(Root {
        path,
        is_git,
        git_common_dir,
    })
}
fn path_output(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    if bytes.is_empty() {
        None
    } else {
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }
}
fn pick_innermost(
    git: Option<PathBuf>,
    protected: &[PathBuf],
    cwd: &Path,
) -> Option<(PathBuf, bool)> {
    protected
        .iter()
        .filter(|p| cwd.starts_with(p))
        .map(|p| (p.clone(), false))
        .chain(git.map(|p| (p, true)))
        .max_by_key(|(p, is_git)| (p.components().count(), *is_git))
}
pub fn require(cwd: &Path) -> Result<Root> {
    find_root(cwd)
        .context("not inside a protected directory (use a git repo or 'undo protect <dir>')")
}
pub fn read_roots() -> Result<Vec<PathBuf>> {
    match fs::read_to_string(crate::paths::data_home().join("roots")) {
        Ok(s) => Ok(s.lines().filter_map(|l| fs::canonicalize(l).ok()).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}
pub fn write_roots(roots: &[PathBuf]) -> Result<()> {
    let dir = crate::paths::data_home();
    crate::paths::private_dir(&dir)?;
    let mut roots = roots.to_vec();
    roots.sort();
    roots.dedup();
    let mut text = String::new();
    for root in roots {
        let p = root.to_str().context("protected path must be UTF-8")?;
        if p.contains(['\n', '\r']) {
            bail!("protected path cannot contain newlines");
        }
        text.push_str(p);
        text.push('\n');
    }
    let tmp = dir.join(format!("roots.tmp-{}", std::process::id()));
    fs::write(&tmp, text)?;
    fs::rename(tmp, dir.join("roots"))?;
    Ok(())
}
pub fn protect(dir: &Path, force: bool) -> Result<()> {
    let path = fs::canonicalize(dir)?;
    let home = std::env::var_os("HOME").and_then(|p| fs::canonicalize(p).ok());
    if !force && (path == Path::new("/") || home.as_ref() == Some(&path)) {
        bail!(
            "refusing to protect {}: too broad (use --force)",
            path.display()
        );
    }
    if !path.is_dir() {
        bail!("not a directory: {}", path.display());
    }
    let mut roots = read_roots()?;
    roots.push(path);
    write_roots(&roots)
}
pub fn unprotect(dir: &Path) -> Result<()> {
    let path = fs::canonicalize(dir)?;
    let mut roots = read_roots()?;
    roots.retain(|p| p != &path);
    write_roots(&roots)?;
    let repo = crate::paths::repo_for(&path);
    println!("{}\nremove with: rm -rf {}", repo.display(), repo.display());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn innermost_component_prefix() {
        assert_eq!(
            pick_innermost(
                Some("/a".into()),
                &["/a/b".into()],
                std::path::Path::new("/a/b/c")
            ),
            Some(("/a/b".into(), false))
        );
        assert_eq!(
            pick_innermost(None, &["/a/b".into()], std::path::Path::new("/a/bc")),
            None
        );
        assert_eq!(
            pick_innermost(
                Some("/a".into()),
                &["/a".into()],
                std::path::Path::new("/a")
            ),
            Some(("/a".into(), true))
        );
    }
}
