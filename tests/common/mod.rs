#![allow(dead_code)]
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};
use tempfile::TempDir;
pub struct Sandbox {
    pub dir: TempDir,
    pub home: PathBuf,
    pub root: PathBuf,
}
impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}
impl Sandbox {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let root = dir.path().join("root");
        fs::create_dir_all(home.join(".local/share/undo")).unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::write(
            home.join(".gitconfig"),
            "[user]\nname = t\nemail = t@t\n[init]\ndefaultBranch = main\n",
        )
        .unwrap();
        Self { dir, home, root }
    }
    pub fn command(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        for (key, _) in std::env::vars_os() {
            if key.as_encoded_bytes().starts_with(b"GIT_") {
                c.env_remove(key);
            }
        }
        c.current_dir(&self.root)
            .env("HOME", &self.home)
            .env("UNDO_HOME", self.home.join(".local/share/undo"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("GIT_CONFIG_GLOBAL", self.home.join(".gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("UNDO_SHELL_HOOK")
            .env_remove("UNDO_DISABLE")
            .env_remove("UNDO_MAX_BYTES");
        c
    }
    pub fn sh(&self, script: &str) -> Output {
        self.command("sh").args(["-c", script]).output().unwrap()
    }
    pub fn write(&self, rel: &str, contents: &str) {
        let p = self.root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, contents).unwrap();
    }
    pub fn git_root() -> Self {
        let s = Self::new();
        s.write("src/main.rs", "fn main() {}\n");
        s.write("README.md", "project\n");
        success(s.sh(
            "git -c user.name=t -c user.email=t@t init -q && git add . && git commit -qm initial",
        ));
        s
    }
    pub fn undo(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_undo"))
            .args(args)
            .output()
            .unwrap()
    }
    pub fn undo_stdin(&self, args: &[&str], stdin: &str) -> Output {
        use std::io::Write;
        let mut child = self
            .command(env!("CARGO_BIN_EXE_undo"))
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
    pub fn data(&self) -> PathBuf {
        self.home.join(".local/share/undo")
    }
    pub fn repo(&self) -> PathBuf {
        fs::read_dir(self.data().join("repos"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
    }
    pub fn tree_hash(&self) -> String {
        hash_tree(&self.root, true)
    }
}

pub fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
pub fn hash_tree(root: &std::path::Path, exclude_git: bool) -> String {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    fn walk(base: &std::path::Path, dir: &std::path::Path, exclude_git: bool, h: &mut Sha256) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if exclude_git && e.file_name() == ".git" {
                continue;
            }
            let p = e.path();
            let m = fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                walk(base, &p, exclude_git, h);
                continue;
            }
            let rel = p.strip_prefix(base).unwrap().as_os_str().as_encoded_bytes();
            h.update((rel.len() as u64).to_le_bytes());
            h.update(rel);
            h.update([u8::from(m.file_type().is_symlink())]);
            h.update((m.permissions().mode() & 0o111).to_le_bytes());
            let contents = if m.file_type().is_symlink() {
                fs::read_link(&p)
                    .unwrap()
                    .as_os_str()
                    .as_encoded_bytes()
                    .to_vec()
            } else {
                fs::read(&p).unwrap()
            };
            h.update((contents.len() as u64).to_le_bytes());
            h.update(contents);
        }
    }
    let mut h = Sha256::new();
    walk(root, root, exclude_git, &mut h);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
