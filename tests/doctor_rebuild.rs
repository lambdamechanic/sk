use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::{parse_status_entries, CliFixture};

#[test]
fn doctor_rebuilds_missing_install_from_locked_commit() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("repo-doctor", "skills/demo", "demo");
    fx.install_from_remote(&remote, "demo");

    let install_dir = fx.skill_dir("demo");
    assert!(install_dir.exists());

    fs::remove_dir_all(&install_dir).unwrap();
    assert!(
        !install_dir.exists(),
        "test precondition: install dir deleted"
    );

    let mut cmd = fx.sk_cmd();
    let out = cmd.args(["doctor", "--apply"]).output().unwrap();
    assert!(
        out.status.success(),
        "doctor --apply should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(install_dir.exists(), "doctor rebuilds from lock/cache");
    let status = parse_status_entries(fx.run_json(&["status", "--json"]));
    assert_eq!(status[0].state, "clean", "rebuild yielded clean install");
}
