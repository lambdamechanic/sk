use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value as Json;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn git(args: &[&str], cwd: &Path) {
    let status = Command::new("git").args(args).current_dir(cwd).status().unwrap();
    assert!(status.success(), "git {:?} failed in {}", args, cwd.display());
}

#[test]
fn check_reports_modified_after_local_edit() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    // Bare remote and work repo with one skill
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

    // Init a project and install the skill
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);
    let file_url = format!("file://{}", bare.to_string_lossy());
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", root.join("cache").to_str().unwrap())
        .args(["install", &file_url, "sfile", "--path", "skill"])
        .output()
        .unwrap();
    assert!(out.status.success(), "sk install failed: {out:?}");

    // First check is ok
    let mut chk1 = cargo_bin_cmd!("sk");
    let out1 = chk1
        .current_dir(&project)
        .args(["check", "--json"])
        .output()
        .unwrap();
    assert!(out1.status.success());
    let arr1: Json = serde_json::from_slice(&out1.stdout).unwrap();
    assert_eq!(arr1[0]["state"].as_str().unwrap(), "ok");

    // Edit the installed file
    fs::write(project.join("skills/sfile/file.txt"), "v1 local edit\n").unwrap();

    // Now check should report modified
    let mut chk2 = cargo_bin_cmd!("sk");
    let out2 = chk2
        .current_dir(&project)
        .args(["check", "--json"])
        .output()
        .unwrap();
    assert!(out2.status.success());
    let arr2: Json = serde_json::from_slice(&out2.stdout).unwrap();
    assert_eq!(arr2[0]["state"].as_str().unwrap(), "modified");
}

