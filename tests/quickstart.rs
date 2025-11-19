use std::{env, fs, path::PathBuf, process::Command};

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
    gh.clear_state();
    let personal_repo = fx.create_remote("user-claude-skills", ".", "seed");

    let commands = quickstart_commands();
    run_quickstart_with_cram(&fx, &gh, &commands, &personal_repo.file_url());

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
    // Detect local edits with doctor.
    let frontend_notes = fx.skill_dir("brand-guidelines").join("LOCAL_NOTES.md");
    fs::write(&frontend_notes, "Modified locally\n").unwrap();
    run(&["doctor"]);

    run(&["config", "set", "default_repo", &personal_repo.file_url()]);
    let retro_dir = fx.skill_dir("retro-template");
    assert!(
        retro_dir.exists(),
        "README quickstart must create retro-template; update tests/README to stay in sync"
    );
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

fn quickstart_commands() -> Vec<String> {
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
    let mut commands = Vec::new();
    let mut remainder = section;
    let fence = "```bash";
    loop {
        let fence_start = match remainder.find(fence) {
            Some(idx) => idx + fence.len(),
            None => break,
        };
        let after_fence = &remainder[fence_start..];
        let fence_end = after_fence
            .find("```")
            .expect("quickstart section must close code block");
        let block = &after_fence[..fence_end];
        for command in block.lines().filter_map(sanitized_command) {
            commands.push(command.to_string());
        }
        remainder = &after_fence[fence_end + "```".len()..];
    }

    assert!(
        !commands.is_empty(),
        "quickstart markers without any ```bash blocks"
    );

    commands
}

fn sanitized_command(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    if !trimmed.starts_with("sk") {
        return None;
    }
    if trimmed.len() > 2 {
        let after = trimmed[2..].chars().next().unwrap_or(' ');
        if !after.is_whitespace() {
            return None;
        }
    }

    if trimmed == "sk" {
        return None;
    }
    if trimmed.contains('<') || trimmed.contains('>') {
        return None;
    }

    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut end = trimmed.len();

    for (idx, ch) in trimmed.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => {
                end = idx;
                break;
            }
            _ => {}
        }
    }

    let command = trimmed[..end].trim_end();
    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

#[test]
fn sanitized_command_strips_inline_comments() {
    assert_eq!(sanitized_command("   # full line comment"), None);
    assert_eq!(
        sanitized_command("sk update    # fetch cache"),
        Some("sk update")
    );
    assert_eq!(
        sanitized_command("sk template create \"Retro #1\"  # annotate"),
        Some("sk template create \"Retro #1\"")
    );
    assert_eq!(
        sanitized_command("sk say \\#literal"),
        Some("sk say \\#literal")
    );
    assert_eq!(sanitized_command("cargo install sk"), None);
    assert_eq!(sanitized_command("sk remove <name>"), None);
}

fn run_quickstart_with_cram(fx: &CliFixture, gh: &FakeGh, commands: &[String], default_repo: &str) {
    let script_path = fx.project.join("quickstart_cram.t");
    let script = build_cram_script(commands, default_repo);
    fs::write(&script_path, script).expect("write quickstart cram file");

    let cram_site = ensure_cram_site();
    let python = env::var("PYTHON").unwrap_or_else(|_| "python3".into());
    let mut cmd = Command::new(python);
    cmd.arg("-m")
        .arg("cram")
        .arg("-E")
        .arg("--shell=/bin/bash")
        .arg(script_path.file_name().unwrap());
    cmd.current_dir(&fx.project);

    let mut python_paths = vec![cram_site.clone()];
    if let Some(existing) = env::var_os("PYTHONPATH") {
        if !existing.is_empty() {
            python_paths.extend(env::split_paths(&existing));
        }
    }
    let pythonpath = env::join_paths(python_paths).expect("join PYTHONPATH entries");
    cmd.env("PYTHONPATH", pythonpath);

    let sk_bin = PathBuf::from(env!("CARGO_BIN_EXE_sk"));
    let sk_bin_dir = sk_bin
        .parent()
        .expect("CARGO_BIN_EXE_sk should include parent directory");
    let mut path_entries = vec![sk_bin_dir.to_path_buf(), gh.bin_dir().to_path_buf()];
    if let Some(existing) = env::var_os("PATH") {
        path_entries.extend(env::split_paths(&existing));
    }
    let joined_path = env::join_paths(path_entries).expect("join PATH entries");
    cmd.env("PATH", joined_path);
    cmd.env("SK_CACHE_DIR", fx.cache_root());
    cmd.env("SK_CONFIG_DIR", fx.config_dir());
    cmd.env("GIT_AUTHOR_NAME", "Test User");
    cmd.env("GIT_AUTHOR_EMAIL", "test@example.com");
    cmd.env("GIT_COMMITTER_NAME", "Test User");
    cmd.env("GIT_COMMITTER_EMAIL", "test@example.com");
    cmd.env("SK_TEST_GH_STATE_FILE", gh.state_file());

    let status = cmd.status().expect("run cram quickstart");
    if !status.success() {
        panic!(
            "cram quickstart failed (install python3 + pip and ensure network access for dependencies)"
        );
    }

    let _ = fs::remove_file(&script_path);
}

fn build_cram_script(commands: &[String], default_repo: &str) -> String {
    let mut script = String::from("# Auto-generated from README quickstart\n\n");
    script.push_str("  $ set -euo pipefail\n");
    script.push_str("  $ cd \"$TESTDIR\"\n");
    for command in commands {
        let rewritten = rewrite_command_for_cram(command, default_repo);
        script.push_str("  $ ");
        script.push_str(&rewritten);
        script.push_str(" >/dev/null 2>&1\n");
    }
    script
}

fn rewrite_command_for_cram(command: &str, default_repo: &str) -> String {
    if command.starts_with("sk config set default_repo ") {
        return format!("sk config set default_repo {}", default_repo);
    }
    command.to_string()
}

fn ensure_cram_site() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("cram-site");
    if dir.join("cram").exists() {
        return dir;
    }
    fs::create_dir_all(&dir).expect("create cram-site directory");
    let target = dir
        .to_str()
        .expect("workspace path should be valid UTF-8 for pip");
    let status = Command::new("python3")
        .args([
            "-m", "pip", "install", "--quiet", "--target", target, "cram",
        ])
        .status()
        .expect("invoke pip to install cram");
    assert!(
        status.success(),
        "failed to install cram via pip (run `python3 -m pip install cram`)."
    );
    dir
}
