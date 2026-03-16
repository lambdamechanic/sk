use std::fs;
use std::path::{Path, PathBuf};

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[test]
fn migrate_root_moves_legacy_skills_and_updates_legacy_config() {
    let fx = CliFixture::new();
    write_skill(&fx.project.join("skills/demo"), "demo");
    write_config(&fx, r#"{"default_root":"./skills"}"#);

    let output = fx.sk_cmd().args(["migrate-root"]).output().unwrap();
    assert!(
        output.status.success(),
        "migrate-root failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !fx.project.join("skills").exists(),
        "legacy root should be moved"
    );
    assert!(
        fx.project.join(".agents/skills/demo/SKILL.md").exists(),
        "managed root should contain migrated skill"
    );

    let config = fs::read_to_string(fx.config_dir().join("config.json")).unwrap();
    assert!(
        config.contains("\"default_root\": \"./.agents/skills\""),
        "unexpected config contents: {config}"
    );
}

#[test]
fn where_falls_back_to_legacy_root_with_warning() {
    let fx = CliFixture::new();
    write_skill(&fx.project.join("skills/demo"), "demo");

    let output = fx.sk_cmd().args(["where", "demo"]).output().unwrap();
    assert!(
        output.status.success(),
        "where should succeed for legacy repos"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("/skills/demo"),
        "unexpected stdout: {stdout}"
    );
    assert!(
        stderr.contains("legacy './skills' layout") && stderr.contains("sk migrate-root"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn where_uses_new_root_when_config_still_points_at_legacy_path() {
    let fx = CliFixture::new();
    write_skill(&fx.project.join(".agents/skills/demo"), "demo");
    write_config(&fx, r#"{"default_root":"./skills"}"#);

    let output = fx.sk_cmd().args(["where", "demo"]).output().unwrap();
    assert!(
        output.status.success(),
        "where should succeed for migrated repos"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("/.agents/skills/demo"),
        "unexpected stdout: {stdout}"
    );
    assert!(
        stderr.contains("default_root still points to './skills'"),
        "unexpected stderr: {stderr}"
    );
}

#[cfg_attr(windows, ignore = "requires directory symlink support")]
#[test]
fn migrate_root_repairs_existing_native_exposures() {
    let fx = CliFixture::new();
    let legacy_root = fx.project.join("skills");
    write_skill(&legacy_root.join("demo"), "demo");
    create_dir_symlink(Path::new("../skills"), &fx.project.join(".agents/skills")).unwrap();
    create_dir_symlink(Path::new("../skills"), &fx.project.join(".claude/skills")).unwrap();

    let output = fx.sk_cmd().args(["migrate-root"]).output().unwrap();
    assert!(
        output.status.success(),
        "migrate-root failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let managed_root = fx.project.join(".agents/skills").canonicalize().unwrap();
    assert!(managed_root.join("demo/SKILL.md").exists());
    assert!(!fs::symlink_metadata(fx.project.join(".agents/skills"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_symlink_to(&fx.project.join(".claude/skills"), &managed_root);
}

fn write_skill(dir: &Path, name: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: fixture\n---\n"),
    )
    .unwrap();
}

fn write_config(fx: &CliFixture, body: &str) {
    fs::create_dir_all(fx.config_dir()).unwrap();
    fs::write(fx.config_dir().join("config.json"), body).unwrap();
}

fn assert_symlink_to(link_path: &Path, expected_target: &Path) {
    let raw_target = fs::read_link(link_path).unwrap();
    let resolved = resolve_link(link_path, &raw_target).canonicalize().unwrap();
    assert_eq!(resolved, expected_target);
}

fn resolve_link(link_path: &Path, raw_target: &Path) -> PathBuf {
    if raw_target.is_absolute() {
        raw_target.to_path_buf()
    } else {
        link_path.parent().unwrap().join(raw_target)
    }
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link_path: &Path) -> std::io::Result<()> {
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)?;
    }
    std::os::unix::fs::symlink(target, link_path)
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link_path: &Path) -> std::io::Result<()> {
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)?;
    }
    std::os::windows::fs::symlink_dir(target, link_path)
}

#[test]
fn migrate_root_keep_existing_skips_when_destination_exists() {
    let fx = CliFixture::new();
    write_skill(&fx.project.join("skills/demo"), "demo");
    write_skill(&fx.project.join(".agents/skills/demo"), "demo");
    write_config(&fx, r#"{"default_root":"./skills"}"#);

    let output = fx
        .sk_cmd()
        .args(["migrate-root", "--keep-existing"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "migrate-root --keep-existing failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Skipping migration"),
        "expected skip message, got: {stdout}"
    );

    // Both should still exist
    assert!(
        fx.project.join("skills/demo/SKILL.md").exists(),
        "legacy root should still exist"
    );
    assert!(
        fx.project.join(".agents/skills/demo/SKILL.md").exists(),
        "new root should still exist"
    );
}

#[test]
fn migrate_root_force_overwrites_destination() {
    let fx = CliFixture::new();
    write_skill(&fx.project.join("skills/demo"), "demo-old");
    write_skill(&fx.project.join(".agents/skills/demo"), "demo-new");
    write_config(&fx, r#"{"default_root":"./skills"}"#);

    let output = fx
        .sk_cmd()
        .args(["migrate-root", "--force"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "migrate-root --force failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Warning: removing existing destination"),
        "expected force warning, got: {stderr}"
    );

    // Legacy should be moved, new should contain old content
    assert!(
        !fx.project.join("skills").exists(),
        "legacy root should be moved"
    );
    let skill_content =
        fs::read_to_string(fx.project.join(".agents/skills/demo/SKILL.md")).unwrap();
    assert!(
        skill_content.contains("demo-old"),
        "should contain migrated content, got: {skill_content}"
    );
}

#[test]
fn migrate_root_errors_without_flag_when_destination_exists() {
    let fx = CliFixture::new();
    write_skill(&fx.project.join("skills/demo"), "demo");
    write_skill(&fx.project.join(".agents/skills/demo"), "demo");
    write_config(&fx, r#"{"default_root":"./skills"}"#);

    let output = fx.sk_cmd().args(["migrate-root"]).output().unwrap();
    assert!(
        !output.status.success(),
        "migrate-root should fail without --force or --keep-existing"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Use --force to overwrite or --keep-existing to skip"),
        "expected usage hint, got: {stderr}"
    );
}
