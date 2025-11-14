use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

#[path = "support/mod.rs"]
mod support;

use support::{git, hashed_leaf};

fn write(path: &Path, contents: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn digest_dir(dir: &Path) -> String {
    sk::digest::digest_dir(dir).expect("compute digest")
}

#[test]
fn upgrade_rolls_back_when_apply_fails_mid_loop() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_root = root.join("cache");
    let remotes_root = root.join("remotes_root");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    // Create two repos r0/r1 with v1->v2 upgrades
    for repo in ["r0", "r1"] {
        let bare = remotes_root.join("remotes").join(format!("{repo}.git"));
        fs::create_dir_all(&bare).unwrap();
        git(&["init", "--bare", "-b", "main"], &bare);
        let work = remotes_root.join("sources").join(repo);
        fs::create_dir_all(&work).unwrap();
        git(&["init", "-b", "main"], &work);
        git(&["remote", "add", "origin", bare.to_str().unwrap()], &work);
        git(&["config", "user.email", "test@example.com"], &work);
        git(&["config", "user.name", "Test User"], &work);
        git(&["config", "commit.gpgSign", "false"], &work);
        write(
            &work.join("skill").join("SKILL.md"),
            &format!("---\nname: s{repo}\ndescription: test\n---\n"),
        );
        write(&work.join("skill").join("file.txt"), "v1\n");
        git(&["add", "."], &work);
        git(&["commit", "-m", "v1"], &work);
        git(&["push", "-u", "origin", "main"], &work);
        // v2
        fs::OpenOptions::new()
            .append(true)
            .open(work.join("skill/file.txt"))
            .unwrap()
            .write_all(b"v2\n")
            .unwrap();
        git(&["add", "."], &work);
        git(&["commit", "-m", "v2"], &work);
        git(&["push", "origin", "main"], &work);
    }

    // Clone caches (hash-only leaf for file://dummy)
    let dummy = "file://dummy";
    for repo in ["r0", "r1"] {
        let bare = remotes_root.join("remotes").join(format!("{repo}.git"));
        let dest = cache_root
            .join("repos/local/o")
            .join(hashed_leaf(dummy, repo));
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        git(
            &["clone", bare.to_str().unwrap(), dest.to_str().unwrap()],
            dest.parent().unwrap(),
        );
        git(&["remote", "set-head", "origin", "-a"], &dest);
    }

    // Install v1 of each into project and build lockfile
    let mut skills = vec![];
    for (i, repo) in ["r0", "r1"].iter().enumerate() {
        let cache = cache_root
            .join("repos/local/o")
            .join(hashed_leaf(dummy, repo));
        let head_prev = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD~1"])
                .current_dir(&cache)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        let dest = project.join("skills").join(format!("s{i}"));
        fs::create_dir_all(&dest).unwrap();
        let mut arch = Command::new("git")
            .args([
                "-C",
                cache.to_str().unwrap(),
                "archive",
                "--format=tar",
                &head_prev,
                "skill",
            ])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let ok = Command::new("tar")
            .args([
                "-x",
                "--strip-components",
                "1",
                "-C",
                dest.to_str().unwrap(),
            ])
            .stdin(arch.stdout.take().unwrap())
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let _ = arch.wait();
        // digest
        let digest = digest_dir(&dest);
        skills.push((format!("s{i}"), repo.to_string(), head_prev, digest));
    }
    let lock = serde_json::json!({
        "version":1,
        "skills": skills.iter().map(|(name, repo, commit, digest)| serde_json::json!({
            "installName": name,
            "source": {"url":"file://dummy","host":"local","owner":"o","repo": repo, "skillPath":"skill"},
            "commit": commit,
            "digest": digest,
            "installedAt": "1970-01-01T00:00:00Z"
        })).collect::<Vec<_>>(),
        "generatedAt":"1970-01-01T00:00:00Z"
    });
    fs::write(
        project.join("skills.lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();
    let pre_lock = fs::read_to_string(project.join("skills.lock.json")).unwrap();
    let pre_digests: Vec<String> = skills
        .iter()
        .map(|(name, _, _, _)| {
            let dest = project.join("skills").join(name);
            digest_dir(&dest)
        })
        .collect();

    // Run upgrade with failure after first swap
    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .env("SK_FAIL_AFTER_FIRST_SWAP", "1")
        .args(["upgrade", "--all"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected failure: {out:?}");

    // Assert lockfile and on-disk digests unchanged (rollback applied)
    let post_lock = fs::read_to_string(project.join("skills.lock.json")).unwrap();
    assert_eq!(pre_lock, post_lock);

    let post_digests: Vec<String> = skills
        .iter()
        .map(|(name, _, _, _)| {
            let dest = project.join("skills").join(name);
            digest_dir(&dest)
        })
        .collect();
    assert_eq!(pre_digests, post_digests);
    assert_eq!(pre_digests, post_digests);
}

#[test]
fn upgrade_rolls_back_when_copy_fails_in_swap() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let cache_root = root.join("cache");
    let remotes_root = root.join("remotes_root");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-b", "main"], &project);

    let dummy = "file://dummy";
    for repo in ["r0", "r1"] {
        let bare = remotes_root.join("remotes").join(format!("{repo}.git"));
        fs::create_dir_all(&bare).unwrap();
        git(&["init", "--bare", "-b", "main"], &bare);
        let work = remotes_root.join("sources").join(repo);
        fs::create_dir_all(&work).unwrap();
        git(&["init", "-b", "main"], &work);
        git(&["remote", "add", "origin", bare.to_str().unwrap()], &work);
        git(&["config", "user.email", "test@example.com"], &work);
        git(&["config", "user.name", "Test User"], &work);
        git(&["config", "commit.gpgSign", "false"], &work);
        write(
            &work.join("skill/SKILL.md"),
            &format!("---\nname: s{repo}\ndescription: test\n---\n"),
        );
        write(&work.join("skill/file.txt"), "v1\n");
        git(&["add", "."], &work);
        git(&["commit", "-m", "v1"], &work);
        git(&["push", "-u", "origin", "main"], &work);
        fs::OpenOptions::new()
            .append(true)
            .open(work.join("skill/file.txt"))
            .unwrap()
            .write_all(b"v2\n")
            .unwrap();
        git(&["add", "."], &work);
        git(&["commit", "-m", "v2"], &work);
        git(&["push", "origin", "main"], &work);
        let dest = cache_root
            .join("repos/local/o")
            .join(hashed_leaf(dummy, repo));
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        git(
            &["clone", bare.to_str().unwrap(), dest.to_str().unwrap()],
            dest.parent().unwrap(),
        );
        git(&["remote", "set-head", "origin", "-a"], &dest);
    }

    // Install v1 for both and build lockfile
    let mut entries = vec![];
    for (i, repo) in ["r0", "r1"].iter().enumerate() {
        let cache = cache_root
            .join("repos/local/o")
            .join(hashed_leaf(dummy, repo));
        let v1 = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD~1"])
                .current_dir(&cache)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let v1 = v1.trim().to_string();
        let dest = project.join("skills").join(format!("s{i}"));
        fs::create_dir_all(&dest).unwrap();
        let mut arch = Command::new("git")
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
        let ok = Command::new("tar")
            .args([
                "-x",
                "--strip-components",
                "1",
                "-C",
                dest.to_str().unwrap(),
            ])
            .stdin(arch.stdout.take().unwrap())
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let _ = arch.wait();
        let digest = digest_dir(&dest);
        entries.push((format!("s{i}"), repo.to_string(), v1, digest));
    }
    let lock = serde_json::json!({
        "version":1,
        "skills": entries.iter().map(|(name, repo, commit, digest)| serde_json::json!({
            "installName": name,
            "source": {"url":"file://dummy","host":"local","owner":"o","repo": repo, "skillPath":"skill"},
            "commit": commit,
            "digest": digest,
            "installedAt": "1970-01-01T00:00:00Z"
        })).collect::<Vec<_>>(),
        "generatedAt":"1970-01-01T00:00:00Z"
    });
    fs::write(
        project.join("skills.lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();
    let pre_lock = fs::read_to_string(project.join("skills.lock.json")).unwrap();

    let mut cmd = cargo_bin_cmd!("sk");
    let out = cmd
        .current_dir(&project)
        .env("SK_CACHE_DIR", cache_root.to_str().unwrap())
        .env("SK_SIMULATE_EXDEV", "1")
        .env("SK_FAIL_COPY", "1")
        .args(["upgrade", "--all"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected failure: {out:?}");

    let post_lock = fs::read_to_string(project.join("skills.lock.json")).unwrap();
    assert_eq!(pre_lock, post_lock);
}
