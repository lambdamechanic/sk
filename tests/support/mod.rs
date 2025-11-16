#![allow(dead_code)]

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command as AssertCommand;
use serde::Deserialize;
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

pub fn git(args: &[&str], cwd: &Path) {
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

fn rev_parse_head(repo: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git rev-parse failed in {}",
        repo.display()
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn normalize_subdir(input: &str) -> String {
    let mut trimmed = input.trim();
    while let Some(rest) = trimmed.strip_prefix("./") {
        trimmed = rest;
    }
    trimmed = trimmed.trim_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        ".".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn path_to_file_url(p: &Path) -> String {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy().replace('\\', "/");
        if s.len() >= 2 && s.as_bytes()[1] == b':' {
            return format!("file:///{s}");
        }
        if s.starts_with("//") {
            return format!("file:{s}");
        }
        if s.starts_with('/') {
            return format!("file://{s}");
        }
        format!("file:///{s}")
    }
    #[cfg(not(windows))]
    {
        format!("file://{}", p.to_string_lossy())
    }
}

pub fn extract_subdir_from_commit(cache: &Path, commit: &str, subdir: &str, dest: &Path) {
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
        .stdout(Stdio::piped())
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
    assert!(
        status.success(),
        "tar extraction failed for {subdir} ({commit})"
    );
    let _ = archive.wait();
}

pub fn hashed_leaf(url: &str, repo: &str) -> String {
    let h = Sha256::digest(url.as_bytes());
    let hex = format!("{h:x}");
    let short = &hex[..12];
    format!("{repo}-{short}")
}

pub fn cache_repo_path(
    cache_root: &Path,
    host: &str,
    owner: &str,
    repo: &str,
    url_for_lock: &str,
) -> PathBuf {
    let leaf = if host == "local" && url_for_lock.starts_with("file://") {
        hashed_leaf(url_for_lock, repo)
    } else {
        repo.to_string()
    };
    cache_root.join("repos").join(host).join(owner).join(leaf)
}

pub struct CacheRepoSpec<'a> {
    pub host: &'a str,
    pub owner: &'a str,
    pub name: &'a str,
    pub url_for_lock: &'a str,
}

pub fn clone_into_cache(cache_root: &Path, spec: CacheRepoSpec<'_>, bare_remote: &Path) -> PathBuf {
    let dest = cache_repo_path(
        cache_root,
        spec.host,
        spec.owner,
        spec.name,
        spec.url_for_lock,
    );
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    git(
        &[
            "clone",
            bare_remote.to_str().unwrap(),
            dest.to_str().unwrap(),
        ],
        dest.parent().unwrap(),
    );
    git(&["remote", "set-head", "origin", "-a"], &dest);
    dest
}

pub struct RemoteRepo {
    pub bare: PathBuf,
    pub work: PathBuf,
    skill_subdir: String,
}

impl RemoteRepo {
    pub fn file_url(&self) -> String {
        path_to_file_url(&self.bare)
    }

    pub fn skill_path(&self) -> &str {
        &self.skill_subdir
    }

    pub fn overwrite_file(&self, rel: &str, contents: &str, message: &str) -> String {
        let mut base = self.work.clone();
        if self.skill_subdir != "." {
            base = base.join(&self.skill_subdir);
        }
        let target = base.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&target, contents).unwrap();
        git(&["add", "."], &self.work);
        git(&["commit", "-m", message], &self.work);
        git(&["push", "origin", "main"], &self.work);
        rev_parse_head(&self.work)
    }

    pub fn head(&self) -> String {
        rev_parse_head(&self.work)
    }
}

pub struct CliFixture {
    _tmp: TempDir,
    pub root: PathBuf,
    pub project: PathBuf,
    cache_base: PathBuf,
    config_dir: PathBuf,
    remotes_dir: PathBuf,
    sources_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusEntry {
    pub install_name: String,
    pub state: String,
    pub locked: Option<String>,
    pub current: Option<String>,
    pub update: Option<String>,
}

impl CliFixture {
    pub fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        git(&["init", "-b", "main"], &project);
        git(&["config", "user.email", "test@example.com"], &project);
        git(&["config", "user.name", "Test User"], &project);
        git(&["config", "commit.gpgSign", "false"], &project);

        let cache_base = root.join("cache");
        let config_dir = root.join("config");
        let remotes_dir = root.join("remotes");
        let sources_dir = root.join("sources");
        fs::create_dir_all(&cache_base).unwrap();
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&remotes_dir).unwrap();
        fs::create_dir_all(&sources_dir).unwrap();

        Self {
            _tmp: tmp,
            root,
            project,
            cache_base,
            config_dir,
            remotes_dir,
            sources_dir,
        }
    }

    pub fn sk_cmd(&self) -> AssertCommand {
        let mut cmd = cargo_bin_cmd!("sk");
        cmd.current_dir(&self.project)
            .env("SK_CACHE_DIR", &self.cache_base)
            .env("SK_CONFIG_DIR", &self.config_dir)
            .env("GIT_AUTHOR_NAME", "Test User")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test User")
            .env("GIT_COMMITTER_EMAIL", "test@example.com");
        cmd
    }

    pub fn sk_process(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sk"));
        cmd.current_dir(&self.project)
            .env("SK_CACHE_DIR", &self.cache_base)
            .env("SK_CONFIG_DIR", &self.config_dir)
            .env("GIT_AUTHOR_NAME", "Test User")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test User")
            .env("GIT_COMMITTER_EMAIL", "test@example.com");
        cmd
    }

    pub fn sk_success(&self, args: &[&str]) {
        let out = self.sk_cmd().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "sk {:?} failed: stdout={:?} stderr={:?}",
            args,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub fn run_json(&self, args: &[&str]) -> Json {
        let out = self.sk_cmd().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "sk {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    pub fn lock_json(&self) -> Json {
        let data = fs::read(self.project.join("skills.lock.json"))
            .expect("lockfile should exist for fixture");
        serde_json::from_slice(&data).unwrap()
    }

    pub fn skill_dir(&self, name: &str) -> PathBuf {
        self.project.join("skills").join(name)
    }

    pub fn create_remote(&self, repo: &str, skill_subdir: &str, skill_name: &str) -> RemoteRepo {
        let bare = self.remotes_dir.join(format!("{repo}.git"));
        fs::create_dir_all(&bare).unwrap();
        git(&["init", "--bare", "-b", "main"], &bare);

        let work = self.sources_dir.join(repo);
        fs::create_dir_all(&work).unwrap();
        git(&["init", "-b", "main"], &work);
        git(&["remote", "add", "origin", bare.to_str().unwrap()], &work);
        git(&["config", "user.email", "test@example.com"], &work);
        git(&["config", "user.name", "Test User"], &work);
        git(&["config", "commit.gpgSign", "false"], &work);

        let normalized = normalize_subdir(skill_subdir);
        let skill_dir = if normalized == "." {
            work.clone()
        } else {
            work.join(&normalized)
        };
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {skill_name}\ndescription: fixture\n---\n"),
        )
        .unwrap();
        fs::write(skill_dir.join("file.txt"), "v1\n").unwrap();
        git(&["add", "."], &work);
        git(&["commit", "-m", "v1"], &work);
        git(&["push", "-u", "origin", "main"], &work);

        RemoteRepo {
            bare,
            work,
            skill_subdir: normalized,
        }
    }

    pub fn install_from_remote(&self, remote: &RemoteRepo, skill_name: &str) {
        let file_url = remote.file_url();
        let mut cmd = self.sk_cmd();
        let mut args = vec!["install", &file_url, skill_name];
        if remote.skill_path() != "." {
            args.push("--path");
            args.push(remote.skill_path());
        }
        let out = cmd.args(&args).output().unwrap();
        assert!(
            out.status.success(),
            "install failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

pub fn parse_status_entries(value: Json) -> Vec<StatusEntry> {
    serde_json::from_value(value).expect("status json shape")
}

pub fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n")
}

pub struct FakeGh {
    bin_dir: PathBuf,
    state_file: PathBuf,
}

impl FakeGh {
    pub fn new(root: &Path) -> Self {
        let stub_dir = root.join("fake-gh");
        fs::create_dir_all(&stub_dir).unwrap();
        let src = stub_dir.join("main.rs");
        fs::write(&src, FAKE_GH_SOURCE).unwrap();
        let bin_name = if cfg!(windows) { "gh.exe" } else { "gh" };
        let bin_path = stub_dir.join(bin_name);
        let status = Command::new("rustc")
            .current_dir(&stub_dir)
            .args([
                "-O",
                src.file_name().unwrap().to_str().unwrap(),
                "-o",
                bin_path.to_string_lossy().as_ref(),
            ])
            .status()
            .expect("rustc should compile fake gh");
        assert!(status.success(), "failed to compile fake gh stub");
        Self {
            bin_dir: stub_dir.clone(),
            state_file: stub_dir.join("state.txt"),
        }
    }

    pub fn configure_cmd(&self, cmd: &mut AssertCommand) {
        let mut combined_paths = vec![self.bin_dir.clone()];
        if let Some(existing) = env::var_os("PATH") {
            combined_paths.extend(env::split_paths(&existing));
        }
        let joined = env::join_paths(combined_paths).expect("join PATH entries");
        cmd.env("PATH", joined);
        cmd.env("SK_TEST_GH_STATE_FILE", &self.state_file);
    }

    pub fn clear_state(&self) {
        let _ = fs::remove_file(&self.state_file);
    }
}

const FAKE_GH_SOURCE: &str = r#"use std::{
    env,
    fs,
    path::PathBuf,
    process::{self, Command},
};

#[derive(Clone, Debug)]
struct PrState {
    number: String,
    url: String,
    status: String,
    pr_state: String,
    merge_commit: Option<String>,
}

fn read_state(path: &PathBuf) -> Option<PrState> {
    let data = fs::read_to_string(path).ok()?;
    let mut parts = data.splitn(5, '|');
    let number = parts.next()?.to_string();
    let url = parts.next()?.to_string();
    let status = parts.next()?.to_string();
    let pr_state = parts
        .next()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "OPEN".into());
    let merge_commit = parts
        .next()
        .and_then(|s| if s.is_empty() { None } else { Some(s.to_string()) });
    Some(PrState {
        number,
        url,
        status,
        pr_state,
        merge_commit,
    })
}

fn write_state(path: &PathBuf, state: &PrState) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let merge = state.merge_commit.clone().unwrap_or_default();
    let content = format!(
        "{}|{}|{}|{}|{}",
        state.number, state.url, state.status, state.pr_state, merge
    );
    fs::write(path, content).expect("write fake gh state");
}

fn run_git(repo: &PathBuf, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repo)
        .args(args)
        .status()
        .expect("run git");
    if !status.success() {
        eprintln!("git {:?} failed in {}", args, repo.display());
        process::exit(1);
    }
}

fn rev_parse(repo: &PathBuf, rev: &str) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", rev])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        process::exit(1);
    };
    if cmd != "pr" {
        eprintln!("fake gh only supports pr");
        process::exit(1);
    }
    let Some(sub) = args.next() else {
        eprintln!("fake gh missing pr subcommand");
        process::exit(1);
    };
    let state_path = PathBuf::from(
        env::var("SK_TEST_GH_STATE_FILE").expect("missing SK_TEST_GH_STATE_FILE"),
    );
    match sub.as_str() {
        "list" => {
            if let Some(state) = read_state(&state_path) {
                let mergeable = if state.status.eq_ignore_ascii_case("DIRTY") {
                    "CONFLICTING"
                } else {
                    "MERGEABLE"
                };
                println!(
                    "[{{\"number\":{},\"url\":\"{}\",\"mergeStateStatus\":\"{}\",\"mergeable\":\"{}\"}}]",
                    state.number, state.url, state.status, mergeable
                );
            } else {
                println!("[]");
            }
        }
        "create" => {
            let status = env::var("SK_TEST_GH_PR_STATE").unwrap_or_else(|_| "CLEAN".into());
            let url = env::var("SK_TEST_GH_PR_URL")
                .unwrap_or_else(|_| "https://example.test/pr/1".into());
            let number = env::var("SK_TEST_GH_PR_NUMBER").unwrap_or_else(|_| "1".into());
            let state = PrState {
                number: number.clone(),
                url: url.clone(),
                status,
                pr_state: "OPEN".into(),
                merge_commit: None,
            };
            write_state(&state_path, &state);
            println!("{url}");
        }
        "merge" => {
            if let Ok(msg) = env::var("SK_TEST_GH_AUTO_MERGE_ERROR") {
                eprintln!("{msg}");
                process::exit(1);
            }
            let mut state = read_state(&state_path).unwrap_or_else(|| {
                eprintln!("no PR to merge");
                process::exit(1);
            });
            if state.status.eq_ignore_ascii_case("DIRTY") {
                eprintln!("conflict");
                process::exit(1);
            }
            let mut merge_commit = env::var("SK_TEST_GH_MERGE_COMMIT_SHA")
                .ok()
                .filter(|s| !s.is_empty());
            if merge_commit.is_none() {
                merge_commit = env::var("SK_TEST_SYNC_BACK_HEAD_SHA")
                    .ok()
                    .filter(|s| !s.is_empty());
            }
            if let (Ok(repo), Ok(branch)) =
                (env::var("SK_TEST_GH_AUTO_MERGE_REPO"), env::var("SK_TEST_GH_AUTO_MERGE_BRANCH"))
            {
                let repo_path = PathBuf::from(repo);
                run_git(&repo_path, &["fetch", "origin"]);
                run_git(&repo_path, &["checkout", "main"]);
                let target = format!("origin/{branch}");
                run_git(&repo_path, &["merge", "--no-ff", &target, "-m", "Fake auto-merge"]);
                run_git(&repo_path, &["push", "origin", "main"]);
                merge_commit = rev_parse(&repo_path, "HEAD");
            }
            let final_commit = merge_commit.unwrap_or_else(|| "feedface".into());
            state.pr_state = "MERGED".into();
            state.merge_commit = Some(final_commit);
            write_state(&state_path, &state);
            eprintln!("merge ok");
        }
        "view" => {
            let Some(state) = read_state(&state_path) else {
                eprintln!("no PR to view");
                process::exit(1);
            };
            let merged_at = if state.pr_state.eq_ignore_ascii_case("MERGED") {
                "\"2024-01-01T00:00:00Z\"".to_string()
            } else {
                "null".to_string()
            };
            let merge_commit = state
                .merge_commit
                .as_ref()
                .map(|sha| format!("{{\"oid\":\"{}\"}}", sha))
                .unwrap_or_else(|| "null".to_string());
            println!(
                "{{\"number\":{},\"url\":\"{}\",\"state\":\"{}\",\"mergeCommit\":{},\"mergedAt\":{}}}",
                state.number, state.url, state.pr_state, merge_commit, merged_at
            );
        }
        other => {
            eprintln!("fake gh unsupported subcommand: {other}");
            process::exit(1);
        }
    }
}
"#;
