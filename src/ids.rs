use crate::root::Root;
use anyhow::Result;
use std::path::Path;
fn index(id: &str) -> Option<usize> {
    let n = id.strip_prefix('@')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}
pub fn resolve(root: &Root, repo: &Path, id: &str) -> Result<String> {
    let g = crate::git::Git::shadow(root, repo);
    let commits = if g.run_status(&["rev-parse", "-q", "--verify", crate::snapshot::REF])? {
        g.run(&["rev-list", "--first-parent", crate::snapshot::REF])?
    } else {
        String::new()
    };
    if let Some(n) = index(id) {
        return commits
            .lines()
            .nth(n)
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("unknown snapshot {id}"));
    }
    if id.len() < 4 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        anyhow::bail!("unknown snapshot {id}");
    }
    let prefix = id.to_ascii_lowercase();
    let mut matches = commits.lines().filter(|c| c.starts_with(&prefix));
    let first = matches
        .next()
        .ok_or_else(|| anyhow::anyhow!("unknown snapshot {id}"))?;
    if matches.next().is_some() {
        anyhow::bail!("ambiguous snapshot prefix {id}");
    }
    Ok(first.to_owned())
}
#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_index() {
        assert_eq!(super::index("@0"), Some(0));
        assert_eq!(super::index("@12"), Some(12));
        for s in [
            "@",
            "@-1",
            "@+1",
            "@a",
            "12",
            "@9999999999999999999999999999999",
        ] {
            assert_eq!(super::index(s), None);
        }
    }
}
