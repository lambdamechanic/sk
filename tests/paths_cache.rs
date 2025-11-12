use std::env;

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
    let proj_dir = tempfile::tempdir().unwrap();
    let proj = proj_dir.path().to_path_buf();

    let rel = resolve_project_path(&proj, "subdir/file.txt");
    assert_eq!(rel, proj.join("subdir/file.txt"));

    let abs_path = tempfile::tempdir().unwrap().path().join("abs.txt");
    let abs_str = abs_path.to_string_lossy().to_string();
    let abs = resolve_project_path(&proj, &abs_str);
    assert_eq!(abs, abs_path);
}
