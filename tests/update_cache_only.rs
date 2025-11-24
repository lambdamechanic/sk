use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

#[path = "support/mod.rs"]
mod support;

use support::{cache_repo_path, git, path_to_file_url};

fn write(path: &Path, contents: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn init_bare_and_work_with_v1(
    root: &Path,
    name: &str,
    skill_path: &str,
) -> (PathBuf, PathBuf, String) {
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
    (bare, work, v1)
}

fn run_cache_refresh(project: &Path, cache_root: &Path) {
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .args(["cache", "refresh"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk cache refresh failed: {out:?}");
}

fn origin_head(cache_repo: &Path) -> String {
    let out = Command::new("git")
        .args([
            "-C",
            cache_repo.to_str().unwrap(),
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git symbolic-ref failed: {:?}",
        out.stderr
    );
    let s = String::from_utf8(out.stdout).unwrap();
    s.trim().rsplit('/').next().unwrap().to_string()
}

#[test]
fn update_is_cache_only_and_fetches() {
    let root = tempdir().unwrap();
    let root = root.path();

    // Prepare bare remote and worktree with v1 committed
    let (bare, work, v1) = init_bare_and_work_with_v1(root, "skill1", "skill");

    // Prepare project with lockfile referencing this repo
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Pre-clone cache at v1 so it becomes stale after we push v2
    let cache_root = root.join("cache");
    let url1 = path_to_file_url(&bare);
    let cache_repo = cache_repo_path(&cache_root, "local", "o", "r1", &url1);
    fs::create_dir_all(cache_repo.parent().unwrap()).unwrap();
    git(
        &[
            "clone",
            bare.to_str().unwrap(),
            cache_repo.to_str().unwrap(),
        ],
        cache_repo.parent().unwrap(),
    );
    // Ensure origin/HEAD set for default-branch detection
    git(&["remote", "set-head", "origin", "-a"], &cache_repo);

    // Advance remote to v2 after the cache clone exists (cache is stale)
    fs::OpenOptions::new()
        .append(true)
        .open(work.join("skill").join("file.txt"))
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

    // Build lockfile (value not used by update semantics)
    let lock = json!({
        "version": 1,
        "skills": [
            {
                "installName": "s1",
                "source": {
                    "url": url1,
                    "host": "local",
                    "owner": "o",
                    "repo": "r1",
                    "skillPath": "skill"
                },
                "commit": v1,
                "digest": "sha256:deadbeef",
                "installedAt": "1970-01-01T00:00:00Z"
            }
        ],
        "generatedAt": "1970-01-01T00:00:00Z"
    });
    let lock_path = project.join("skills.lock.json");
    fs::write(&lock_path, serde_json::to_string_pretty(&lock).unwrap()).unwrap();
    let before = fs::read_to_string(&lock_path).unwrap();

    // Run cache refresh; it should only touch cache, not the project
    run_cache_refresh(&project, &cache_root);

    // Project lockfile unchanged
    let after = fs::read_to_string(&lock_path).unwrap();
    assert_eq!(before, after, "update must not modify project lockfile");

    // Cache has fetched the latest commit on origin/main
    let tip = String::from_utf8(
        Command::new("git")
            .args([
                "-C",
                cache_repo.to_str().unwrap(),
                "rev-parse",
                "refs/remotes/origin/main",
            ])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let tip = tip.trim();
    assert_eq!(tip, v2, "cache should fetch latest origin/main");
}

#[test]
fn update_refreshes_default_branch_head() {
    let root = tempdir().unwrap();
    let root = root.path();

    let (bare, work, _v1) = init_bare_and_work_with_v1(root, "skill-head", "skill");

    let project = root.join("project-head");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let cache_root = root.join("cache-head");
    let url = path_to_file_url(&bare);
    let lock = json!({
        "version": 1,
        "skills": [
            {
                "installName": "shead",
                "source": {
                    "url": url,
                    "host": "local",
                    "owner": "o",
                    "repo": "rhead",
                    "skillPath": "skill"
                },
                "commit": "deadbeef",
                "digest": "sha256:1",
                "installedAt": "1970-01-01T00:00:00Z"
            }
        ],
        "generatedAt": "1970-01-01T00:00:00Z"
    });
    fs::write(
        project.join("skills.lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();

    run_cache_refresh(&project, &cache_root);

    let cache_repo = cache_repo_path(&cache_root, "local", "o", "rhead", &url);
    assert!(
        cache_repo.exists(),
        "cache repo missing at {}",
        cache_repo.display()
    );
    assert_eq!(origin_head(&cache_repo), "main");

    // Change remote default branch to trunk and push new commit
    git(&["checkout", "-b", "trunk"], &work);
    write(&work.join("skill").join("file.txt"), "trunk\n");
    git(&["add", "."], &work);
    git(&["commit", "-m", "trunk"], &work);
    git(&["push", "origin", "trunk"], &work);
    git(&["symbolic-ref", "HEAD", "refs/heads/trunk"], &bare);

    run_cache_refresh(&project, &cache_root);

    assert_eq!(origin_head(&cache_repo), "trunk");
}

#[test]
fn cache_refresh_clones_missing_cache_once() {
    let root = tempdir().unwrap();
    let root = root.path();

    // remote with a single commit
    let (bare, _work, head) = init_bare_and_work_with_v1(root, "missing-cache", "skill");

    // project referencing the remote
    let project = root.join("project-missing-cache");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let cache_root = root.join("cache-missing");
    let url = path_to_file_url(&bare);
    let lock = json!({
        "version": 1,
        "skills": [
            {
                "installName": "missing",
                "source": {
                    "url": url,
                    "host": "local",
                    "owner": "o",
                    "repo": "missing",
                    "skillPath": "skill"
                },
                "commit": head,
                "digest": "sha256:1",
                "installedAt": "1970-01-01T00:00:00Z"
            }
        ],
        "generatedAt": "1970-01-01T00:00:00Z"
    });
    fs::write(
        project.join("skills.lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();

    // No cache exists yet, so refresh should clone once and fetch
    run_cache_refresh(&project, &cache_root);

    let cache_repo = cache_repo_path(&cache_root, "local", "o", "missing", &url);
    assert!(cache_repo.exists(), "cache clone should be created");
    let cached_head = String::from_utf8(
        Command::new("git")
            .args(["-C", cache_repo.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(cached_head.trim(), head);
}
