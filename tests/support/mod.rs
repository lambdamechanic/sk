#![allow(dead_code)]

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command as AssertCommand;
use serde::Deserialize;
use serde_json::Value as Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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

fn path_to_file_url(p: &Path) -> String {
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
