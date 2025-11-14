use anyhow::{anyhow, bail, Context, Result};
use gix_url as gurl;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoSpec {
    pub url: String,
    pub host: String,
    pub owner: String,
    pub repo: String,
}

impl RepoSpec {
    fn https_fallback(&self) -> Option<String> {
        if self.host.is_empty()
            || self.host == "local"
            || self.owner.is_empty()
            || self.repo.is_empty()
        {
            return None;
        }
        Some(format!(
            "https://{}/{}/{}.git",
            self.host, self.owner, self.repo
        ))
    }

    fn clone_candidates(&self) -> Vec<String> {
        let mut urls = vec![self.url.clone()];
        if let Some(fallback) = self.https_fallback() {
            if fallback != self.url {
                urls.push(fallback);
            }
        }
        urls
    }
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
    // Shorthand: @owner/repo -> choose https or ssh on default_host
    if let Some(rest) = input.strip_prefix('@') {
        let (owner, repo) = rest
            .split_once('/')
            .context("expected @owner/repo format")?;
        let url = if https {
            format!("https://{default_host}/{owner}/{repo}.git")
        } else {
            format!("git@{default_host}:{owner}/{repo}.git")
        };
        return Ok(RepoSpec {
            url,
            host: default_host.to_string(),
            owner: owner.to_string(),
            repo: repo.to_string(),
        });
    }
    // Use gix-url for all other forms (ssh, scp-like, https/http, file)
    let parsed =
        gurl::Url::try_from(input).map_err(|_| anyhow::anyhow!("unable to parse repo URL"))?;
    match parsed.scheme {
        gurl::Scheme::File => {
            // Derive owner/repo from filesystem path: parent-dir and basename (without .git)
            // Path is stored as bytes and typically starts with '/'
            use std::path::Path;
            let path_bytes: &[u8] = parsed.path.as_ref();
            let path_str: Cow<'_, str> = String::from_utf8_lossy(path_bytes);
            let p = Path::new(path_str.as_ref());
            let repo_name = p.file_name().and_then(|s| s.to_str()).unwrap_or("repo");
            let repo = repo_name.trim_end_matches(".git").to_string();
            let owner = p
                .parent()
                .and_then(|pp| pp.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("local")
                .to_string();
            Ok(RepoSpec {
                url: input.to_string(),
                host: "local".to_string(),
                owner,
                repo,
            })
        }
        _ => {
            // owner/repo come from the URL path; trim leading '/', drop trailing .git
            let host = parsed
                .host()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("missing host in URL"))?;
            let path_bytes: &[u8] = parsed.path.as_ref();
            let path_s = String::from_utf8_lossy(path_bytes);
            let mut comps = path_s.trim_start_matches('/').split('/');
            let owner = comps.next().unwrap_or("").to_string();
            let repo = comps
                .next()
                .unwrap_or("")
                .trim_end_matches(".git")
                .to_string();
            if owner.is_empty() || repo.is_empty() {
                bail!("cannot parse owner/repo from URL path: {path_s}");
            }
            Ok(RepoSpec {
                url: input.to_string(),
                host,
                owner,
                repo,
            })
        }
    }
}

pub fn ensure_cached_repo(cache_dir: &Path, spec: &RepoSpec) -> Result<()> {
    if !cache_dir.exists() {
        std::fs::create_dir_all(cache_dir.parent().unwrap_or_else(|| Path::new(".")))?;
        let mut clone_err: Option<anyhow::Error> = None;
        let mut cloned = false;
        for remote in spec.clone_candidates() {
            let status = Command::new("git")
                .args(["clone", &remote, cache_dir.to_string_lossy().as_ref()])
                .status()
                .context("git clone failed")?;
            if status.success() {
                cloned = true;
                break;
            } else {
                clone_err = Some(anyhow::anyhow!(
                    "git clone failed for {} (status {})",
                    remote,
                    status
                ));
            }
        }
        if !cloned {
            return Err(
                clone_err.unwrap_or_else(|| anyhow::anyhow!("git clone failed for {}", spec.url))
            );
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

fn read_origin_head(cache_dir: &Path) -> Result<Option<String>> {
    let cache = cache_dir.to_string_lossy();
    let out = Command::new("git")
        .args([
            "-C",
            &cache,
            "symbolic-ref",
            "-q",
            "refs/remotes/origin/HEAD",
        ])
        .output()
        .context("git symbolic-ref failed")?;
    if !out.status.success() {
        return Ok(None);
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let branch = s.rsplit('/').next().unwrap_or("").to_string();
    if branch.is_empty() {
        return Ok(None);
    }
    let verify = Command::new("git")
        .args([
            "-C",
            &cache,
            "show-ref",
            "--verify",
            &format!("refs/remotes/origin/{branch}"),
        ])
        .output()
        .context("git show-ref failed")?;
    if !verify.status.success() {
        return Ok(None);
    }
    Ok(Some(branch))
}

fn query_remote_default_branch(remote: &str) -> Result<String> {
    let ls = Command::new("git")
        .args(["ls-remote", "--symref", remote, "HEAD"])
        .output()
        .context("git ls-remote --symref failed")?;
    if !ls.status.success() {
        bail!("git ls-remote failed for {remote}");
    }
    let txt = String::from_utf8_lossy(&ls.stdout);
    for line in txt
        .lines()
        .filter(|l| l.starts_with("ref: ") && l.ends_with("\tHEAD"))
    {
        let Some(name) = line.split_whitespace().nth(1) else {
            continue;
        };
        let Some(branch) = name.rsplit('/').next() else {
            continue;
        };
        if branch.is_empty() {
            continue;
        }
        return Ok(branch.to_string());
    }
    bail!("unable to determine default branch from ls-remote output for {remote}");
}

fn set_origin_head(cache_dir: &Path, branch: &str) -> Result<()> {
    let cache = cache_dir.to_string_lossy();
    let out = Command::new("git")
        .args(["-C", &cache, "remote", "set-head", "origin", branch])
        .output()
        .context("git remote set-head failed")?;
    if !out.status.success() {
        bail!("git remote set-head origin {branch} failed");
    }
    Ok(())
}

pub fn refresh_default_branch(cache_dir: &Path, spec: &RepoSpec) -> Result<String> {
    let mut last_err: Option<anyhow::Error> = None;
    for remote in spec.clone_candidates() {
        match query_remote_default_branch(&remote) {
            Ok(branch) => {
                set_origin_head(cache_dir, &branch)?;
                return Ok(branch);
            }
            Err(err) => {
                last_err = Some(err);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("unable to determine default branch for {}", spec.url)))
}

pub fn detect_or_set_default_branch(cache_dir: &Path, spec: &RepoSpec) -> Result<String> {
    if let Some(branch) = read_origin_head(cache_dir)? {
        return Ok(branch);
    }
    refresh_default_branch(cache_dir, spec)
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
        bail!("unable to resolve rev: {rev}");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn has_object(cache_dir: &Path, oid: &str) -> Result<bool> {
    let out = Command::new("git")
        .args(["-C", &cache_dir.to_string_lossy(), "cat-file", "-t", oid])
        .output()
        .context("git cat-file failed")?;
    Ok(out.status.success())
}
