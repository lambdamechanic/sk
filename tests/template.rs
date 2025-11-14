use std::fs;

#[path = "support/mod.rs"]
mod support;

use support::CliFixture;

#[test]
fn template_create_scaffolds_skill() {
    let fx = CliFixture::new();
    fx.sk_success(&["init"]);

    let template_repo = fx.create_remote("template-source", ".", "template-source");
    let source_arg = format!("{} template-source", template_repo.file_url());
    fx.sk_success(&["config", "set", "template_source", &source_arg]);

    fx.sk_success(&[
        "template",
        "create",
        "custom-skill",
        "Custom skill description",
    ]);

    let skill_dir = fx.skill_dir("custom-skill");
    assert!(skill_dir.exists(), "skill directory should exist");
    let meta = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert!(meta.contains("name: custom-skill"));
    assert!(meta.contains("description: Custom skill description"));
    let copied = fs::read_to_string(skill_dir.join("file.txt")).unwrap();
    assert_eq!(copied, "v1\n");
}

#[test]
fn template_create_uses_custom_root() {
    let fx = CliFixture::new();
    fx.sk_success(&["init", "--root", "./custom-skills"]);

    let template_repo = fx.create_remote("template-root", ".", "template-root");
    let source_arg = format!("{} template-root", template_repo.file_url());
    fx.sk_success(&["config", "set", "template_source", &source_arg]);

    fx.sk_success(&["template", "create", "rooted", "Rooted description"]);

    let custom_dir = fx.project.join("custom-skills").join("rooted");
    assert!(custom_dir.exists());
    let contents = fs::read_to_string(custom_dir.join("SKILL.md")).unwrap();
    assert!(contents.contains("name: rooted"));
}
