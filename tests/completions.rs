#[path = "support/mod.rs"]
mod support;

use std::str;

use support::CliFixture;

#[test]
fn completions_skills_emit_sorted_names() {
    let fx = CliFixture::new();
    let remote_bravo = fx.create_remote("repo-bravo", "skill", "bravo");
    fx.install_from_remote(&remote_bravo, "bravo");
    let remote_alpha = fx.create_remote("repo-alpha", "skill", "alpha");
    fx.install_from_remote(&remote_alpha, "alpha");

    let output = fx
        .sk_cmd()
        .args(["completions", "--skills"])
        .output()
        .unwrap();
    assert!(output.status.success(), "skills emitter should succeed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["alpha", "bravo"], "names should be sorted");
}

#[test]
fn bash_completion_contains_dynamic_hook() {
    let fx = CliFixture::new();
    let output = fx
        .sk_cmd()
        .args(["completions", "--shell", "bash"])
        .output()
        .unwrap();
    assert!(output.status.success(), "bash completions should render");
    let script = String::from_utf8(output.stdout).unwrap();
    assert!(
        script.contains("__sk_dynamic_complete"),
        "dynamic hook should be embedded"
    );
    assert!(
        script.contains("completions --skills"),
        "script should call back into sk for skill data"
    );
}

#[test]
fn zsh_completion_uses_skill_function() {
    let fx = CliFixture::new();
    let output = fx
        .sk_cmd()
        .args(["completions", "--shell", "zsh"])
        .output()
        .unwrap();
    assert!(output.status.success(), "zsh completions should render");
    let script = String::from_utf8(output.stdout).unwrap();
    assert!(
        script.contains("_sk_skill_names"),
        "zsh script should inject the skill completion helper"
    );
    assert!(
        script.contains("completions --skills"),
        "helper should source names from sk completions --skills"
    );
}
