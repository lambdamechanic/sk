use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::{git, normalize_newlines, CliFixture, FakeGh};

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
