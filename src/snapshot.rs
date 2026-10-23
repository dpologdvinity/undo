use crate::{
    git::{self, Git},
    lock, log, paths,
    root::Root,
};
use anyhow::{Result, bail};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub const HEAVY: &[&str] = &[
    "node_modules",
    "target",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    ".cache",
    "__pycache__",
];
pub const REF: &str = "refs/heads/snapshots";
pub struct Meta {
    pub trigger: String,
    pub cmd: Option<String>,
    pub cwd: PathBuf,
}
pub fn format_message(m: &Meta) -> String {
    let cmd = m.cmd.as_deref().unwrap_or("");
    let first: String = cmd
        .lines()
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("(none)")
        .chars()
        .take(200)
        .collect();
    let full: String = cmd.replace('\n', "\\n").chars().take(2000).collect();
    format!(
        "{}: {}\n\nundo-trigger: {}\nundo-cmd: {}\nundo-cwd: {}\n",
        m.trigger,
        first,
        m.trigger,
        full,
        m.cwd.display()
    )
}
pub fn parse_claude_input(json: &str) -> (Option<PathBuf>, Option<String>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return (None, None);
    };
    let cwd = v["cwd"].as_str().map(PathBuf::from);
    let cmd = v["tool_name"].as_str().map(|name| {
        if name == "Bash" {
            v["tool_input"]["command"].as_str().unwrap_or("").to_owned()
        } else {
            format!(
                "{} {}",
                name,
                v["tool_input"]["file_path"]
                    .as_str()
                    .or_else(|| v["tool_input"]["notebook_path"].as_str())
                    .unwrap_or("")
            )
        }
    });
    (cwd, cmd)
}
pub fn ensure_repo(root: &Root, repo: &Path) -> Result<()> {
    if !repo.join("HEAD").exists() {
        Git::shadow(root, repo).init_bare()?;
        fs::write(
            repo.join("undo-root"),
            root.path.as_os_str().as_encoded_bytes(),
        )?;
    }
    Ok(())
}
pub fn path(bytes: &[u8]) -> &Path {
    use std::os::unix::ffi::OsStrExt;
    Path::new(std::ffi::OsStr::from_bytes(bytes))
}
pub fn nul_paths(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes.split(|b| *b == 0).filter(|p| !p.is_empty())
}
fn add_paths(g: &Git, paths: &[u8]) -> Result<()> {
    if !paths.is_empty() {
        g.run_stdin(
            &[
                "--literal-pathspecs",
                "add",
                "-f",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            paths,
        )?;
    }
    Ok(())
}
pub fn capture_tree(root: &Root, repo: &Path) -> Result<String> {
    ensure_repo(root, repo)?;
    let g = Git::shadow(root, repo);
    if !root.is_git {
        fs::write(
            repo.join("info/exclude"),
            HEAVY.iter().map(|n| format!("{n}/\n")).collect::<String>(),
        )?;
    }
    g.run(&["add", "-A"])?;
    if root.is_git {
        let tracked = Git::plain()
            .at(&root.path)
            .bytes(&["ls-files", "-s", "-z"])?;
        let mut force = Vec::new();
        for entry in nul_paths(&tracked) {
            if entry.starts_with(b"160000 ") {
                continue;
            }
            if let Some(i) = entry.iter().position(|b| *b == b'\t') {
                let p = &entry[i + 1..];
                if fs::symlink_metadata(root.path.join(path(p)))
                    .is_ok_and(|m| m.is_file() || m.file_type().is_symlink())
                {
                    force.extend_from_slice(p);
                    force.push(0);
                }
            }
        }
        add_paths(&g, &force)?;
    }
    let entries = g.bytes(&["ls-files", "-s", "-z"])?;
    let mut nested = Vec::new();
    for e in nul_paths(&entries).filter(|e| e.starts_with(b"160000 ")) {
        if let Some(i) = e.iter().position(|b| *b == b'\t') {
            nested.extend_from_slice(&e[i + 1..]);
            nested.push(0);
        }
    }
    if !nested.is_empty() {
        g.run_stdin(
            &[
                "--literal-pathspecs",
                "rm",
                "--cached",
                "-f",
                "-q",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            &nested,
        )?;
    }
    let nested_lines: Vec<u8> = nested
        .iter()
        .map(|b| if *b == 0 { b'\n' } else { *b })
        .collect();
    fs::write(repo.join("undo-nested"), nested_lines)?;
    if root.is_git {
        let mut args = vec![
            "ls-files".to_owned(),
            "-o".into(),
            "-i".into(),
            "--exclude-standard".into(),
            "-z".into(),
            "--".into(),
            ".".into(),
        ];
        args.extend(HEAVY.iter().map(|n| format!(":(exclude,glob)**/{n}/**")));
        let ignored = g.bytes(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
        let mut small = Vec::new();
        for p in nul_paths(&ignored) {
            if fs::symlink_metadata(root.path.join(path(p)))
                .is_ok_and(|m| m.file_type().is_symlink() || (m.is_file() && m.len() <= 1_048_576))
            {
                small.extend_from_slice(p);
                small.push(0);
            }
        }
        add_paths(&g, &small)?;
    }
    g.run(&["write-tree"])
}
pub fn snap_locked(root: &Root, repo: &Path, meta: &Meta) -> Result<(String, Option<String>)> {
    let tree = capture_tree(root, repo)?;
    let g = Git::shadow(root, repo);
    let parent = if g.run_status(&["rev-parse", "-q", "--verify", REF])? {
        Some(g.run(&["rev-parse", REF])?)
    } else {
        None
    };
    if let Some(p) = &parent
        && g.run(&["rev-parse", &format!("{p}^{{tree}}")])? == tree
    {
        return Ok((tree, None));
    }
    let mut args = vec!["commit-tree", &tree];
    if let Some(p) = &parent {
        args.extend(["-p", p]);
    }
    args.extend(["-F", "-"]);
    let commit = g.run_stdin(&args, format_message(meta).as_bytes())?;
    g.run(&[
        "update-ref",
        REF,
        &commit,
        parent
            .as_deref()
            .unwrap_or("0000000000000000000000000000000000000000"),
    ])?;
    crate::prune::spawn_if_due(root, repo);
    Ok((tree, Some(commit)))
}
pub fn snap(root: &Root, meta: &Meta) -> Result<Option<String>> {
    let repo = paths::repo_for(&root.path);
    fs::create_dir_all(&repo)?;
    let _lock = lock::acquire(&repo, Duration::from_secs(2))?;
    Ok(snap_locked(root, &repo, meta)?.1)
}
pub fn cli_snap(trigger: String, mut cmd: Option<String>, cwd: Option<PathBuf>) {
    if std::env::var_os("UNDO_DISABLE").is_some_and(|s| s == "1") {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    git::set_deadline(deadline);
    let mut cwd = cwd
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let result = (|| -> Result<()> {
        git::check_version()?;
        if trigger == "claude" {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut s = String::new();
                let r = std::io::stdin()
                    .take(1_048_577)
                    .read_to_string(&mut s)
                    .map(|_| s);
                let _ = tx.send(r);
            });
            let s = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| anyhow::anyhow!("snapshot timed out"))??;
            if s.len() > 1_048_576 || serde_json::from_str::<serde_json::Value>(&s).is_err() {
                log::error(None, "invalid Claude JSON");
            } else {
                let (c, m) = parse_claude_input(&s);
                if let Some(c) = c {
                    cwd = c;
                }
                cmd = m;
            }
        }
        if let Some(root) = crate::root::find_root(&cwd) {
            snap(
                &root,
                &Meta {
                    trigger,
                    cmd,
                    cwd: cwd.clone(),
                },
            )?;
        }
        if Instant::now() >= deadline {
            bail!("snapshot timed out");
        }
        Ok(())
    })();
    if let Err(e) = result {
        let msg = format!("{e:#}");
        log::error(
            Some(&cwd),
            if msg.contains("timed out") {
                "snapshot timed out"
            } else {
                &msg
            },
        );
    }
}
#[derive(Clone, Debug)]
pub struct Commit {
    pub id: String,
    pub tree: String,
    pub ct: u64,
    pub message: String,
}
impl Commit {
    pub fn field(&self, name: &str) -> &str {
        self.message
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .unwrap_or("")
    }
}
pub fn history(root: &Root, repo: &Path) -> Result<Vec<Commit>> {
    let g = Git::shadow(root, repo);
    if !repo.join("HEAD").exists() || !g.run_status(&["rev-parse", "-q", "--verify", REF])? {
        return Ok(vec![]);
    }
    let data = g.bytes(&[
        "log",
        "--first-parent",
        "-z",
        "--format=%H%x00%T%x00%ct%x00%B",
        REF,
    ])?;
    let fields: Vec<_> = data.split(|b| *b == 0).collect();
    let mut commits = Vec::new();
    for f in fields.as_chunks::<4>().0 {
        commits.push(Commit {
            id: String::from_utf8(f[0].to_vec())?,
            tree: String::from_utf8(f[1].to_vec())?,
            ct: std::str::from_utf8(f[2])?.parse()?,
            message: String::from_utf8(f[3].to_vec())?,
        });
    }
    Ok(commits)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn messages_truncate_chars_and_escape_newlines() {
        let m = Meta {
            trigger: "shell".into(),
            cmd: Some(format!("{}\nsecond", "é".repeat(2100))),
            cwd: "/p".into(),
        };
        let s = format_message(&m);
        assert_eq!(
            s.lines().next().unwrap(),
            format!("shell: {}", "é".repeat(200))
        );
        assert!(s.contains(&format!("undo-cmd: {}\n", "é".repeat(2000))));
        assert!(
            format_message(&Meta {
                cmd: Some("a\nb".into()),
                ..m
            })
            .contains("undo-cmd: a\\nb\n")
        );
        assert!(
            format_message(&Meta {
                trigger: "manual".into(),
                cmd: None,
                cwd: "/p".into()
            })
            .starts_with("manual: (none)\n\n")
        );
    }
    #[test]
    fn claude_fields() {
        assert_eq!(
            parse_claude_input(
                r#"{"cwd":"/p","tool_name":"Bash","tool_input":{"command":"rm x"}}"#
            ),
            (Some("/p".into()), Some("rm x".into()))
        );
        assert_eq!(
            parse_claude_input(r#"{"tool_name":"Edit","tool_input":{"file_path":"a"}}"#).1,
            Some("Edit a".into())
        );
        assert_eq!(parse_claude_input("bad"), (None, None));
    }
}
