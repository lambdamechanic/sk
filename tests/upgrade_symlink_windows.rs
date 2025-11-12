#![cfg(windows)]
use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use std::os::windows::fs as winfs;
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
fn upgrade_preserves_directory_symlink_windows() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_root = root.join("cache");
    let remotes_root = root.join("remotes_root");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Prepare repo with directory symlink
    let bare = remotes_root.join("remotes").join("r0.git");
    fs::create_dir_all(&bare).unwrap();
    git(&["init", "--bare", "-b", "main"], &bare);
    let work = remotes_root.join("sources").join("r0");
    fs::create_dir_all(&work).unwrap();
    git(&["init", "-b", "main"], &work);
    git(&["remote", "add", "origin", bare.to_str().unwrap()], &work);
    git(&["config", "user.email", "test@example.com"], &work);
    git(&["config", "user.name", "Test User"], &work);
    git(&["config", "commit.gpgSign", "false"], &work);
    git(&["config", "core.symlinks", "true"], &work);

    fs::create_dir_all(work.join("skill/dir")).unwrap();
    fs::write(work.join("skill/dir/file.txt"), "v1\n").unwrap();
    // Create directory symlink dir-link -> dir
    if winfs::symlink_dir("dir", work.join("skill/dir-link")).is_err() {
        // Symlinks not supported; skip
        return;
    }
    fs::write(
        work.join("skill/SKILL.md"),
        "---\nname: s0\ndescription: test\n---\n",
    )
    .unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-m", "v1"], &work);
    git(&["push", "-u", "origin", "main"], &work);
    // v2 updates file
    fs::write(work.join("skill/dir/file.txt"), "v2\n").unwrap();
    git(&["add", "."], &work);
    git(&["commit", "-m", "v2"], &work);
    git(&["push", "origin", "main"], &work);

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
    fn hashed_leaf(url: &str, repo: &str) -> String {
        use sha2::{Digest, Sha256};
        let h = Sha256::digest(url.as_bytes());
        let hex = format!("{h:x}");
        let short = &hex[..12];
        format!("{repo}-{short}")
    }
    let file_url = path_to_file_url(&bare);
    let cache = cache_root
        .join("repos/local/o")
        .join(hashed_leaf(&file_url, "r0"));
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    git(
        &["clone", bare.to_str().unwrap(), cache.to_str().unwrap()],
        cache.parent().unwrap(),
    );
    git(&["remote", "set-head", "origin", "-a"], &cache);

    // Install v1 via archive
    let dest = project.join("skills/s0");
    fs::create_dir_all(&dest).unwrap();
    let mut archive = Command::new("git")
        .args([
            "-C",
            cache.to_str().unwrap(),
            "archive",
            "--format=tar",
            "HEAD~1",
            "skill",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let status = Command::new("tar")
        .args([
            "-x",
            "--strip-components",
            "1",
            "-C",
            dest.to_str().unwrap(),
        ])
        .stdin(archive.stdout.take().unwrap())
        .status()
        .unwrap();
    assert!(status.success());
    let _ = archive.wait();

    // Build lock
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&cache)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let head = head.trim().to_string();
    let digest_v1 = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        let mut files: Vec<_> = walkdir::WalkDir::new(&dest)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .collect();
        files.sort();
        for p in files {
            let rel = p.strip_prefix(&dest).unwrap();
            h.update(rel.to_string_lossy().as_bytes());
            h.update(fs::read(&p).unwrap());
        }
        format!("sha256:{:x}", h.finalize())
    };
    let lock = serde_json::json!({"version":1,"skills":[{"installName":"s0","source":{"url":file_url,"host":"local","owner":"o","repo":"r0","skillPath":"skill"},"ref": null,"commit": head,"digest": digest_v1,"installedAt":"1970-01-01T00:00:00Z"}],"generatedAt":"1970-01-01T00:00:00Z"});
    fs::write(
        project.join("skills.lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();

    // Simulate EXDEV path to exercise copy fallback
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .env("SK_SIMULATE_EXDEV", "1")
        .args(["upgrade", "--all"])
        .output()
        .unwrap();
    assert!(out.status.success(), "upgrade failed: {out:?}");

    // dir-link should still be a symlink dir
    let link_path = project.join("skills/s0/dir-link");
    let meta = fs::symlink_metadata(&link_path).unwrap();
    assert!(meta.file_type().is_symlink());
}
