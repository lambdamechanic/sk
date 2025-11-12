use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

fn git(args: &[&str], cwd: &Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "git {:?} failed in {}",
        args,
        cwd.display()
    );
}

fn write(path: &Path, contents: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, contents).unwrap();
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
fn update_is_cache_only_and_fetches() {
    let root = tempdir().unwrap();
    let root = root.path();

    // Prepare bare remote with two commits
    let (bare, _v1, v2) = init_skill_repo(root, "skill1", "skill");

    // Prepare project with lockfile referencing this repo
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let lock = json!({
        "version": 1,
        "skills": [
            {
                "installName": "s1",
                "source": {
                    "url": format!("file:///{}", bare.to_string_lossy()),
                    "host": "local",
                    "owner": "o",
                    "repo": "r1",
                    "skillPath": "skill"
                },
                "ref": null,
                "commit": v2, // value not used by update
                "digest": "sha256:deadbeef",
                "installedAt": "1970-01-01T00:00:00Z"
            }
        ],
        "generatedAt": "1970-01-01T00:00:00Z"
    });
    let lock_path = project.join("skills.lock.json");
    fs::write(&lock_path, serde_json::to_string_pretty(&lock).unwrap()).unwrap();
    let before = fs::read_to_string(&lock_path).unwrap();

    // Pre-clone cache at older state to ensure fetch path is exercised
    let cache_root = root.join("cache");
    let cache_repo = cache_root.join("repos/local/o/r1");
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

    // Run update; it should only touch cache, not the project
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .args(["update"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk update failed: {out:?}");

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
