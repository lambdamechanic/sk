use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as Json;
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
    assert!(
        status.success(),
        "git {:?} failed in {}",
        args,
        cwd.display()
    );
}

fn rev_parse_head(repo: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git rev-parse failed in {}",
        repo.display()
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn init_remote_with_skill(
    root: &Path,
    repo_name: &str,
    skill_subdir: &str,
    skill_name: &str,
) -> (PathBuf, PathBuf, String) {
    let bare = root.join("remotes").join(format!("{repo_name}.git"));
    fs::create_dir_all(&bare).unwrap();
    git(&["init", "--bare", "-b", "main"], &bare);

    let work = root.join("work").join(repo_name);
    fs::create_dir_all(&work).unwrap();
    git(&["init", "-b", "main"], &work);
    git(&["remote", "add", "origin", bare.to_str().unwrap()], &work);
    git(&["config", "user.email", "test@example.com"], &work);
    git(&["config", "user.name", "Test User"], &work);
    git(&["config", "commit.gpgSign", "false"], &work);

    let skill_dir = work.join(skill_subdir);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {skill_name}\ndescription: test\n---\n"),
    )
    .unwrap();
    fs::write(skill_dir.join("file.txt"), "v1\n").unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-m", "v1"], &work);
    git(&["push", "-u", "origin", "main"], &work);
    let v1 = rev_parse_head(&work);
    (bare, work, v1)
}

fn path_to_file_url(p: &Path) -> String {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy().replace('\\', "/");
        if s.len() >= 2 && s.as_bytes()[1] == b':' {
            return format!("file:///{s}");
        }
        if s.starts_with("//") {
            return format!("file:{s}");
        }
        if s.starts_with('/') {
            return format!("file://{s}");
        }
        format!("file:///{s}")
    }
    #[cfg(not(windows))]
    {
        format!("file://{}", p.to_string_lossy())
    }
}

fn status_json(project: &Path, cache_override: &Path) -> Json {
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(project)
        .env("SK_CACHE_DIR", cache_override.to_str().unwrap())
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk status failed: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn status_reports_modified_after_local_edit() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_override = root.join("cache");

    let (bare, _work, _v1) = init_remote_with_skill(root, "r1", "skill", "sfile");

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let file_url = path_to_file_url(&bare);
    let mut install = cargo_bin_cmd!("sk");
    let out = install
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_override.to_str().unwrap())
        .args(["install", &file_url, "sfile", "--path", "skill"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk install failed: {out:?}");

    let status_clean = status_json(&project, &cache_override);
    assert_eq!(status_clean.as_array().unwrap().len(), 1);
    assert_eq!(
        status_clean[0]["state"].as_str().unwrap(),
        "clean",
        "fresh install should be clean"
    );
    assert!(status_clean[0]["update"].is_null());
    let locked = status_clean[0]["locked"].as_str().unwrap().to_string();
    let current = status_clean[0]["current"].as_str().unwrap().to_string();
    assert_eq!(locked, current);

    // Local edit flips status to modified
    fs::write(
        project.join("skills").join("sfile").join("file.txt"),
        "local edit\n",
    )
    .unwrap();

    let status_modified = status_json(&project, &cache_override);
    assert_eq!(status_modified.as_array().unwrap().len(), 1);
    assert_eq!(
        status_modified[0]["state"].as_str().unwrap(),
        "modified",
        "local edits must be reported"
    );
    let modified_digest = status_modified[0]["current"]
        .as_str()
        .expect("digest present for modified skill");
    assert_ne!(modified_digest, locked);
}

#[test]
fn status_reports_remote_update_after_cache_fetch() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_override = root.join("cache");

    let (bare, work, v1) = init_remote_with_skill(root, "r2", "skill", "sfile");

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let file_url = path_to_file_url(&bare);
    let mut install = cargo_bin_cmd!("sk");
    let out = install
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_override.to_str().unwrap())
        .args(["install", &file_url, "sfile", "--path", "skill"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk install failed: {out:?}");

    let first_status = status_json(&project, &cache_override);
    assert_eq!(first_status[0]["state"].as_str().unwrap(), "clean");
    assert!(first_status[0]["update"].is_null());

    // Advance remote repository to v2 and push
    fs::write(work.join("skill").join("file.txt"), "v2\n").unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-m", "v2"], &work);
    git(&["push", "origin", "main"], &work);
    let v2 = rev_parse_head(&work);

    let mut update = cargo_bin_cmd!("sk");
    let out = update
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_override.to_str().unwrap())
        .args(["update"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk update failed: {out:?}");

    let status_after_update = status_json(&project, &cache_override);
    assert_eq!(
        status_after_update[0]["state"].as_str().unwrap(),
        "clean",
        "local tree still matches lock digest"
    );
    let update_str = status_after_update[0]["update"]
        .as_str()
        .expect("update string present after cache fetch");
    let expected = format!("{} -> {}", &v1[..7], &v2[..7]);
    assert_eq!(update_str, expected);
}
