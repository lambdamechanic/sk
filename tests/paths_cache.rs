use std::env;
use std::path::PathBuf;

use sk::paths::{cache_root, resolve_project_path};

#[test]
fn cache_root_respects_env_override() {
    let td = tempfile::tempdir().unwrap();
    let override_dir = td.path().join("cache_override");
    std::fs::create_dir_all(&override_dir).unwrap();

    env::set_var("SK_CACHE_DIR", &override_dir);
    let root = cache_root();
    // cache_root appends "repos" to the override
    assert_eq!(root, override_dir.join("repos"));
}

#[test]
fn resolve_project_path_rel_and_abs() {
    let proj = PathBuf::from("/tmp/project-root");
    let rel = resolve_project_path(&proj, "subdir/file.txt");
    assert_eq!(rel, PathBuf::from("/tmp/project-root/subdir/file.txt"));

    let abs = resolve_project_path(&proj, "/var/log/syslog");
    assert_eq!(abs, PathBuf::from("/var/log/syslog"));
}

