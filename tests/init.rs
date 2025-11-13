#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[test]
fn init_creates_install_root_and_lockfile() {
    let fx = CliFixture::new();

    fx.sk_success(&["init"]);

    let skills_dir = fx.project.join("skills");
    assert!(skills_dir.exists(), "init creates install root");

    let lock = fx.lock_json();
    assert_eq!(lock["version"].as_i64(), Some(1));
    assert!(
        lock["skills"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false),
        "fresh init has empty lockfile"
    );

    // Idempotent rerun should also succeed
    fx.sk_success(&["init"]);
}
