use std::path::Path;
pub fn error(root: Option<&Path>, msg: &str) {
    let dir = crate::paths::data_home();
    if crate::paths::private_dir(&dir).is_err() {
        return;
    }
    let line = format!(
        "{} {} {}\n",
        crate::paths::now(),
        root.map(|p| p.to_string_lossy())
            .unwrap_or_else(|| "-".into()),
        msg.replace(['\r', '\n'], " ")
    );
    let _ = append(&dir.join("log"), &line);
}
fn append(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    file.try_lock()?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.seek(SeekFrom::End(0))?;
    file.write_all(line.as_bytes())?;
    if file.metadata()?.len() > 1_048_576 {
        file.seek(SeekFrom::End(-524_288))?;
        let mut tail = Vec::new();
        file.read_to_end(&mut tail)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&tail)?;
        file.set_len(tail.len() as u64)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn truncation_keeps_tail() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("log");
        std::fs::write(&p, "x".repeat(1_048_576)).unwrap();
        super::append(&p, "last entry\n").unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(bytes.len(), 524_288);
        assert!(bytes.ends_with(b"last entry\n"));
    }
}
