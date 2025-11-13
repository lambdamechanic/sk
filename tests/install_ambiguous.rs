use assert_cmd::cargo::cargo_bin_cmd;
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

#[test]
fn install_requires_path_when_names_conflict() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    // Create a bare remote and a work repo with two skills that share the same name
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

    // Two different subdirs, same skill name "dupe"
    fs::create_dir_all(work.join("skills/a")).unwrap();
    fs::write(
        work.join("skills/a/SKILL.md"),
        "---\nname: dupe\ndescription: a\n---\n",
    )
    .unwrap();
    fs::create_dir_all(work.join("skills/b")).unwrap();
    fs::write(
        work.join("skills/b/SKILL.md"),
        "---\nname: dupe\ndescription: b\n---\n",
    )
    .unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-m", "v1"], &work);
    git(&["push", "-u", "origin", "main"], &work);

    // Init a project to install into
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let file_url = path_to_file_url(&bare);

    // Attempt install without --path should fail with helpful error listing candidates
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", root.join("cache").to_str().unwrap())
        .args(["install", &file_url, "dupe"]) // no --path
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "install unexpectedly succeeded: {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Multiple skills named 'dupe'"),
        "stderr missing conflict preface: {}",
        stderr
    );
    assert!(
        stderr.contains("skills/a") && stderr.contains("skills/b"),
        "stderr should list both candidates: {}",
        stderr
    );
    assert!(
        stderr.contains("found in remotes/r"),
        "stderr should mention repo identifier: {}",
        stderr
    );

    // Disambiguate with --path; should succeed
    let mut cmd2 = cargo_bin_cmd!("sk");
    let out2 = cmd2
        .current_dir(&project)
        .env("SK_CACHE_DIR", root.join("cache").to_str().unwrap())
        .args(["install", &file_url, "dupe", "--path", "skills/b"])
        .output()
        .unwrap();
    assert!(
        out2.status.success(),
        "install with --path failed: {out2:?}"
    );
    assert!(project.join("skills/dupe/SKILL.md").exists());
}

#[test]
fn install_reports_missing_skill_md_for_path() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

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

    fs::create_dir_all(work.join("skills/a")).unwrap();
    fs::write(
        work.join("skills/a/SKILL.md"),
        "---\nname: sfile\ndescription: test\n---\n",
    )
    .unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-m", "v1"], &work);
    git(&["push", "-u", "origin", "main"], &work);

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let file_url = path_to_file_url(&bare);
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", root.join("cache").to_str().unwrap())
        .args(["install", &file_url, "sfile", "--path", "skills/missing"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "install unexpectedly succeeded: {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("'skills/missing/SKILL.md' not found or invalid"),
        "stderr missing not-found hint: {}",
        stderr
    );
}

#[test]
fn install_reports_invalid_skill_md_for_path() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

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

    fs::create_dir_all(work.join("skills/a")).unwrap();
    fs::write(
        work.join("skills/a/SKILL.md"),
        "---\nname: sfile\ndescription: test\n---\n",
    )
    .unwrap();
    fs::create_dir_all(work.join("skills/bad")).unwrap();
    fs::write(work.join("skills/bad/SKILL.md"), "no frontmatter here\n").unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-m", "v1"], &work);
    git(&["push", "-u", "origin", "main"], &work);

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let file_url = path_to_file_url(&bare);
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", root.join("cache").to_str().unwrap())
        .args(["install", &file_url, "sfile", "--path", "skills/bad"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "install unexpectedly succeeded: {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("'skills/bad/SKILL.md' not found or invalid"),
        "stderr missing invalid hint: {}",
        stderr
    );
}
