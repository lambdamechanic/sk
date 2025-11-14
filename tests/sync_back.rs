use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::{git, normalize_newlines, CliFixture};

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
