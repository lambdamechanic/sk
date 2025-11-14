use super::short_sha;
use super::target::SyncTarget;
use crate::git;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::env;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub(crate) struct PrAutomationReport {
    pub(crate) pr: GhPrInfo,
    pub(crate) auto_merge_armed: bool,
}

pub(super) fn automate_pr_flow(
    wt_path: &Path,
    branch_name: &str,
    spec: &git::RepoSpec,
) -> Result<Option<PrAutomationReport>> {
    if which::which("gh").is_err() {
        println!(
            "Skipping PR automation: 'gh' CLI not found. Install https://cli.github.com/ to auto-open PRs."
        );
        return Ok(None);
    }

    let repo_selector = format_repo_selector(spec);
    let (pr, created) = ensure_pull_request(&repo_selector, wt_path, branch_name)?;
    if created {
        println!("Opened PR {} for branch '{branch_name}'.", pr.url);
    } else {
        println!("Reusing PR {} for branch '{branch_name}'.", pr.url);
    }

    let outcome = auto_merge_pull_request(&repo_selector, wt_path, &pr);
    match &outcome {
        AutoMergeOutcome::Armed => {
            println!(
                "Auto-merge armed; GitHub will land {} once required checks pass.",
                pr.url
            );
        }
        AutoMergeOutcome::Conflict => {
            println!(
                "Auto-merge blocked by conflicts. Resolve manually: {}",
                pr.url
            );
        }
        AutoMergeOutcome::Skipped(reason) => {
            println!("Auto-merge skipped for {} ({reason}).", pr.url);
            if let Some(tip) = auto_merge_tip(reason, spec) {
                println!("{tip}");
            }
        }
    }

    Ok(Some(PrAutomationReport {
        pr,
        auto_merge_armed: matches!(outcome, AutoMergeOutcome::Armed),
    }))
}

pub(super) fn maybe_wait_for_auto_merge(
    target: &SyncTarget,
    report: &PrAutomationReport,
    pushed_head: &str,
    install_name: &str,
) -> Result<String> {
    if !report.auto_merge_armed {
        return Ok(pushed_head.to_string());
    }
    let timeout = auto_merge_timeout();
    let poll = auto_merge_poll_interval();
    let seconds = timeout.as_secs();
    println!(
        "Auto-merge armed; polling {} for the merged commit (up to {}s)...",
        report.pr.url,
        if seconds == 0 {
            timeout.as_millis() as u64 / 1000
        } else {
            seconds
        }
    );
    match wait_for_merge_commit(target, report, timeout, poll) {
        Ok(Some(commit)) => {
            println!(
                "Auto-merge landed commit {}. Updating lockfile.",
                short_sha(&commit)
            );
            Ok(commit)
        }
        Ok(None) => {
            println!(
                "Auto-merge for '{}' has not finished yet; keeping lock at {}. Run 'sk upgrade {}' after the PR merges.",
                install_name,
                short_sha(pushed_head),
                install_name
            );
            Ok(pushed_head.to_string())
        }
        Err(err) => {
            eprintln!(
                "Warning: unable to confirm merged commit for '{}': {err:#}. Keeping lock at {}.",
                install_name,
                short_sha(pushed_head)
            );
            Ok(pushed_head.to_string())
        }
    }
}

fn ensure_pull_request(
    repo_selector: &str,
    wt_path: &Path,
    branch_name: &str,
) -> Result<(GhPrInfo, bool)> {
    if let Some(existing) = find_existing_pr(repo_selector, wt_path, branch_name)? {
        return Ok((existing, false));
    }
    create_pull_request(repo_selector, wt_path, branch_name)?;
    let created = find_existing_pr(repo_selector, wt_path, branch_name)?
        .ok_or_else(|| anyhow!("gh pr create succeeded but no PR was found"))?;
    Ok((created, true))
}

fn find_existing_pr(
    repo_selector: &str,
    wt_path: &Path,
    branch_name: &str,
) -> Result<Option<GhPrInfo>> {
    let out = Command::new("gh")
        .current_dir(wt_path)
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--head",
            branch_name,
            "--limit",
            "1",
            "--json",
            "number,url,mergeStateStatus,mergeable",
            "-R",
            repo_selector,
        ])
        .output()
        .context("run gh pr list")?;
    if !out.status.success() {
        bail!(
            "gh pr list failed: {}",
            format_gh_failure(&out.stdout, &out.stderr)
        );
    }
    let mut entries: Vec<GhPrInfo> =
        serde_json::from_slice(&out.stdout).context("parse gh pr list JSON output")?;
    Ok(entries.pop())
}

fn create_pull_request(repo_selector: &str, wt_path: &Path, branch_name: &str) -> Result<()> {
    let out = Command::new("gh")
        .current_dir(wt_path)
        .args([
            "pr",
            "create",
            "--fill",
            "--head",
            branch_name,
            "-R",
            repo_selector,
        ])
        .output()
        .context("run gh pr create")?;
    if !out.status.success() {
        bail!(
            "gh pr create failed: {}",
            format_gh_failure(&out.stdout, &out.stderr)
        );
    }
    Ok(())
}

fn auto_merge_pull_request(repo_selector: &str, wt_path: &Path, pr: &GhPrInfo) -> AutoMergeOutcome {
    if is_pr_conflicted(pr) {
        return AutoMergeOutcome::Conflict;
    }

    let number = pr.number.to_string();
    match Command::new("gh")
        .current_dir(wt_path)
        .args([
            "pr",
            "merge",
            &number,
            "--auto",
            "--merge",
            "-R",
            repo_selector,
        ])
        .output()
    {
        Ok(out) if out.status.success() => AutoMergeOutcome::Armed,
        Ok(out) => AutoMergeOutcome::Skipped(format_gh_failure(&out.stdout, &out.stderr)),
        Err(err) => AutoMergeOutcome::Skipped(err.to_string()),
    }
}

fn is_pr_conflicted(pr: &GhPrInfo) -> bool {
    matches!(
        pr.merge_state_status
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("dirty")),
        Some(true)
    ) || matches!(
        pr.mergeable
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("conflicting")),
        Some(true)
    )
}

fn format_repo_selector(spec: &git::RepoSpec) -> String {
    if spec.host.is_empty() {
        format!("{}/{}", spec.owner, spec.repo)
    } else {
        format!("{}/{}/{}", spec.host, spec.owner, spec.repo)
    }
}

fn auto_merge_tip(reason: &str, spec: &git::RepoSpec) -> Option<String> {
    if !reason
        .to_ascii_lowercase()
        .contains("enablepullrequestautomerge")
    {
        return None;
    }

    let repo_slug = format!("{}/{}", spec.owner, spec.repo);
    let cmd = if spec.host.is_empty() || spec.host.eq_ignore_ascii_case("github.com") {
        format!("gh repo edit {repo_slug} --enable-auto-merge")
    } else {
        format!(
            "gh repo edit -R {}/{repo_slug} --enable-auto-merge",
            spec.host
        )
    };
    let host = if spec.host.is_empty() {
        "github.com"
    } else {
        spec.host.as_str()
    };
    let settings_url = format!("https://{host}/{repo_slug}/settings");
    Some(format!(
        "Tip: enable auto-merge with `{cmd}` or toggle Auto-merge under Settings → General ({settings_url})."
    ))
}

fn wait_for_merge_commit(
    target: &SyncTarget,
    report: &PrAutomationReport,
    timeout: Duration,
    poll: Duration,
) -> Result<Option<String>> {
    let repo_selector = format_repo_selector(&target.spec);
    let start = Instant::now();
    loop {
        let status = query_pr_merge_status(&repo_selector, report.pr.number)?;
        if status.is_merged() {
            if let Some(commit) = status.merge_commit_oid() {
                git::ensure_cached_repo(&target.cache_dir, &target.spec)?;
                if git::has_object(&target.cache_dir, &commit)? {
                    return Ok(Some(commit));
                }
            }
        } else if status.is_closed() {
            return Ok(None);
        }
        if start.elapsed() >= timeout {
            return Ok(None);
        }
        if poll.is_zero() {
            thread::yield_now();
        } else {
            thread::sleep(poll);
        }
    }
}

fn query_pr_merge_status(repo_selector: &str, number: u64) -> Result<GhPrMergeStatus> {
    let out = Command::new("gh")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--json",
            "state,mergeCommit",
            "-R",
            repo_selector,
        ])
        .output()
        .context("run gh pr view")?;
    if !out.status.success() {
        bail!(
            "gh pr view failed: {}",
            format_gh_failure(&out.stdout, &out.stderr)
        );
    }
    let status: GhPrMergeStatus =
        serde_json::from_slice(&out.stdout).context("parse gh pr view JSON output")?;
    Ok(status)
}

fn auto_merge_timeout() -> Duration {
    duration_from_env_ms("SK_SYNC_BACK_AUTO_MERGE_TIMEOUT_MS", 120_000)
}

fn auto_merge_poll_interval() -> Duration {
    duration_from_env_ms("SK_SYNC_BACK_AUTO_MERGE_POLL_MS", 2_000)
}

fn duration_from_env_ms(key: &str, default_ms: u64) -> Duration {
    env::var(key)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(default_ms))
}

#[derive(Debug, Deserialize)]
pub(crate) struct GhPrInfo {
    pub(crate) number: u64,
    pub(crate) url: String,
    #[serde(rename = "mergeStateStatus")]
    pub(crate) merge_state_status: Option<String>,
    pub(crate) mergeable: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhPrMergeStatus {
    state: Option<String>,
    #[serde(rename = "mergeCommit")]
    merge_commit: Option<JsonValue>,
}

impl GhPrMergeStatus {
    fn is_merged(&self) -> bool {
        matches!(
            self.state
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case("merged")),
            Some(true)
        )
    }

    fn is_closed(&self) -> bool {
        matches!(
            self.state
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case("closed")),
            Some(true)
        )
    }

    fn merge_commit_oid(&self) -> Option<String> {
        self.merge_commit.as_ref().and_then(|value| match value {
            JsonValue::String(s) if !s.is_empty() => Some(s.clone()),
            JsonValue::Object(map) => map
                .get("oid")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    map.get("sha")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                }),
            _ => None,
        })
    }
}

enum AutoMergeOutcome {
    Armed,
    Conflict,
    Skipped(String),
}

fn format_gh_failure(stdout: &[u8], stderr: &[u8]) -> String {
    let mut combined = String::new();
    if !stderr.is_empty() {
        combined.push_str(&String::from_utf8_lossy(stderr));
    }
    if !stdout.is_empty() {
        combined.push_str(&String::from_utf8_lossy(stdout));
    }
    let trimmed = combined.trim();
    if trimmed.is_empty() {
        "gh command failed".to_string()
    } else {
        trimmed.to_string()
    }
}
