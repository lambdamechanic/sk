use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::{git, parse_status_entries, CliFixture};

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

#[test]
fn doctor_skips_upgrade_when_only_other_paths_change() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("doctor-upgrade-none", "skills/demo", "demo");
    fx.install_from_remote(&remote, "demo");

    let outside = remote.work.join("UNRELATED.txt");
    fs::write(&outside, "outside change\n").unwrap();
    git(&["add", "UNRELATED.txt"], &remote.work);
    git(&["commit", "-m", "outside change"], &remote.work);
    git(&["push", "origin", "main"], &remote.work);

    fx.sk_success(&["update"]);

    let mut cmd = fx.sk_cmd();
    let out = cmd.args(["doctor"]).output().unwrap();
    assert!(
        out.status.success(),
        "doctor should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("Upgrade available"),
        "doctor should not report upgrades for unrelated repo changes: {stdout}"
    );
}

#[test]
fn doctor_reports_upgrade_when_skill_path_changes() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("doctor-upgrade-hit", "skills/demo", "demo");
    fx.install_from_remote(&remote, "demo");

    remote.overwrite_file("file.txt", "v2\n", "touch skill contents");
    fx.sk_success(&["update"]);

    let mut cmd = fx.sk_cmd();
    let out = cmd.args(["doctor"]).output().unwrap();
    assert!(
        out.status.success(),
        "doctor should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Upgrade available"),
        "doctor should report upgrades when skill path changes: {stdout}"
    );
}
