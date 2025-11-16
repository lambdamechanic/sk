use std::{fs, process::Output, str};

#[path = "support/mod.rs"]
mod support;

use serde_json::Value as Json;
use support::{cache_repo_path, CliFixture};

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

#[test]
fn diff_recovers_missing_cache() {
    let fix = CliFixture::new();
    let remote = fix.create_remote("diff-missing-cache", "skill", "demo");
    fix.install_from_remote(&remote, "demo");

    let cache_dir = cache_dir_for_first_skill(fix.lock_json(), fix.cache_root());
    if cache_dir.exists() {
        fs::remove_dir_all(&cache_dir).unwrap();
    }

    let output = fix.sk_cmd().args(["diff", "demo"]).output().unwrap();
    assert!(
        output.status.success(),
        "sk diff failed: {:?}",
        output.stderr
    );
    let stdout = stdout_string(&output);
    assert!(
        stdout.contains("(no differences)"),
        "expected clean diff output after re-cloning the cache:\n{}",
        stdout
    );
}

fn cache_dir_for_first_skill(lock: Json, cache_root: &std::path::Path) -> std::path::PathBuf {
    let skill = lock["skills"]
        .as_array()
        .and_then(|arr| arr.first())
        .expect("lock contains at least one skill");
    let source = &skill["source"];
    let host = source["host"].as_str().expect("host");
    let owner = source["owner"].as_str().expect("owner");
    let repo = source["repo"].as_str().expect("repo");
    let url = source["url"].as_str().expect("url");
    cache_repo_path(cache_root, host, owner, repo, url)
}
