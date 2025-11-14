use std::{env, fs};

#[path = "support/mod.rs"]
mod support;

use support::{CliFixture, FakeGh};

#[test]
fn quickstart_readme_flow() {
    if env::var_os("CI").is_none() {
        println!("skipping quickstart_readme_flow (set CI=1 to run)");
        return;
    }
    let fx = CliFixture::new();
    let gh = FakeGh::new(&fx.root);

    let run = |args: &[&str]| {
        let mut cmd = fx.sk_cmd();
        gh.configure_cmd(&mut cmd);
        let out = cmd.args(args).output().unwrap();
        assert!(
            out.status.success(),
            "sk {:?} failed:\nstdout={}\nstderr={}",
            args,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    };

    // Quickstart: init + config
    run(&["init"]);
    run(&["config", "set", "default_root", "./skills"]);
    run(&["config", "set", "protocol", "https"]);

    run(&[
        "install",
        "@anthropics/skills",
        "template-skill",
        "--alias",
        "template",
    ]);
    run(&["install", "@anthropics/skills", "frontend-design"]);
    run(&["install", "@anthropics/skills", "artifacts-builder"]);
    run(&["list"]);
    run(&["status", "template", "frontend-design", "artifacts-builder"]);

    // Detect local edits with doctor.
    let frontend_notes = fx.skill_dir("frontend-design").join("LOCAL_NOTES.md");
    fs::write(&frontend_notes, "Modified locally\n").unwrap();
    run(&["doctor"]);

    let personal_repo = fx.create_remote("user-claude-skills", ".", "seed");
    let retro_dir = fx.skill_dir("retro-template");
    fs::create_dir_all(&retro_dir).unwrap();
    fs::write(
        retro_dir.join("SKILL.md"),
        "---\nname: retro-template\ndescription: Retro template quickstart skill\n---\n",
    )
    .unwrap();
    fs::write(
        retro_dir.join("NOTES.md"),
        "Checklist outline for sprint retros.\n",
    )
    .unwrap();

    gh.clear_state();
    run(&[
        "sync-back",
        "retro-template",
        "--repo",
        &personal_repo.file_url(),
        "--skill-path",
        "retro-template",
        "--branch",
        "sk/new/retro-template",
    ]);

    // Confirm retro-template made it into the lockfile.
    let lock = fx.lock_json();
    assert!(
        lock["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["installName"] == "retro-template"),
        "retro-template should be tracked in skills.lock.json"
    );
}
