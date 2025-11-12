use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::json;
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

fn write(path: &Path, contents: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, contents).unwrap();
}

#[test]
fn file_url_caches_do_not_collide() {
    let root = tempdir().unwrap();
    let root = root.path();

    // Create two bare repos with the same owner/repo leaf (o/r.git) but different absolute paths
    let bare1 = root.join("remotes1").join("o").join("r.git");
    fs::create_dir_all(&bare1).unwrap();
    git(&["init", "--bare", "-b", "main"], &bare1);

    let work1 = root.join("work1");
    fs::create_dir_all(&work1).unwrap();
    git(&["init", "-b", "main"], &work1);
    git(
        &["remote", "add", "origin", bare1.to_str().unwrap()],
        &work1,
    );
    git(&["config", "user.email", "test@example.com"], &work1);
    git(&["config", "user.name", "Test User"], &work1);
    write(
        &work1.join("skill").join("SKILL.md"),
        "---\nname: s\ndescription: t\n---\n",
    );
    write(&work1.join("skill").join("file.txt"), "v1\n");
    git(&["add", "."], &work1);
    git(&["commit", "-m", "v1"], &work1);
    git(&["push", "-u", "origin", "main"], &work1);

    let bare2 = root.join("remotes2").join("o").join("r.git");
    fs::create_dir_all(&bare2).unwrap();
    git(&["init", "--bare", "-b", "main"], &bare2);
    let work2 = root.join("work2");
    fs::create_dir_all(&work2).unwrap();
    git(&["init", "-b", "main"], &work2);
    git(
        &["remote", "add", "origin", bare2.to_str().unwrap()],
        &work2,
    );
    git(&["config", "user.email", "test@example.com"], &work2);
    git(&["config", "user.name", "Test User"], &work2);
    write(
        &work2.join("skill").join("SKILL.md"),
        "---\nname: s\ndescription: t\n---\n",
    );
    write(&work2.join("skill").join("file.txt"), "v1\n");
    git(&["add", "."], &work2);
    git(&["commit", "-m", "v1"], &work2);
    git(&["push", "-u", "origin", "main"], &work2);

    // Build lockfile with both file URLs
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

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let lock = json!({
        "version": 1,
        "skills": [
            {
                "installName": "s1",
                "source": { "url": path_to_file_url(&bare1), "host": "local", "owner": "o", "repo": "r", "skillPath": "skill" },
                "ref": null, "commit": "deadbeef", "digest": "sha256:1", "installedAt": "1970-01-01T00:00:00Z"
            },
            {
                "installName": "s2",
                "source": { "url": path_to_file_url(&bare2), "host": "local", "owner": "o", "repo": "r", "skillPath": "skill" },
                "ref": null, "commit": "deadbeef", "digest": "sha256:2", "installedAt": "1970-01-01T00:00:00Z"
            }
        ],
        "generatedAt": "1970-01-01T00:00:00Z"
    });
    fs::write(
        project.join("skills.lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();

    let cache_root = root.join("cache");
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .args(["update"]) // ensures caches exist
        .output()
        .unwrap();
    assert!(out.status.success(), "sk update failed: {out:?}");

    // Expect two distinct cache directories under local/o/
    let o_dir = cache_root.join("repos/local/o");
    let children = fs::read_dir(&o_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    // Should not be just ["r"], instead two hashed leaves starting with "r-"
    assert!(
        children.iter().all(|n| n.starts_with("r-")),
        "unexpected children: {children:?}"
    );
    assert!(
        children.len() >= 2,
        "expected at least two distinct caches: {children:?}"
    );
}
