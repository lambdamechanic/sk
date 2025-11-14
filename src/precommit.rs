use crate::{git, lock};
use anyhow::{bail, Result};
use gix_url as gurl;

pub fn run_precommit(allow_local: bool) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        // No lockfile; nothing to check.
        return Ok(());
    }
    let lf = lock::Lockfile::load(&lock_path)?;

    let mut local_entries: Vec<String> = vec![];
    for s in &lf.skills {
        let spec = s.source.repo_spec();
        let url = spec.url.as_str();
        let is_local = is_local_source(url, &spec.host);
        if is_local {
            local_entries.push(format!(
                "{} -> {} (path: {})",
                s.install_name,
                url,
                s.source.skill_path()
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
    if host_field == "local" {
        return true;
    }
    match infer_source_kind(url) {
        SourceKind::LocalFile => true,
        SourceKind::RemoteHost(host) => host_is_local(&host),
        SourceKind::Unknown => false,
    }
}

fn extract_netloc(rest: &str) -> String {
    // Take up to first '/'
    let mut host_port = rest.split('/').next().unwrap_or("").to_string();
    // Drop optional userinfo@ prefix (e.g., user@host or user@[::1])
    if let Some(idx) = host_port.rfind('@') {
        host_port = host_port[idx + 1..].to_string();
    }
    // Strip brackets for IPv6 like [::1]:22
    if host_port.starts_with('[') {
        if let Some(end) = host_port.find(']') {
            return host_port[1..end].to_string();
        }
    }
    // Drop :port
    if let Some((h, _port)) = host_port.split_once(':') {
        return h.to_string();
    }
    host_port.to_ascii_lowercase()
}

fn host_is_local(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn infer_source_kind(url: &str) -> SourceKind {
    let lowered = url.trim().to_ascii_lowercase();
    if let Some(kind) = parse_with_gix(&lowered) {
        return kind;
    }
    if lowered.starts_with("file://") {
        return SourceKind::LocalFile;
    }
    if let Some(host) = host_from_prefix(&lowered) {
        return SourceKind::RemoteHost(host);
    }
    if let Some(host) = parse_scp_like(&lowered) {
        return SourceKind::RemoteHost(host);
    }
    SourceKind::Unknown
}

fn parse_with_gix(url: &str) -> Option<SourceKind> {
    let parsed = gurl::Url::try_from(url).ok()?;
    if matches!(parsed.scheme, gurl::Scheme::File) {
        return Some(SourceKind::LocalFile);
    }
    parsed
        .host()
        .map(|h| SourceKind::RemoteHost(h.to_string().to_ascii_lowercase()))
}

fn host_from_prefix(url: &str) -> Option<String> {
    for scheme in ["https://", "http://", "ssh://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            return Some(extract_netloc(rest));
        }
    }
    None
}

fn parse_scp_like(url: &str) -> Option<String> {
    if url.contains("://") {
        return None;
    }
    let (host_part, _) = url.split_once(':')?;
    let maybe_host = host_part
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(host_part);
    if is_windows_drive(maybe_host) {
        return None;
    }
    let trimmed = maybe_host.trim_matches(|c| c == '[' || c == ']');
    Some(trimmed.to_ascii_lowercase())
}

fn is_windows_drive(segment: &str) -> bool {
    segment.len() == 1 && segment.as_bytes()[0].is_ascii_alphabetic()
}

enum SourceKind {
    LocalFile,
    RemoteHost(String),
    Unknown,
}
