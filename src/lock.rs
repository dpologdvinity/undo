use std::{fs::File, path::Path, time::Duration};
pub fn acquire(repo: &Path, deadline: Duration) -> anyhow::Result<File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(repo.join("undo.lock"))?;
    let end = std::time::Instant::now() + deadline;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(std::fs::TryLockError::WouldBlock) => anyhow::bail!("lock timeout"),
            Err(e) => return Err(e.into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn second_lock_times_out() {
        let d = tempfile::tempdir().unwrap();
        let first = acquire(d.path(), Duration::from_millis(100)).unwrap();
        assert!(
            acquire(d.path(), Duration::from_millis(60))
                .unwrap_err()
                .to_string()
                .contains("lock timeout")
        );
        drop(first);
        assert!(acquire(d.path(), Duration::from_millis(60)).is_ok());
    }
}
