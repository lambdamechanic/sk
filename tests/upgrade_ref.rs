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

fn write(path: &Path, contents: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, contents).unwrap();
}

#[test]
fn upgrade_ref_override_persists_without_commit_change() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_root = root.join("cache");
    let remotes_root = root.join("remotes_root");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Create a repo with v1 and a second branch pointing to same commit
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
    write(
        &work.join("skill").join("SKILL.md"),
        "---\nname: s0\ndescription: test\n---\n",
    );
    write(&work.join("skill").join("file.txt"), "v1\n");
    git(&["add", "."], &work);
    git(&["commit", "-m", "v1"], &work);
    git(&["branch", "new-branch"], &work);
    git(&["push", "-u", "origin", "main"], &work);
    git(&["push", "origin", "new-branch"], &work);
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

    // Cache clone
    let cache = cache_root.join("repos/local/o/r0");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    git(
        &["clone", bare.to_str().unwrap(), cache.to_str().unwrap()],
        cache.parent().unwrap(),
    );
    git(&["remote", "set-head", "origin", "-a"], &cache);

    // Install v1
    let dest = project.join("skills").join("s0");
    fs::create_dir_all(&dest).unwrap();
    // Extract via git archive | tar
    let strip = "1"; // strip 'skill'
    let mut archive = Command::new("git")
        .args([
            "-C",
            cache.to_str().unwrap(),
            "archive",
            "--format=tar",
            &v1,
            "skill",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let status = Command::new("tar")
        .args([
            "-x",
            "--strip-components",
            strip,
            "-C",
            dest.to_str().unwrap(),
        ])
        .stdin(archive.stdout.take().unwrap())
        .status()
        .unwrap();
    assert!(status.success());
    let _ = archive.wait();
    let digest_v1 = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut files: Vec<_> = walkdir::WalkDir::new(&dest)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .collect();
        files.sort();
        for p in files {
            let rel = p.strip_prefix(&dest).unwrap();
            hasher.update(rel.to_string_lossy().as_bytes());
            hasher.update(fs::read(&p).unwrap());
        }
        format!("sha256:{:x}", hasher.finalize())
    };
    let lock = serde_json::json!({
        "version":1,
        "skills":[{"installName":"s0","source":{"url":"file://dummy","host":"local","owner":"o","repo":"r0","skillPath":"skill"},"ref": null,"commit": v1,"digest": digest_v1,"installedAt":"1970-01-01T00:00:00Z"}],
        "generatedAt":"1970-01-01T00:00:00Z"
    });
    fs::write(
        project.join("skills.lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();

    // Run upgrade with a ref override that points to same commit
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .args(["upgrade", "s0", "--ref", "new-branch"])
        .output()
        .unwrap();
    assert!(out.status.success(), "upgrade failed: {out:?}");

    // Lockfile should record ref=new-branch even if commit unchanged
    let new_lock: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join("skills.lock.json")).unwrap())
            .unwrap();
    let ref_field = new_lock["skills"][0]["ref"].as_str().unwrap();
    assert_eq!(ref_field, "new-branch");
}
