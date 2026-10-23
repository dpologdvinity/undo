use std::path::{Path, PathBuf};
use std::{fs, os::unix::fs::PermissionsExt};
pub fn data_home() -> PathBuf {
    std::env::var_os("UNDO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(|p| PathBuf::from(p).join("undo")))
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/undo")
        })
}
pub fn repo_for(root: &Path) -> PathBuf {
    data_home()
        .join("repos")
        .join(format!("{}.git", root_hash(root)))
}
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}
pub fn private_repo(repo: &Path) -> std::io::Result<()> {
    let home = data_home();
    private_dir(&home)?;
    private_dir(&home.join("repos"))?;
    private_dir(repo)
}
fn root_hash(root: &Path) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(root.as_os_str().as_encoded_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hashes_are_stable_and_distinct() {
        assert_eq!(
            root_hash(Path::new("/a")),
            "6a50dc8584134c7de537c0052ff6d236bf874355e050c90523e0c5ff2a543a28"
        );
        assert_ne!(root_hash(Path::new("/a")), root_hash(Path::new("/b")));
    }
}
