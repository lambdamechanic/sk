use std::{process::Output, str};

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

fn stdout_string(output: &Output) -> String {
    support::normalize_newlines(str::from_utf8(&output.stdout).unwrap())
}

#[test]
fn diff_reports_clean_install() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("diff-clean", "skill", "demo");
    fix.install_from_remote(&remote, "demo");

    let output = fix.sk_cmd().args(["diff"]).output().unwrap();
    assert!(
        output.status.success(),
        "sk diff failed: {:?}",
        output.stderr
    );
    let stdout = stdout_string(&output);
    assert!(
        stdout.contains("(no differences)"),
        "expected clean diff output:\n{}",
        stdout
    );
}

#[test]
fn diff_shows_remote_updates_after_cache_refresh() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("diff-update", "skill", "demo");
    fix.install_from_remote(&remote, "demo");

    remote.overwrite_file("file.txt", "v2\n", "v2");
    fix.sk_success(&["update"]);

    let output = fix.sk_cmd().args(["diff", "demo"]).output().unwrap();
    assert!(
        output.status.success(),
        "sk diff failed: {:?}",
        output.stderr
    );
    let stdout = stdout_string(&output);
    assert!(
        stdout.contains("+v2"),
        "expected diff to show remote additions:\n{}",
        stdout
    );
}
