use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as Json;
use std::fs;
use std::path::Path;
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

#[test]
fn install_from_file_url_writes_lock_and_files() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    // Create a bare remote and a work repo with a single skill named sfile under skill/
    let bare = root.join("remotes").join("r.git");
    fs::create_dir_all(&bare).unwrap();
    git(&["init", "--bare", "-b", "main"], &bare);

    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    git(&["init", "-b", "main"], &work);
    git(&["remote", "add", "origin", bare.to_str().unwrap()], &work);
    git(&["config", "user.email", "test@example.com"], &work);
    git(&["config", "user.name", "Test User"], &work);
    git(&["config", "commit.gpgSign", "false"], &work);
    fs::create_dir_all(work.join("skill")).unwrap();
    fs::write(
        work.join("skill/SKILL.md"),
        "---\nname: sfile\ndescription: test\n---\n",
    )
    .unwrap();
    fs::write(work.join("skill/file.txt"), "v1\n").unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-m", "v1"], &work);
    git(&["push", "-u", "origin", "main"], &work);
    // Capture HEAD commit
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&work)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let head = head.trim().to_string();

    // Init a project to install into
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Run `sk install file:///... sfile --path skill` using a temp cache root
    let file_url = format!("file://{}", bare.to_string_lossy());
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", root.join("cache").to_str().unwrap())
        .args(["install", &file_url, "sfile", "--path", "skill"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk install failed: {out:?}");

    // Verify installed directory exists
    assert!(project.join("skills/sfile/SKILL.md").exists());

    // Verify lockfile contents
    let lock: Json =
        serde_json::from_str(&fs::read_to_string(project.join("skills.lock.json")).unwrap())
            .unwrap();
    let skills = lock["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 1);
    let entry = &skills[0];
    assert_eq!(entry["installName"].as_str().unwrap(), "sfile");
    assert_eq!(entry["source"]["url"].as_str().unwrap(), &file_url);
    assert_eq!(entry["source"]["repo"].as_str().unwrap(), "r");
    assert_eq!(entry["source"]["host"].as_str().unwrap(), "local");
    assert_eq!(entry["source"]["owner"].as_str().unwrap(), "remotes");
    // Commit pinned should be the work HEAD
    assert_eq!(entry["commit"].as_str().unwrap(), &head);

    // sk check should report ok
    let mut chk = cargo_bin_cmd!("sk");
    let out = chk
        .current_dir(&project)
        .args(["check", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let arr: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(arr[0]["state"].as_str().unwrap(), "ok");
}
