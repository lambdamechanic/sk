use assert_cmd::cargo::cargo_bin_cmd;
use proptest::prelude::*;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

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

fn digest_dir(dir: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    files.sort();
    for path in files {
        let rel = path.strip_prefix(dir).unwrap_or(&path);
        hasher.update(rel.to_string_lossy().as_bytes());
        let data = fs::read(&path).unwrap();
        hasher.update(&data);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn init_skill_repo(root: &Path, name: &str, skill_path: &str) -> (PathBuf, String, String) {
    let bare = root.join("remotes").join(format!("{name}.git"));
    fs::create_dir_all(&bare).unwrap();
    git(&["init", "--bare", "-b", "main"], &bare);

    let work = root.join("sources").join(name);
    fs::create_dir_all(&work).unwrap();
    git(&["init", "-b", "main"], &work);
    git(&["remote", "add", "origin", bare.to_str().unwrap()], &work);
    // Configure identity for commits on CI
    git(&["config", "user.email", "test@example.com"], &work);
    git(&["config", "user.name", "Test User"], &work);

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

    // v2
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

fn clone_to_cache(
    cache_root: &Path,
    host: &str,
    owner: &str,
    repo: &str,
    bare_remote: &Path,
) -> PathBuf {
    let dest = cache_root.join("repos").join(host).join(owner).join(repo);
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    git(
        &[
            "clone",
            bare_remote.to_str().unwrap(),
            dest.to_str().unwrap(),
        ],
        dest.parent().unwrap(),
    );
    // Ensure origin/HEAD set to main
    git(&["remote", "set-head", "origin", "-a"], &dest);
    dest
}

fn extract_subdir(cache: &Path, commit: &str, subdir: &str, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    let strip = subdir.split('/').count().to_string();
    let mut archive = Command::new("git")
        .args([
            "-C",
            cache.to_str().unwrap(),
            "archive",
            "--format=tar",
            commit,
            subdir,
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = archive.stdout.take().unwrap();
    let status = Command::new("tar")
        .args([
            "-x",
            "--strip-components",
            &strip,
            "-C",
            dest.to_str().unwrap(),
        ])
        .stdin(stdout)
        .status()
        .unwrap();
    assert!(status.success());
    let _ = archive.wait();
}

prop_compose! {
    fn counts_and_index()(n in 2usize..=3)(idx in 0usize..n-1, n in Just(n)) -> (usize, usize) { (n, idx) }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2, .. ProptestConfig::default() })]
    #[test]
    fn upgrade_is_atomic_when_any_modified((n, modified_idx) in counts_and_index()) {
        // Layout temp dirs: cache, remotes, project
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let cache_root = root.join("cache");
        let remotes_root = root.join("remotes_root");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        git(&["init", "-b", "main"], &project);

        // Build N skills
        let host = "local"; let owner = "o";
        let mut entries = vec![];
        for i in 0..n {
            let repo = format!("r{i}");
            let skill_path = format!("skill-{i}");
            let (bare, v1, _v2) = init_skill_repo(&remotes_root, &repo, &skill_path);
            let cache = clone_to_cache(&cache_root, host, owner, &repo, &bare);

            // Install v1 into project
            let installed_name = format!("s{i}");
            let dest = project.join("skills").join(&installed_name);
            extract_subdir(&cache, &v1, &skill_path, &dest);
            let digest = digest_dir(&dest);
            entries.push((installed_name, repo, skill_path, v1, digest));
        }

        // Mark one as modified by appending to file
        let (name_k, _repo_k, _path_k, _v1_k, _digest_k) = entries[modified_idx].clone();
        let f = project.join("skills").join(&name_k).join("file.txt");
        fs::OpenOptions::new().append(true).open(&f).unwrap().write_all(b"local-edit\n").unwrap();
        let _new_digest = digest_dir(&project.join("skills").join(&name_k));

        // Write lockfile with v1 commits and original digests
        let lock = serde_json::json!({
            "version": 1,
            "skills": entries.iter().map(|(name, repo, skill_path, v1, digest)| serde_json::json!({
                "installName": name,
                "source": {"url":"file://dummy","host":host,"owner":owner,"repo":repo,"skillPath": skill_path},
                "ref": null,
                "commit": v1,
                "digest": digest,
                "installedAt": "1970-01-01T00:00:00Z"
            })).collect::<Vec<_>>(),
            "generatedAt": "1970-01-01T00:00:00Z"
        });
        write(&project.join("skills.lock.json"), &serde_json::to_string_pretty(&lock).unwrap());

        // Snapshot pre-state
        let pre_lock = fs::read_to_string(project.join("skills.lock.json")).unwrap();
        let pre_digests: Vec<String> = entries.iter().map(|(name,_,_,_,_)| digest_dir(&project.join("skills").join(name))).collect();

        // Run `sk upgrade --all` and expect failure (modified)
        let mut cmd = cargo_bin_cmd!("sk");
        cmd.current_dir(&project);
        cmd.env("SK_CACHE_DIR", cache_root.to_str().unwrap());
        let out = cmd.args(["upgrade", "--all"]).output().unwrap();
        assert!(!out.status.success(), "upgrade unexpectedly succeeded");

        // Assert no changes on disk and lockfile unchanged
        let post_lock = fs::read_to_string(project.join("skills.lock.json")).unwrap();
        assert_eq!(pre_lock, post_lock, "lockfile changed on failure");
        let post_digests: Vec<String> = entries.iter().map(|(name,_,_,_,_)| digest_dir(&project.join("skills").join(name))).collect();
        prop_assert_eq!(pre_digests, post_digests);
    }
}
