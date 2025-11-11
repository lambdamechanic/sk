use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct RepoSpec {
    pub url: String,
    pub host: String,
    pub owner: String,
    pub repo: String,
}

pub fn ensure_git_repo() -> Result<PathBuf> {
    let root = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git")?;
    if !root.status.success() {
        anyhow::bail!("Not a git repository (run inside a git working tree)");
    }
    let s = String::from_utf8(root.stdout).unwrap_or_default();
    Ok(PathBuf::from(s.trim()))
}

pub fn parse_repo_input(input: &str, https: bool, default_host: &str) -> Result<RepoSpec> {
    // Accept forms: @owner/repo, git@github.com:owner/repo.git, https://github.com/owner/repo(.git)
    if let Some(rest) = input.strip_prefix('@') {
        let (owner, repo) = rest
            .split_once('/')
            .context("expected @owner/repo format")?;
        let url = if https {
            format!("https://{}/{}/{}.git", default_host, owner, repo)
        } else {
            format!("git@{}:{}/{}.git", default_host, owner, repo)
        };
        return Ok(RepoSpec {
            url,
            host: default_host.to_string(),
            owner: owner.to_string(),
            repo: repo.to_string(),
        });
    }
    // ssh short form
    if input.starts_with("git@github.com:") || input.starts_with("git@") {
        // Parse git@host:owner/repo(.git)
        let after_colon = input.split_once(':').context("malformed ssh URL")?.1;
        let host = input
            .split_once('@')
            .context("malformed ssh URL")?
            .1
            .split(':')
            .next()
            .unwrap()
            .to_string();
        let path = after_colon.trim_end_matches(".git");
        let (owner, repo) = path.split_once('/').context("malformed ssh URL path")?;
        return Ok(RepoSpec {
            url: input.to_string(),
            host,
            owner: owner.to_string(),
            repo: repo.to_string(),
        });
    }
    // https
    if input.starts_with("https://") || input.starts_with("http://") || input.starts_with("ssh://")
    {
        let url_s = input.to_string();
        // naive parse of host/owner/repo for cache path
        let no_proto = url_s.split("//").nth(1).unwrap_or("").to_string();
        let mut parts = no_proto.split('/');
        let host_s = parts.next().unwrap_or("").to_string();
        let owner_s = parts.next().unwrap_or("").to_string();
        let repo_s = parts
            .next()
            .unwrap_or("")
            .trim_end_matches(".git")
            .to_string();
        if host_s.is_empty() || owner_s.is_empty() || repo_s.is_empty() {
            bail!("cannot parse repo triplet from URL: {}", url_s);
        }
        return Ok(RepoSpec {
            url: url_s,
            host: host_s,
            owner: owner_s,
            repo: repo_s,
        });
    }
    bail!("Unrecognized repo input: {}", input)
}

pub fn ensure_cached_repo(cache_dir: &Path, spec: &RepoSpec) -> Result<()> {
    if !cache_dir.exists() {
        std::fs::create_dir_all(cache_dir.parent().unwrap_or_else(|| Path::new(".")))?;
        // clone
        let status = Command::new("git")
            .args(["clone", &spec.url, cache_dir.to_string_lossy().as_ref()])
            .status()
            .context("git clone failed")?;
        if !status.success() {
            bail!("git clone failed for {}", spec.url);
        }
    }
    // fetch --prune
    let status = Command::new("git")
        .args(["-C", &cache_dir.to_string_lossy(), "fetch", "--prune"])
        .status()
        .context("git fetch failed")?;
    if !status.success() {
        bail!("git fetch failed for cache: {}", cache_dir.display());
    }
    Ok(())
}

pub fn detect_or_set_default_branch(cache_dir: &Path, remote: &str) -> Result<String> {
    // Try to read origin/HEAD
    let out = Command::new("git")
        .args([
            "-C",
            &cache_dir.to_string_lossy(),
            "symbolic-ref",
            "-q",
            "refs/remotes/origin/HEAD",
        ])
        .output()
        .context("git symbolic-ref failed")?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        // refs/remotes/origin/main -> main
        if let Some(branch) = s.rsplit('/').next() {
            return Ok(branch.to_string());
        }
    }
    // Fallback: query remote default and set it locally
    let ls = Command::new("git")
        .args(["ls-remote", "--symref", remote, "HEAD"])
        .output()
        .context("git ls-remote --symref failed")?;
    if !ls.status.success() {
        bail!("git ls-remote failed for {}", remote);
    }
    let txt = String::from_utf8_lossy(&ls.stdout);
    // Expect a line like: ref: refs/heads/main	HEAD
    let mut branch = None;
    for line in txt.lines() {
        if line.starts_with("ref: ") && line.ends_with("\tHEAD") {
            if let Some(name) = line.split_whitespace().nth(1) {
                branch = name.rsplit('/').next().map(|s| s.to_string());
            }
        }
    }
    let branch = branch.context("unable to determine default branch from ls-remote")?;
    // Set origin/HEAD accordingly
    let _ = Command::new("git")
        .args([
            "-C",
            &cache_dir.to_string_lossy(),
            "remote",
            "set-head",
            "origin",
            &branch,
        ])
        .status();
    Ok(branch)
}

pub fn rev_parse(cache_dir: &Path, rev: &str) -> Result<String> {
    let out = Command::new("git")
        .args([
            "-C",
            &cache_dir.to_string_lossy(),
            "rev-parse",
            "--verify",
            rev,
        ])
        .output()
        .context("git rev-parse failed")?;
    if !out.status.success() {
        bail!("unable to resolve rev: {}", rev);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn remote_branch_tip(cache_dir: &Path, branch: &str) -> Result<Option<String>> {
    // Check if refs/remotes/origin/<branch> exists
    let full = format!("refs/remotes/origin/{}", branch);
    let out = Command::new("git")
        .args([
            "-C",
            &cache_dir.to_string_lossy(),
            "show-ref",
            "--verify",
            &full,
        ])
        .output()
        .context("git show-ref failed")?;
    if !out.status.success() {
        return Ok(None);
    }
    // First field is SHA
    let s = String::from_utf8_lossy(&out.stdout);
    let sha = s.split_whitespace().next().unwrap_or("").to_string();
    Ok(if sha.is_empty() { None } else { Some(sha) })
}

pub fn has_object(cache_dir: &Path, oid: &str) -> Result<bool> {
    let out = Command::new("git")
        .args(["-C", &cache_dir.to_string_lossy(), "cat-file", "-t", oid])
        .output()
        .context("git cat-file failed")?;
    Ok(out.status.success())
}
