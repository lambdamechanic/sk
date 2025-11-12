use crate::{git, lock};
use anyhow::{bail, Context, Result};
use std::fs;

pub fn run_precommit(allow_local: bool) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        // No lockfile; nothing to check.
        return Ok(());
    }
    let data = fs::read(&lock_path).with_context(|| format!("reading {}", lock_path.display()))?;
    let lf: lock::Lockfile = serde_json::from_slice(&data).context("parse lockfile")?;

    let mut local_entries: Vec<String> = vec![];
    for s in &lf.skills {
        let url = s.source.url.as_str();
        let is_local = is_local_source(url, &s.source.host);
        if is_local {
            local_entries.push(format!(
                "{} -> {} (path: {})",
                s.install_name, url, s.source.skill_path
            ));
        }
    }

    if !local_entries.is_empty() {
        eprintln!(
            "sk precommit: detected local (file:// or localhost) sources in skills.lock.json:"
        );
        for e in &local_entries {
            eprintln!("  - {e}");
        }
        eprintln!(
            "These entries will not be usable by collaborators. Replace with ssh/https URLs, or run with --allow-local to bypass."
        );
        if !allow_local {
            bail!("local sources present; failing precommit");
        }
    }
    Ok(())
}

fn is_local_source(url: &str, host_field: &str) -> bool {
    // 1) Explicit sentinel from lock entry for file:// repos
    if host_field == "local" {
        return true;
    }
    let u = url.to_ascii_lowercase();
    // 2) file:// scheme
    if u.starts_with("file://") {
        return true;
    }
    // 3) Extract host from common URL forms and match exact localhost/loopback
    // http(s)://host[:port]/...
    if let Some(rest) = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
    {
        return host_is_local(extract_netloc(rest));
    }
    // ssh://host[:port]/...
    if let Some(rest) = u.strip_prefix("ssh://") {
        return host_is_local(extract_netloc(rest));
    }
    // scp-like: <user>@<host>:owner/repo (support any user, IPv6-in-brackets)
    if let Some((_, after_at)) = u.split_once('@') {
        if let Some((host_part, _path)) = after_at.split_once(':') {
            let host = if host_part.starts_with('[') && host_part.ends_with(']') {
                &host_part[1..host_part.len() - 1]
            } else {
                host_part
            };
            return host_is_local(host.to_string());
        }
    }
    false
}

fn extract_netloc(rest: &str) -> String {
    // take up to first '/'
    let host_port = rest.split('/').next().unwrap_or("").to_string();
    // strip brackets for IPv6 like [::1]:22
    if host_port.starts_with('[') {
        if let Some(end) = host_port.find(']') {
            return host_port[1..end].to_string();
        }
    }
    // drop :port
    if let Some((h, _port)) = host_port.split_once(':') {
        return h.to_string();
    }
    host_port
}

fn host_is_local(host: String) -> bool {
    matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1")
}
