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

fn init_remote_with_skill(
    root: &Path,
    repo_name: &str,
    skill_subdir: &str,
    skill_name: &str,
) -> (PathBuf, PathBuf) {
    let bare = root.join("remotes").join(format!("{repo_name}.git"));
    fs::create_dir_all(&bare).unwrap();
    git(&["init", "--bare", "-b", "main"], &bare);

    let work = root.join("sources").join(repo_name);
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
    (bare, work)
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

fn read_lockfile(project: &Path) -> Json {
    let data = fs::read(project.join("skills.lock.json")).expect("lockfile present");
    serde_json::from_slice(&data).unwrap()
}

fn install_sample_skill(project: &Path, cache_dir: &Path, bare: &Path) {
    let file_url = path_to_file_url(bare);
    let mut install = cargo_bin_cmd!("sk");
    let out = install
        .current_dir(project)
        .env("SK_CACHE_DIR", cache_dir.to_str().unwrap())
        .args(["install", &file_url, "sample", "--path", "skill"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk install failed: {out:?}");
}

#[test]
fn remove_deletes_clean_install() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_dir = root.join("cache");
    let (bare, _work) = init_remote_with_skill(root, "repo-clean", "skill", "sample");

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    install_sample_skill(&project, &cache_dir, &bare);
    let skill_dir = project.join("skills").join("sample");
    assert!(skill_dir.exists(), "skill dir created");

    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_dir.to_str().unwrap())
        .args(["remove", "sample"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk remove failed: {out:?}");

    assert!(
        !skill_dir.exists(),
        "skill directory should be deleted after remove"
    );
    let lock = read_lockfile(&project);
    assert!(
        lock["skills"].as_array().unwrap().is_empty(),
        "lockfile entry dropped"
    );
}

#[test]
fn remove_refuses_dirty_install_without_force() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_dir = root.join("cache");
    let (bare, _work) = init_remote_with_skill(root, "repo-dirty", "skill", "sample");

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    install_sample_skill(&project, &cache_dir, &bare);
    let skill_dir = project.join("skills").join("sample");
    fs::write(skill_dir.join("file.txt"), "local edit\n").unwrap();

    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_dir.to_str().unwrap())
        .args(["remove", "sample"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "remove should fail when install is modified"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Refusing to remove"),
        "stderr should mention refusal: {stderr}"
    );
    assert!(skill_dir.exists(), "dir left intact on failure");
    let lock = read_lockfile(&project);
    assert_eq!(lock["skills"].as_array().unwrap().len(), 1);
}

#[test]
fn remove_force_allows_dirty_install() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_dir = root.join("cache");
    let (bare, _work) = init_remote_with_skill(root, "repo-force", "skill", "sample");

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    install_sample_skill(&project, &cache_dir, &bare);
    let skill_dir = project.join("skills").join("sample");
    fs::write(skill_dir.join("file.txt"), "local edit\n").unwrap();

    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_dir.to_str().unwrap())
        .args(["remove", "sample", "--force"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "forced remove should succeed: {out:?}"
    );

    assert!(
        !skill_dir.exists(),
        "skill directory deleted even when dirty with --force"
    );
    let lock = read_lockfile(&project);
    assert!(
        lock["skills"].as_array().unwrap().is_empty(),
        "lockfile entry dropped after forced remove"
    );
}
