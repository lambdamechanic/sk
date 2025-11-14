use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[test]
fn remove_deletes_clean_install() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("repo-clean", "skill", "sample");
    fix.install_from_remote(&remote, "sample");
    let skill_dir = fix.skill_dir("sample");
    assert!(skill_dir.exists(), "skill dir created");

    let out = fix.sk_cmd().args(["remove", "sample"]).output().unwrap();
    assert!(out.status.success(), "sk remove failed: {out:?}");

    assert!(
        !skill_dir.exists(),
        "skill directory should be deleted after remove"
    );
    let lock = fix.lock_json();
    assert!(
        lock["skills"].as_array().unwrap().is_empty(),
        "lockfile entry dropped"
    );
}

#[test]
fn remove_refuses_dirty_install_without_force() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("repo-dirty", "skill", "sample");
    fix.install_from_remote(&remote, "sample");
    let skill_dir = fix.skill_dir("sample");
    fs::write(skill_dir.join("file.txt"), "local edit\n").unwrap();

    let out = fix.sk_cmd().args(["remove", "sample"]).output().unwrap();
    assert!(
        !out.status.success(),
        "remove should fail when install is modified"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Refusing to remove"),
        "stderr should mention refusal: {stderr}"
    );
    assert!(skill_dir.exists(), "dir left intact on failure");
    let lock = fix.lock_json();
    assert_eq!(lock["skills"].as_array().unwrap().len(), 1);
}

#[test]
fn remove_force_allows_dirty_install() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("repo-force", "skill", "sample");
    fix.install_from_remote(&remote, "sample");
    let skill_dir = fix.skill_dir("sample");
    fs::write(skill_dir.join("file.txt"), "local edit\n").unwrap();

    let out = fix
        .sk_cmd()
        .args(["remove", "sample", "--force"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "forced remove should succeed: {out:?}"
    );

    assert!(
        !skill_dir.exists(),
        "skill directory deleted even when dirty with --force"
    );
    let lock = fix.lock_json();
    assert!(
        lock["skills"].as_array().unwrap().is_empty(),
        "lockfile entry dropped after forced remove"
    );
}
