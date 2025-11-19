use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as Json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

#[path = "support/mod.rs"]
mod support;

use support::{clone_into_cache, extract_subdir_from_commit, git, path_to_file_url, CacheRepoSpec};

fn write(path: &Path, contents: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn digest_dir(dir: &Path) -> String {
    sk::digest::digest_dir(dir).expect("compute digest")
}

fn init_skill_repo(root: &Path, name: &str, skill_path: &str) -> (PathBuf, String, String) {
    let bare = root.join("remotes").join(format!("{name}.git"));
    fs::create_dir_all(&bare).unwrap();
    git(&["init", "--bare", "-b", "main"], &bare);

    let work = root.join("sources").join(name);
    fs::create_dir_all(&work).unwrap();
    git(&["init", "-b", "main"], &work);
    git(&["remote", "add", "origin", bare.to_str().unwrap()], &work);
    git(&["config", "user.email", "test@example.com"], &work);
    git(&["config", "user.name", "Test User"], &work);
    git(&["config", "commit.gpgSign", "false"], &work);

    // v1
    write(
        &work.join(skill_path).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: test\n---\n"),
    );
    write(&work.join(skill_path).join("file.txt"), "v1\n");
    git(&["add", "."], &work);
    git(&["commit", "-m", "v1"], &work);
    git(&["push", "-u", "origin", "main"], &work);
    let v1 = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&work)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let v1 = v1.trim().to_string();

    // v2 (touch file)
    fs::OpenOptions::new()
        .append(true)
        .open(work.join(skill_path).join("file.txt"))
        .unwrap()
        .write_all(b"v2\n")
        .unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-m", "v2"], &work);
    git(&["push", "origin", "main"], &work);
    let v2 = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&work)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let v2 = v2.trim().to_string();
    (bare, v1, v2)
}

#[test]
fn upgrade_fetches_cache_and_applies_without_update() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_root = root.join("cache");
    let remotes_root = root.join("remotes_root");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let host = "local";
    let owner = "o";
    let repo = "r0";
    let skill_path = "skill-0";
    let (bare, v1, v2) = init_skill_repo(&remotes_root, repo, skill_path);
    let file_url = path_to_file_url(&bare);
    let cache = clone_into_cache(
        &cache_root,
        CacheRepoSpec {
            host,
            owner,
            name: repo,
            url_for_lock: &file_url,
        },
        &bare,
    );

    // Install v1
    let dest = project.join("skills").join("s0");
    extract_subdir_from_commit(&cache, &v1, skill_path, &dest);
    let digest_v1 = digest_dir(&dest);
    let lock = serde_json::json!({
        "version":1,
        "skills":[{
            "installName":"s0",
            "source": {"url":file_url,"host":host,"owner":owner,"repo":repo,"skillPath":skill_path},
            "commit": v1,
            "digest": digest_v1,
            "installedAt":"1970-01-01T00:00:00Z"
        }],
        "generatedAt":"1970-01-01T00:00:00Z"
    });
    write(
        &project.join("skills.lock.json"),
        &serde_json::to_string_pretty(&lock).unwrap(),
    );

    // Advance remote to v2 (already done in init); do NOT run `sk update`

    // Run upgrade: should fetch and apply
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .args(["upgrade", "--all"]) // should succeed
        .output()
        .unwrap();
    assert!(out.status.success(), "upgrade failed: {out:?}");

    // Assert lockfile now points to v2 and digest changed
    let new_lock: Json =
        serde_json::from_str(&fs::read_to_string(project.join("skills.lock.json")).unwrap())
            .unwrap();
    let new_commit = new_lock["skills"][0]["commit"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(new_commit, v2);
    let new_digest = new_lock["skills"][0]["digest"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(new_digest, digest_v1);
}

#[test]
fn upgrade_refreshes_lock_when_local_matches_remote() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_root = root.join("cache");
    let remotes_root = root.join("remotes_root");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let host = "local";
    let owner = "o";
    let repo = "r1";
    let skill_path = "skill-0";
    let (bare, v1, v2) = init_skill_repo(&remotes_root, repo, skill_path);
    let file_url = path_to_file_url(&bare);
    let cache = clone_into_cache(
        &cache_root,
        CacheRepoSpec {
            host,
            owner,
            name: repo,
            url_for_lock: &file_url,
        },
        &bare,
    );

    let dest = project.join("skills").join("s0");
    extract_subdir_from_commit(&cache, &v1, skill_path, &dest);
    let digest_v1 = digest_dir(&dest);
    let lock = serde_json::json!({
        "version":1,
        "skills":[{
            "installName":"s0",
            "source": {"url":file_url,"host":host,"owner":owner,"repo":repo,"skillPath":skill_path},
            "commit": v1,
            "digest": digest_v1,
            "installedAt":"1970-01-01T00:00:00Z"
        }],
        "generatedAt":"1970-01-01T00:00:00Z"
    });
    write(
        &project.join("skills.lock.json"),
        &serde_json::to_string_pretty(&lock).unwrap(),
    );

    fs::remove_dir_all(&dest).unwrap();
    extract_subdir_from_commit(&cache, &v2, skill_path, &dest);
    let digest_v2 = digest_dir(&dest);

    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .args(["upgrade", "s0"])
        .output()
        .unwrap();
    assert!(out.status.success(), "upgrade failed: {out:?}");

    let new_lock: Json =
        serde_json::from_str(&fs::read_to_string(project.join("skills.lock.json")).unwrap())
            .unwrap();
    let new_commit = new_lock["skills"][0]["commit"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(new_commit, v2);
    let new_digest = new_lock["skills"][0]["digest"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(new_digest, digest_v2);
}

#[test]
fn upgrade_handles_cross_device_rename_simulation() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_root = root.join("cache");
    let remotes_root = root.join("remotes_root");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let host = "local";
    let owner = "o";
    let repo = "r0";
    let skill_path = "skill-0";
    let (bare, v1, v2) = init_skill_repo(&remotes_root, repo, skill_path);
    let file_url = {
        #[cfg(windows)]
        {
            let s = bare.to_string_lossy().replace('\\', "/");
            if s.len() >= 2 && s.as_bytes()[1] == b':' {
                format!("file:///{s}")
            } else if s.starts_with("//") {
                format!("file:{s}")
            } else if s.starts_with('/') {
                format!("file://{s}")
            } else {
                format!("file:///{s}")
            }
        }
        #[cfg(not(windows))]
        {
            format!("file://{}", bare.to_string_lossy())
        }
    };
    let cache = clone_into_cache(
        &cache_root,
        CacheRepoSpec {
            host,
            owner,
            name: repo,
            url_for_lock: &file_url,
        },
        &bare,
    );

    // Install v1
    let dest = project.join("skills").join("s0");
    extract_subdir_from_commit(&cache, &v1, skill_path, &dest);
    let digest_v1 = digest_dir(&dest);
    let lock = serde_json::json!({
        "version":1,
        "skills":[{"installName":"s0","source": {"url":file_url,"host":host,"owner":owner,"repo":repo,"skillPath":skill_path},"commit": v1,"digest": digest_v1,"installedAt":"1970-01-01T00:00:00Z"}],
        "generatedAt":"1970-01-01T00:00:00Z"
    });
    write(
        &project.join("skills.lock.json"),
        &serde_json::to_string_pretty(&lock).unwrap(),
    );

    // Simulate cross-device rename by env flag
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .env("SK_SIMULATE_EXDEV", "1")
        .args(["upgrade", "--all"]) // should succeed using fallback copy
        .output()
        .unwrap();
    assert!(out.status.success(), "upgrade failed: {out:?}");

    // lockfile moved to v2
    let new_lock: Json =
        serde_json::from_str(&fs::read_to_string(project.join("skills.lock.json")).unwrap())
            .unwrap();
    let new_commit = new_lock["skills"][0]["commit"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(new_commit, v2);
}

#[test]
fn upgrade_does_not_mutate_on_extract_failure() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_root = root.join("cache");
    let remotes_root = root.join("remotes_root");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let host = "local";
    let owner = "o";

    // r0 has stable skill path
    let (bare0, v1_0, _v2_0) = init_skill_repo(&remotes_root, "r0", "skill-0");
    let file_url0 = path_to_file_url(&bare0);
    let cache0 = clone_into_cache(
        &cache_root,
        CacheRepoSpec {
            host,
            owner,
            name: "r0",
            url_for_lock: &file_url0,
        },
        &bare0,
    );
    // r1 removes the skill path in v2 to trigger extract failure
    let bare1 = remotes_root.join("removes").join("r1.git");
    fs::create_dir_all(&bare1).unwrap();
    git(&["init", "--bare", "-b", "main"], &bare1);
    let work1 = remotes_root.join("sources").join("r1");
    fs::create_dir_all(&work1).unwrap();
    git(&["init", "-b", "main"], &work1);
    git(
        &["remote", "add", "origin", bare1.to_str().unwrap()],
        &work1,
    );
    git(&["config", "user.email", "test@example.com"], &work1);
    git(&["config", "user.name", "Test User"], &work1);
    git(&["config", "commit.gpgSign", "false"], &work1);
    write(
        &work1.join("skill-1").join("SKILL.md"),
        "---\nname: s1\ndescription: test\n---\n",
    );
    write(&work1.join("skill-1").join("file.txt"), "v1\n");
    git(&["add", "."], &work1);
    git(&["commit", "-m", "v1"], &work1);
    git(&["push", "-u", "origin", "main"], &work1);
    // v2 removes the skill-1 directory
    git(&["rm", "-r", "skill-1"], &work1);
    git(&["commit", "-m", "remove skill"], &work1);
    git(&["push", "origin", "main"], &work1);
    let file_url1 = path_to_file_url(&bare1);
    let cache1 = clone_into_cache(
        &cache_root,
        CacheRepoSpec {
            host,
            owner,
            name: "r1",
            url_for_lock: &file_url1,
        },
        &bare1,
    );
    let v1_1 = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD~1"]) // the v1 commit
            .current_dir(&work1)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let v1_1 = v1_1.trim().to_string();

    // Install v1 contents
    let dest0 = project.join("skills").join("s0");
    extract_subdir_from_commit(&cache0, &v1_0, "skill-0", &dest0);
    let dig0 = digest_dir(&dest0);
    let dest1 = project.join("skills").join("s1");
    extract_subdir_from_commit(&cache1, &v1_1, "skill-1", &dest1);
    let dig1 = digest_dir(&dest1);

    // Lockfile
    let lock = serde_json::json!({
        "version":1,
        "skills":[
            {"installName":"s0","source": {"url":file_url0,"host":host,"owner":owner,"repo":"r0","skillPath":"skill-0"},"commit": v1_0,"digest": dig0,"installedAt":"1970-01-01T00:00:00Z"},
            {"installName":"s1","source": {"url":file_url1,"host":host,"owner":owner,"repo":"r1","skillPath":"skill-1"},"commit": v1_1,"digest": dig1,"installedAt":"1970-01-01T00:00:00Z"}
        ],
        "generatedAt":"1970-01-01T00:00:00Z"
    });
    write(
        &project.join("skills.lock.json"),
        &serde_json::to_string_pretty(&lock).unwrap(),
    );
    let pre_lock = fs::read_to_string(project.join("skills.lock.json")).unwrap();

    // Upgrade should fail due to extract error and must not mutate installs or lock
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .args(["upgrade", "--all"]) // expect failure
        .output()
        .unwrap();
    assert!(!out.status.success(), "upgrade unexpectedly succeeded");
    assert_eq!(
        pre_lock,
        fs::read_to_string(project.join("skills.lock.json")).unwrap()
    );
    assert_eq!(dig0, digest_dir(&dest0));
    assert_eq!(dig1, digest_dir(&dest1));
}
