use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::{git, normalize_newlines, CliFixture, FakeGh};

#[test]
fn sync_back_defaults_to_config_repo_and_install_name() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let personal = fx.create_remote("skills-default", ".", "placeholder");
    let target_url = personal.file_url();
    fx.sk_success(&["config", "set", "default_repo", target_url.as_str()]);

    let skill_dir = fx.skill_dir("local-default");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: local-default\ndescription: local\n---\n",
    )
    .unwrap();
    fs::write(skill_dir.join("README.md"), "v1\n").unwrap();

    let mut cmd = fx.sk_cmd();
    cmd.env("SK_FORCE_GH_MISSING", "1").args([
        "sync-back",
        "local-default",
        "--message",
        "Default config publish",
    ]);
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "sync-back failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let lock = fx.lock_json();
    let skills = lock
        .get("skills")
        .and_then(|v| v.as_array())
        .expect("skills array");
    let entry = skills
        .iter()
        .find(|entry| entry.get("installName") == Some(&"local-default".into()))
        .expect("lock entry for local-default");
    assert_eq!(
        entry["source"]["repoKey"].as_str().unwrap(),
        "local/remotes/skills-default"
    );
    assert_eq!(
        entry["source"]["skillPath"].as_str().unwrap(),
        "local-default"
    );

    let repos = lock["repos"]["entries"].as_array().unwrap();
    assert!(
        repos
            .iter()
            .any(|repo| repo["url"].as_str() == Some(target_url.as_str())),
        "expected repo entry for default repo: {:?}",
        repos
    );
}

#[test]
fn sync_back_errors_when_default_repo_missing() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let skill_dir = fx.skill_dir("needs-repo");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: needs-repo\ndescription: missing repo\n---\n",
    )
    .unwrap();
    fs::write(skill_dir.join("README.md"), "content\n").unwrap();

    let out = fx
        .sk_cmd()
        .args(["sync-back", "needs-repo", "--message", "Should fail"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "sync-back unexpectedly succeeded without default repo"
    );
    let stderr = normalize_newlines(&String::from_utf8_lossy(&out.stderr));
    assert!(
        stderr.contains("default_repo is not configured"),
        "missing helpful error: {stderr}"
    );
}

#[test]
fn sync_back_warns_when_rsync_missing() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let personal = fx.create_remote("skills-rsync", ".", "placeholder");
    let target_url = personal.file_url();
    fx.sk_success(&["config", "set", "default_repo", target_url.as_str()]);

    let skill_dir = fx.skill_dir("needs-rsync");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: needs-rsync\ndescription: rsync fallback\n---\n",
    )
    .unwrap();
    fs::write(skill_dir.join("README.md"), "fallback\n").unwrap();

    let gh = FakeGh::new(&fx.root);
    gh.clear_state();
    let mut cmd = fx.sk_cmd();
    gh.configure_cmd(&mut cmd);
    cmd.env("SK_FORCE_RSYNC_MISSING", "1").args([
        "sync-back",
        "needs-rsync",
        "--message",
        "Force rsync warning",
    ]);
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "sync-back failed during rsync fallback: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = normalize_newlines(&String::from_utf8_lossy(&out.stderr));
    assert!(
        stderr.contains("Warning: 'rsync' not found"),
        "rsync warning missing: {stderr}"
    );
}

#[test]
fn sync_back_warns_when_gh_missing() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let personal = fx.create_remote("skills-missing-gh", ".", "placeholder");
    let target_url = personal.file_url();
    fx.sk_success(&["config", "set", "default_repo", target_url.as_str()]);

    let skill_dir = fx.skill_dir("gh-warning");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: gh-warning\ndescription: gh fallback\n---\n",
    )
    .unwrap();
    fs::write(skill_dir.join("README.md"), "gh fallback\n").unwrap();

    let out = fx
        .sk_cmd()
        .env("SK_FORCE_GH_MISSING", "1")
        .args(["sync-back", "gh-warning", "--message", "GH warning"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "sync-back failed without gh: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = normalize_newlines(&String::from_utf8_lossy(&out.stderr));
    assert!(
        stderr.contains(
            "Warning: skipping PR automation because the GitHub CLI ('gh') is unavailable"
        ),
        "gh warning missing: {stderr}"
    );
}

#[test]
fn sync_back_publishes_new_skill_with_repo_override() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("skills-upstream", "template", "template-skill");

    let skill_dir = fx.skill_dir("sk");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: sk\ndescription: repo-scoped CLI skill\n---\n",
    )
    .unwrap();
    fs::write(skill_dir.join("README.md"), "local content\n").unwrap();

    let repo_url = remote.file_url();
    let out = fx
        .sk_cmd()
        .args([
            "sync-back",
            "sk",
            "--repo",
            &repo_url,
            "--skill-path",
            "sk",
            "--branch",
            "sk/new-skill",
            "--message",
            "Add sk skill from fixture",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "sync-back failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    git(&["fetch", "origin"], &remote.work);
    git(&["checkout", "sk/new-skill"], &remote.work);
    let pushed = fs::read_to_string(remote.work.join("sk").join("README.md")).unwrap();
    assert_eq!(normalize_newlines(&pushed), "local content\n");

    let lock = fx.lock_json();
    let skills = lock
        .get("skills")
        .and_then(|v| v.as_array())
        .expect("lockfile skills");
    assert!(
        skills
            .iter()
            .any(|entry| entry.get("installName") == Some(&"sk".into())),
        "lockfile should include new skill entry"
    );
}

#[test]
fn sync_back_auto_creates_pr_and_arms_auto_merge() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("skills-auto", "template", "template-skill");

    let skill_dir = fx.skill_dir("sk-auto");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: sk-auto\ndescription: repo-scoped CLI skill\n---\n",
    )
    .unwrap();
    fs::write(skill_dir.join("README.md"), "auto content\n").unwrap();

    let gh = FakeGh::new(&fx.root);
    gh.clear_state();

    let repo_url = remote.file_url();
    let mut cmd = fx.sk_cmd();
    gh.configure_cmd(&mut cmd);
    let out = cmd
        .env("SK_TEST_GH_PR_STATE", "CLEAN")
        .env("SK_TEST_GH_PR_URL", "https://example.test/pr/7")
        .env("SK_TEST_GH_PR_NUMBER", "7")
        .args([
            "sync-back",
            "sk-auto",
            "--repo",
            &repo_url,
            "--skill-path",
            "sk-auto",
            "--branch",
            "sk/auto-branch",
            "--message",
            "Automate PR flow",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "sync-back failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = normalize_newlines(&String::from_utf8_lossy(&out.stdout));
    assert!(
        stdout.contains("Opened PR https://example.test/pr/7"),
        "stdout missing PR creation: {stdout}"
    );
    assert!(
        stdout.contains("Auto-merge armed; GitHub will land https://example.test/pr/7"),
        "stdout missing auto-merge message: {stdout}"
    );
}

#[test]
fn sync_back_reports_conflicts_when_auto_merge_fails() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("skills-conflict", "template", "template-skill");

    let skill_dir = fx.skill_dir("sk-conflict");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: sk-conflict\ndescription: repo-scoped CLI skill\n---\n",
    )
    .unwrap();
    fs::write(skill_dir.join("README.md"), "conflict content\n").unwrap();

    let gh = FakeGh::new(&fx.root);
    gh.clear_state();

    let repo_url = remote.file_url();
    let mut cmd = fx.sk_cmd();
    gh.configure_cmd(&mut cmd);
    let out = cmd
        .env("SK_TEST_GH_PR_STATE", "DIRTY")
        .env("SK_TEST_GH_PR_URL", "https://example.test/pr/404")
        .env("SK_TEST_GH_PR_NUMBER", "404")
        .args([
            "sync-back",
            "sk-conflict",
            "--repo",
            &repo_url,
            "--skill-path",
            "sk-conflict",
            "--branch",
            "sk/conflict-branch",
            "--message",
            "Conflict PR",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "sync-back failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = normalize_newlines(&String::from_utf8_lossy(&out.stdout));
    assert!(
        stdout.contains("Opened PR https://example.test/pr/404"),
        "stdout missing PR creation: {stdout}"
    );
    assert!(
        stdout.contains(
            "Auto-merge blocked by conflicts. Resolve manually: https://example.test/pr/404"
        ),
        "stdout missing conflict message: {stdout}"
    );
}

#[test]
fn sync_back_points_to_auto_merge_settings_when_disabled() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("skills-no-automerge", "template", "template-skill");

    let skill_dir = fx.skill_dir("sk-no-automerge");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: sk-no-automerge\ndescription: repo-scoped CLI skill\n---\n",
    )
    .unwrap();
    fs::write(skill_dir.join("README.md"), "auto merge disabled\n").unwrap();

    let gh = FakeGh::new(&fx.root);
    gh.clear_state();

    let repo_url = remote.file_url();
    let mut cmd = fx.sk_cmd();
    gh.configure_cmd(&mut cmd);
    let out = cmd
        .env(
            "SK_TEST_GH_AUTO_MERGE_ERROR",
            "GraphQL: Pull request Protected branch rules not configured for this branch (enablePullRequestAutoMerge)",
        )
        .env("SK_TEST_GH_PR_STATE", "CLEAN")
        .env("SK_TEST_GH_PR_URL", "https://example.test/pr/505")
        .env("SK_TEST_GH_PR_NUMBER", "505")
        .args([
            "sync-back",
            "sk-no-automerge",
            "--repo",
            &repo_url,
            "--skill-path",
            "sk-no-automerge",
            "--branch",
            "sk/no-automerge-branch",
            "--message",
            "Auto merge disabled",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "sync-back failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = normalize_newlines(&String::from_utf8_lossy(&out.stdout));
    assert!(
        stdout.contains("Tip: enable auto-merge with `gh repo edit"),
        "stdout missing auto-merge tip: {stdout}"
    );
}

#[test]
fn sync_back_refreshes_lock_to_merged_commit_after_auto_merge() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("skills-lock-refresh", "template", "template-skill");

    let skill_dir = fx.skill_dir("sk-merged");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: sk-merged\ndescription: lock refresh test\n---\n",
    )
    .unwrap();
    fs::write(skill_dir.join("README.md"), "lock refresh content\n").unwrap();

    let gh = FakeGh::new(&fx.root);
    gh.clear_state();

    let branch = "sk/lock-refresh";
    let repo_url = remote.file_url();
    let merge_repo = remote.work.to_string_lossy().to_string();
    let mut cmd = fx.sk_cmd();
    gh.configure_cmd(&mut cmd);
    let out = cmd
        .env("SK_TEST_GH_PR_STATE", "CLEAN")
        .env("SK_TEST_GH_PR_URL", "https://example.test/pr/909")
        .env("SK_TEST_GH_PR_NUMBER", "909")
        .env("SK_TEST_GH_AUTO_MERGE_REPO", &merge_repo)
        .env("SK_TEST_GH_AUTO_MERGE_BRANCH", branch)
        .env("SK_SYNC_BACK_AUTO_MERGE_TIMEOUT_MS", "2000")
        .env("SK_SYNC_BACK_AUTO_MERGE_POLL_MS", "100")
        .args([
            "sync-back",
            "sk-merged",
            "--repo",
            &repo_url,
            "--skill-path",
            remote.skill_path(),
            "--branch",
            branch,
            "--message",
            "Lock refresh test",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "sync-back failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let lock = fx.lock_json();
    let entry = lock
        .get("skills")
        .and_then(|v| v.as_array())
        .and_then(|skills| {
            skills
                .iter()
                .find(|skill| skill.get("installName") == Some(&"sk-merged".into()))
        })
        .expect("lock entry exists");
    let locked_commit = entry
        .get("commit")
        .and_then(|v| v.as_str())
        .expect("commit field present");

    let merged_head = remote.head();
    assert_eq!(
        locked_commit, merged_head,
        "lockfile should track merged commit"
    );
}

#[test]
fn sync_back_refreshes_local_digest_to_merged_commit() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let remote = fx.create_remote("skills-digest-refresh", "template", "sk-digest");
    fx.install_from_remote(&remote, "sk-digest");

    // Upstream changes land after install but before sync-back.
    remote.overwrite_file("file.txt", "remote v2\n", "Upstream edit");

    // Local edits add a new file so the sync-back branch diverges.
    let skill_dir = fx.skill_dir("sk-digest");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("local.txt"), "local content\n").unwrap();

    let gh = FakeGh::new(&fx.root);
    gh.clear_state();

    let branch = "sk/digest-refresh";
    let merge_repo = remote.work.to_string_lossy().to_string();
    let mut cmd = fx.sk_cmd();
    gh.configure_cmd(&mut cmd);
    let out = cmd
        .env("SK_TEST_GH_PR_STATE", "CLEAN")
        .env("SK_TEST_GH_PR_URL", "https://example.test/pr/111")
        .env("SK_TEST_GH_PR_NUMBER", "111")
        .env("SK_TEST_GH_AUTO_MERGE_REPO", &merge_repo)
        .env("SK_TEST_GH_AUTO_MERGE_BRANCH", branch)
        .env("SK_SYNC_BACK_AUTO_MERGE_TIMEOUT_MS", "2000")
        .env("SK_SYNC_BACK_AUTO_MERGE_POLL_MS", "100")
        .args([
            "sync-back",
            "sk-digest",
            "--branch",
            branch,
            "--message",
            "Refresh digest after merge",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "sync-back failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let remote_skill_dir = if remote.skill_path() == "." {
        remote.work.clone()
    } else {
        remote.work.join(remote.skill_path())
    };
    let local_file = fs::read_to_string(skill_dir.join("file.txt")).unwrap();
    let remote_file = fs::read_to_string(remote_skill_dir.join("file.txt")).unwrap();
    assert_eq!(
        normalize_newlines(&local_file),
        normalize_newlines(&remote_file),
        "local install should match upstream merged contents"
    );

    let local_digest = sk::digest::digest_dir(&skill_dir).unwrap();
    let remote_digest = sk::digest::digest_dir(&remote_skill_dir).unwrap();
    assert_eq!(
        local_digest, remote_digest,
        "local digest should equal merged commit digest"
    );

    let lock = fx.lock_json();
    let entry = lock
        .get("skills")
        .and_then(|v| v.as_array())
        .and_then(|skills| {
            skills
                .iter()
                .find(|skill| skill.get("installName") == Some(&"sk-digest".into()))
        })
        .expect("lock entry exists");
    let locked_digest = entry
        .get("digest")
        .and_then(|v| v.as_str())
        .expect("digest field present");
    assert_eq!(
        locked_digest, local_digest,
        "lock digest should capture merged tree"
    );

    let locked_commit = entry
        .get("commit")
        .and_then(|v| v.as_str())
        .expect("commit field present");
    let merged_head = remote.head();
    assert_eq!(
        locked_commit, merged_head,
        "lockfile should track merged commit"
    );
}
