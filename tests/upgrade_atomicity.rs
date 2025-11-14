use assert_cmd::cargo::cargo_bin_cmd;
use proptest::prelude::*;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

#[path = "support/mod.rs"]
mod support;

use support::{clone_into_cache, extract_subdir_from_commit, git, path_to_file_url};

fn write(path: &Path, contents: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn digest_dir(dir: &Path) -> String {
    sk::digest::digest_dir(dir).expect("compute digest")
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
    git(&["config", "commit.gpgSign", "false"], &work);

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
            let (bare, v1, v2) = init_skill_repo(&remotes_root, &repo, &skill_path);
            let file_url = path_to_file_url(&bare);
            let cache = clone_into_cache(&cache_root, host, owner, &repo, &bare, &file_url);

            // Install v1 into project
            let installed_name = format!("s{i}");
            let dest = project.join("skills").join(&installed_name);
            extract_subdir_from_commit(&cache, &v1, &skill_path, &dest);
            let digest = digest_dir(&dest);
            entries.push((installed_name, repo, skill_path, v1, v2, digest, file_url));
        }

        // Mark one as modified by appending to file
        let (name_k, _repo_k, _path_k, _v1_k, _v2_k, _digest_k, _url_k) = entries[modified_idx].clone();
        let f = project.join("skills").join(&name_k).join("file.txt");
        fs::OpenOptions::new().append(true).open(&f).unwrap().write_all(b"local-edit\n").unwrap();
        let _new_digest = digest_dir(&project.join("skills").join(&name_k));

        // Write lockfile with v1 commits and original digests
        let lock = serde_json::json!({
            "version": 1,
            "skills": entries.iter().map(|(name, repo, skill_path, v1, _v2, digest, url)| serde_json::json!({
                "installName": name,
                "source": {"url":url,"host":host,"owner":owner,"repo":repo,"skillPath": skill_path},
                "commit": v1,
                "digest": digest,
                "installedAt": "1970-01-01T00:00:00Z"
            })).collect::<Vec<_>>(),
            "generatedAt": "1970-01-01T00:00:00Z"
        });
        write(&project.join("skills.lock.json"), &serde_json::to_string_pretty(&lock).unwrap());

        // Snapshot pre-state
        // Run `sk upgrade --all` and expect clean skills to advance while modified ones are skipped
        let mut cmd = cargo_bin_cmd!("sk");
        cmd.current_dir(&project);
        cmd.env("SK_CACHE_DIR", cache_root.to_str().unwrap());
        let out = cmd.args(["upgrade", "--all"]).output().unwrap();
        assert!(out.status.success(), "upgrade failed: {}", String::from_utf8_lossy(&out.stderr));

        let post_lock = fs::read_to_string(project.join("skills.lock.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&post_lock).unwrap();
        let skills = parsed.get("skills").and_then(|v| v.as_array()).unwrap();

        for (idx, (name, _repo, _path, v1, v2, _digest, _url)) in entries.iter().enumerate() {
            let entry = skills
                .iter()
                .find(|e| e.get("installName") == Some(&serde_json::Value::String(name.clone())))
                .expect("lock entry");
            let commit = entry
                .get("commit")
                .and_then(|v| v.as_str())
                .expect("commit str");
            let file_txt = fs::read_to_string(project.join("skills").join(name).join("file.txt")).unwrap();
            if idx == modified_idx {
                prop_assert_eq!(commit, v1);
                prop_assert!(file_txt.contains("local-edit"));
            } else {
                prop_assert_eq!(commit, v2);
                prop_assert!(file_txt.contains("v2"));
                prop_assert!(!file_txt.contains("local-edit"));
            }
        }
    }
}
