use std::fs;
use std::path::{Path, PathBuf};

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[cfg_attr(windows, ignore = "requires directory symlink support")]
#[test]
fn init_with_expose_both_creates_native_roots() {
    let fx = CliFixture::new();

    fx.sk_success(&["init", "--expose", "both"]);

    let managed_root = fx.managed_root().canonicalize().unwrap();
    assert_eq!(
        managed_root,
        fx.project.join(".agents/skills").canonicalize().unwrap()
    );
    assert_symlink_to(&fx.project.join(".claude/skills"), &managed_root);
}

#[cfg_attr(windows, ignore = "requires directory symlink support")]
#[test]
fn expose_is_idempotent() {
    let fx = CliFixture::new();

    fx.sk_success(&["init"]);
    fx.sk_success(&["expose", "codex"]);
    fx.sk_success(&["expose", "codex"]);

    let managed_root = fx.managed_root().canonicalize().unwrap();
    assert_eq!(
        managed_root,
        fx.project.join(".agents/skills").canonicalize().unwrap()
    );
}

#[test]
fn expose_rejects_conflicting_target_paths() {
    let fx = CliFixture::new();

    fx.sk_success(&["init"]);
    fs::create_dir_all(fx.project.join(".claude/skills")).unwrap();

    let output = fx.sk_cmd().args(["expose", "claude"]).output().unwrap();
    assert!(!output.status.success(), "expose should fail on conflicts");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists and is not managed by sk"),
        "unexpected stderr: {stderr}"
    );
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
