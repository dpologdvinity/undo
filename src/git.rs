use crate::root::Root;
use anyhow::{Context, Result, bail};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
static DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);
static VERSION: OnceLock<std::result::Result<(), String>> = OnceLock::new();
#[derive(Clone)]
pub struct Git {
    cwd: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
    config: Vec<OsString>,
    shadow: bool,
}
impl Git {
    pub fn plain() -> Self {
        Self {
            cwd: None,
            env: vec![],
            config: vec![],
            shadow: false,
        }
    }
    pub fn at(mut self, path: &Path) -> Self {
        self.cwd = Some(path.into());
        self
    }
    pub fn shadow(root: &Root, repo: &Path) -> Self {
        let mut g = Self::plain().at(&root.path);
        g.shadow = true;
        for (k, v) in [
            ("GIT_DIR", repo.as_os_str()),
            ("GIT_WORK_TREE", root.path.as_os_str()),
            ("GIT_INDEX_FILE", repo.join("undo-index").as_os_str()),
        ] {
            g.env.push((k.into(), v.into()));
        }
        for (k, v) in [
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "undo"),
            ("GIT_AUTHOR_EMAIL", "undo@localhost"),
            ("GIT_COMMITTER_NAME", "undo"),
            ("GIT_COMMITTER_EMAIL", "undo@localhost"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("LC_ALL", "C"),
        ] {
            g.env.push((k.into(), v.into()));
        }
        g.config = [
            "core.hooksPath=/dev/null",
            "core.fsmonitor=false",
            "core.autocrlf=false",
            "core.safecrlf=false",
            "gc.auto=0",
            "core.quotePath=false",
            "advice.addEmbeddedRepo=false",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        if root.is_git
            && let Some(common) = &root.git_common_dir
        {
            let exclude = common.join("info/exclude");
            if exclude.is_file() {
                let mut value = OsString::from("core.excludesFile=");
                value.push(exclude);
                g.config.push(value);
            }
        }
        g
    }
    pub fn index(self, path: &Path) -> Self {
        self.env("GIT_INDEX_FILE", path.as_os_str())
    }
    pub fn env(mut self, key: &str, value: impl Into<OsString>) -> Self {
        self.env.retain(|(k, _)| k != key);
        self.env.push((key.into(), value.into()));
        self
    }
    pub fn command(&self) -> Command {
        let mut c = Command::new("git");
        if let Some(cwd) = &self.cwd {
            c.current_dir(cwd);
        }
        for (k, _) in std::env::vars_os() {
            if (self.shadow && k.as_encoded_bytes().starts_with(b"GIT_"))
                || ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"]
                    .iter()
                    .any(|s| k == *s)
            {
                c.env_remove(k);
            }
        }
        for (k, v) in &self.env {
            c.env(k, v);
        }
        for v in &self.config {
            c.args([OsString::from("-c"), v.clone()]);
        }
        c
    }
    pub fn output(&self, args: &[&str], input: &[u8]) -> Result<Output> {
        let deadline = *DEADLINE.lock().unwrap();
        if deadline.is_some_and(|d| Instant::now() >= d) {
            bail!("timed out");
        }
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("could not run git")?;
        let mut stdin = child.stdin.take().unwrap();
        let input = input.to_vec();
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let out = std::thread::spawn(move || {
            let mut b = Vec::new();
            stdout.read_to_end(&mut b).map(|_| b)
        });
        let err = std::thread::spawn(move || {
            let mut b = Vec::new();
            stderr.read_to_end(&mut b).map(|_| b)
        });
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break s;
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                let _ = child.kill();
                let _ = child.wait();
                bail!("timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let stdout = out
            .join()
            .map_err(|_| anyhow::anyhow!("git stdout reader failed"))??;
        let stderr = err
            .join()
            .map_err(|_| anyhow::anyhow!("git stderr reader failed"))??;
        let written = writer
            .join()
            .map_err(|_| anyhow::anyhow!("git stdin writer failed"))?;
        if status.success() {
            written?;
        }
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }
    pub fn bytes(&self, args: &[&str]) -> Result<Vec<u8>> {
        self.bytes_stdin(args, &[])
    }
    pub fn bytes_stdin(&self, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
        let o = self.output(args, input)?;
        if !o.status.success() {
            bail!(
                "git {}: {}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Ok(o.stdout)
    }
    pub fn run(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8(self.bytes(args)?)?
            .trim_end_matches('\n')
            .into())
    }
    pub fn run_stdin(&self, args: &[&str], input: &[u8]) -> Result<String> {
        Ok(String::from_utf8(self.bytes_stdin(args, input)?)?
            .trim_end_matches('\n')
            .into())
    }
    pub fn run_status(&self, args: &[&str]) -> Result<bool> {
        Ok(self.output(args, &[])?.status.success())
    }
}
pub fn set_deadline(d: Instant) {
    *DEADLINE.lock().unwrap() = Some(d);
}
pub fn check_version() -> Result<()> {
    let result = VERSION.get_or_init(|| {
        Git::plain()
            .run(&["--version"])
            .and_then(|s| {
                if version_ok(&s) {
                    Ok(())
                } else {
                    bail!("git >= 2.25 required")
                }
            })
            .map_err(|e| e.to_string())
    });
    result.as_ref().map_err(|s| anyhow::anyhow!("{s}")).copied()
}
fn version_ok(s: &str) -> bool {
    let version = s
        .strip_prefix("git version ")
        .unwrap_or(s)
        .split_whitespace()
        .next()
        .unwrap_or("");
    let mut p = version.split('.').filter_map(|s| s.parse::<u32>().ok());
    matches!((p.next(), p.next()), (Some(major), Some(minor)) if major > 2 || (major == 2 && minor >= 25))
}
#[cfg(test)]
mod tests {
    #[test]
    fn git_versions() {
        assert!(super::version_ok("git version 2.55.0"));
        assert!(super::version_ok("git version 2.39.3 (Apple Git-146)"));
        assert!(!super::version_ok("git version 2.24.1"));
        assert!(!super::version_ok("garbage"));
    }
}
