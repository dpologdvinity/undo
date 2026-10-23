mod common;
use common::Sandbox;

#[test]
fn tree_hash_tracks_content_not_mtime() {
    let s = Sandbox::new();
    s.write("file", "one");
    let first = s.tree_hash();
    s.write("file", "two");
    assert_ne!(first, s.tree_hash());
    let second = s.tree_hash();
    assert!(s.sh("touch file").status.success());
    assert_eq!(second, s.tree_hash());
}
