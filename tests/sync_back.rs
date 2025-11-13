use std::fs;
use std::process::Command;

#[path = "support/mod.rs"]
mod support;

use support::{git, normalize_newlines, CliFixture};

#[test]
fn sync_back_pushes_branch_with_local_edits() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("repo-sync", "skills/sample", "sample");
    fx.install_from_remote(&remote, "sample");

    let skill_dir = fx.skill_dir("sample");
    fs::write(skill_dir.join("file.txt"), "local edit\n").unwrap();

    let mut cmd = fx.sk_cmd();
    let out = cmd
        .args([
            "sync-back",
            "sample",
            "--branch",
            "sync/test",
            "--message",
            "sync test",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "sync-back failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    git(&["fetch", "origin"], &remote.work);
    git(&["checkout", "sync/test"], &remote.work);
    let synced =
        fs::read_to_string(remote.work.join(remote.skill_path()).join("file.txt")).unwrap();
    assert_eq!(normalize_newlines(&synced), "local edit\n");

    let log = Command::new("git")
        .args([
            "-C",
            remote.work.to_str().unwrap(),
            "log",
            "-1",
            "--pretty=%s",
        ])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&log.stdout).contains("sync test"),
        "commit message propagated"
    );
}
