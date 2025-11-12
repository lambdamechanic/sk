use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::prelude::*;
use predicates::str::contains;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

fn git(args: &[&str], cwd: &Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success(), "git {:?} failed in {}", args, cwd.display());
}

fn cache_repo_path(root: &Path, host: &str, owner: &str, repo: &str) -> PathBuf {
    root.join("repos").join(host).join(owner).join(repo)
}

fn write_lockfile(project: &Path, skills_json: &str) {
    let body = format!(
        "{{\n  \"version\":1,\n  \"skills\": [\n{}\n  ],\n  \"generatedAt\": \"2020-01-01T00:00:00Z\"\n}}\n",
        skills_json
    );
    fs::write(project.join("skills.lock.json"), body).unwrap();
}

#[test]
fn doctor_reports_duplicate_install_names() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Two entries with the same installName: "dup"
    write_lockfile(
        &project,
        r#"    {
      "installName": "dup",
      "source": {
        "url": "git@local:o/r0.git",
        "host": "local",
        "owner": "o",
        "repo": "r0",
        "skillPath": "skill-0"
      },
      "ref": null,
      "commit": "deadbeef",
      "digest": "abc",
      "installedAt": "2020-01-01T00:00:00Z"
    },
    {
      "installName": "dup",
      "source": {
        "url": "git@local:o/r1.git",
        "host": "local",
        "owner": "o",
        "repo": "r1",
        "skillPath": "skill-1"
      },
      "ref": null,
      "commit": "cafebabe",
      "digest": "def",
      "installedAt": "2020-01-01T00:00:00Z"
    }"#,
    );

    let mut cmd = cargo_bin_cmd!("sk");
    cmd.current_dir(&project)
        .env("SK_CACHE_DIR", tmp.path().join("cache").to_str().unwrap())
        .args(["doctor"]);
    cmd.assert()
        .success()
        .stdout(contains("Duplicate installName in lockfile"));
}

#[test]
fn doctor_prunes_unreferenced_cache_with_apply() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Lockfile references local/o/r1; we will create an extra unreferenced cache local/o/r0
    write_lockfile(
        &project,
        r#"    {
      "installName": "a",
      "source": {
        "url": "git@local:o/r1.git",
        "host": "local",
        "owner": "o",
        "repo": "r1",
        "skillPath": "skill-1"
      },
      "ref": null,
      "commit": "cafebabe",
      "digest": "def",
      "installedAt": "2020-01-01T00:00:00Z"
    }"#,
    );

    let cache_root = tmp.path().join("cache");
    let unref = cache_repo_path(&cache_root, "local", "o", "r0");
    fs::create_dir_all(unref.join(".git")).unwrap();
    let referenced = cache_repo_path(&cache_root, "local", "o", "r1");
    fs::create_dir_all(referenced.join(".git")).unwrap();

    let mut cmd = cargo_bin_cmd!("sk");
    cmd.current_dir(&project)
        .env("SK_CACHE_DIR", &cache_root)
        .args(["doctor", "--apply"]);
    cmd.assert().success();

    assert!(
        !unref.exists(),
        "unreferenced cache should be pruned by doctor --apply"
    );
    assert!(referenced.exists(), "referenced cache must remain");
}

#[test]
fn doctor_drops_orphan_lock_entries_and_normalizes_lockfile() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Two orphans (missing install dir, no cache/commit) -> should be removed on --apply
    write_lockfile(
        &project,
        r#"    {
      "installName": "b",
      "source": {
        "url": "git@local:o/r2.git",
        "host": "local",
        "owner": "o",
        "repo": "r2",
        "skillPath": "skill-2"
      },
      "ref": null,
      "commit": "1111111",
      "digest": "zzz",
      "installedAt": "2020-01-01T00:00:00Z"
    },
    {
      "installName": "a",
      "source": {
        "url": "git@local:o/r3.git",
        "host": "local",
        "owner": "o",
        "repo": "r3",
        "skillPath": "skill-3"
      },
      "ref": null,
      "commit": "2222222",
      "digest": "yyy",
      "installedAt": "2020-01-01T00:00:00Z"
    }"#,
    );

    let mut cmd = cargo_bin_cmd!("sk");
    cmd.current_dir(&project)
        .env("SK_CACHE_DIR", tmp.path().join("cache"))
        .args(["doctor", "--apply"]);
    cmd.assert().success();

    // Lockfile should now have zero skills after dropping both orphans
    let lock = fs::read_to_string(project.join("skills.lock.json")).unwrap();
    assert!(
        lock.contains("\"skills\": []"),
        "lockfile should have no skills after dropping orphans"
    );
}
