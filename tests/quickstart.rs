use std::{env, fs};

#[path = "support/mod.rs"]
mod support;

use shell_words::split as shell_split;
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
    for args in quickstart_commands() {
        let mut normalized: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        if normalized.first().copied() == Some("sk") {
            normalized.remove(0);
        }
        run(&normalized);
    }

    // Detect local edits with doctor.
    let frontend_notes = fx.skill_dir("brand-guidelines").join("LOCAL_NOTES.md");
    fs::write(&frontend_notes, "Modified locally\n").unwrap();
    run(&["doctor"]);

    let personal_repo = fx.create_remote("user-claude-skills", ".", "seed");
    run(&["config", "set", "default_repo", &personal_repo.file_url()]);
    run(&[
        "template",
        "create",
        "retro-template",
        "Retro template quickstart skill",
    ]);
    let retro_dir = fx.skill_dir("retro-template");
    fs::write(
        retro_dir.join("NOTES.md"),
        "Checklist outline for sprint retros.\n",
    )
    .unwrap();

    gh.clear_state();
    run(&["sync-back", "retro-template"]);

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

fn quickstart_commands() -> Vec<Vec<String>> {
    let content =
        fs::read_to_string("README.md").expect("README must exist for quickstart extraction");
    let start_marker = "<!-- QUICKSTART COMMANDS START -->";
    let end_marker = "<!-- QUICKSTART COMMANDS END -->";
    let start = content
        .find(start_marker)
        .expect("README missing quickstart start marker");
    let end = content
        .find(end_marker)
        .expect("README missing quickstart end marker");
    let section = &content[start + start_marker.len()..end];
    let snippet_start = section
        .find("```bash")
        .expect("quickstart section must use ```bash")
        + "```bash".len();
    let snippet_end = section[snippet_start..]
        .find("```")
        .expect("quickstart section must close code block")
        + snippet_start;
    let snippet = &section[snippet_start..snippet_end];

    snippet
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            Some(
                shell_split(trimmed)
                    .unwrap_or_else(|err| panic!("parse quickstart command `{}`: {err}", trimmed)),
            )
        })
        .collect()
}
