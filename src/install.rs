use crate::{config, digest, git, lock, paths, skills};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct InstallArgs<'a> {
    pub repo: &'a str,
    pub skill_name: &'a str,
    pub r#ref: Option<&'a str>,
    pub alias: Option<&'a str>,
    pub path: Option<&'a str>,
    pub root: Option<&'a str>,
    pub https: bool,
}

pub fn run_install(args: InstallArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let root_rel = args.root.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, root_rel);
    fs::create_dir_all(&install_root)?;

    // Parse repo and ensure cache
    let spec = git::parse_repo_input(args.repo, args.https, &cfg.default_host)?;
    let cache_dir = paths::cache_repo_path(&spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, &spec)?;
    let default_branch = git::detect_or_set_default_branch(&cache_dir, &spec.url)?;

    // Resolve commit
    let commit = match args.r#ref {
        Some(r) => {
            // Try origin/<branch>, then tag or sha
            let try1 = git::rev_parse(&cache_dir, &format!("origin/{}", r));
            if let Ok(c1) = try1 { c1 } else { git::rev_parse(&cache_dir, r)? }
        }
        None => {
            let rev = format!("refs/remotes/origin/{}", default_branch);
            git::rev_parse(&cache_dir, &rev)?
        }
    };

    // Discover skills
    let skills_found = skills::list_skills_in_repo(&cache_dir, &commit)?;
    let mut candidates: Vec<_> = skills_found
        .into_iter()
        .filter(|s| s.meta.name == args.skill_name)
        .collect();
    if candidates.is_empty() { bail!("No skill named '{}' found in repo {}", args.skill_name, spec.url); }
    let chosen = if candidates.len() > 1 {
        if let Some(p) = args.path { 
            let norm = p.trim_matches('/');
            candidates.into_iter().find(|s| s.skill_path == norm).context("--path did not match any candidate")?
        } else {
            let paths = candidates.iter().map(|s| s.skill_path.as_str()).collect::<Vec<_>>().join(", ");
            bail!("Multiple skills named '{}' found. Re-run with --path one of: {}", args.skill_name, paths);
        }
    } else {
        candidates.remove(0)
    };

    let install_name = args.alias.unwrap_or(&chosen.meta.name);
    let dest = install_root.join(install_name);
    if dest.exists() {
        bail!("Install destination '{}' already exists", dest.display());
    }

    // Extract subdir from commit to dest via git archive | tar
    fs::create_dir_all(&dest)?;
    let strip_components = chosen.skill_path.split('/').count();
    let mut archive = Command::new("git")
        .args(["-C", &cache_dir.to_string_lossy(), "archive", "--format=tar", &commit, &chosen.skill_path])
        .stdout(Stdio::piped())
        .spawn()
        .context("spawn git archive failed")?;
    let mut tar = Command::new("tar")
        .args(["-x", "--strip-components", &strip_components.to_string(), "-C", &dest.to_string_lossy()])
        .stdin(archive.stdout.take().unwrap())
        .spawn()
        .context("spawn tar failed")?;
    let st1 = archive.wait()?; let st2 = tar.wait()?;
    if !st1.success() || !st2.success() { bail!("failed to extract skill contents"); }

    // Compute digest
    let digest = digest::digest_dir(&dest)?;

    // Update lockfile
    let lock_path = project_root.join("skills.lock.json");
    let mut lf = if lock_path.exists() {
        let data = fs::read(&lock_path)?; serde_json::from_slice::<lock::Lockfile>(&data)?
    } else { lock::Lockfile::empty_now() };

    if lf.skills.iter().any(|s| s.installName == install_name) {
        bail!("Lockfile already contains skill with installName '{}'", install_name);
    }

    let ref_field = args.r#ref.map(|s| s.to_string());
    let entry = lock::LockSkill {
        installName: install_name.to_string(),
        source: lock::Source { url: spec.url.clone(), host: spec.host.clone(), owner: spec.owner.clone(), repo: spec.repo.clone(), skillPath: chosen.skill_path.clone() },
        ref_: ref_field,
        commit: commit.clone(),
        digest: digest.clone(),
        installedAt: Utc::now().to_rfc3339(),
    };
    lf.skills.push(entry);
    lf.generatedAt = Utc::now().to_rfc3339();
    crate::lock::save_lockfile(&lock_path, &lf)?;

    println!("Installed '{}' to {} @ {}", install_name, dest.display(), &commit[..7]);
    Ok(())
}

pub fn extract_subdir_from_commit(cache_dir: &Path, commit: &str, subdir: &str, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let strip_components = subdir.split('/').count();
    let mut archive = Command::new("git")
        .args(["-C", &cache_dir.to_string_lossy(), "archive", "--format=tar", commit, subdir])
        .stdout(Stdio::piped())
        .spawn()
        .context("spawn git archive failed")?;
    let mut tar = Command::new("tar")
        .args(["-x", "--strip-components", &strip_components.to_string(), "-C", &dest.to_string_lossy()])
        .stdin(archive.stdout.take().unwrap())
        .spawn()
        .context("spawn tar failed")?;
    let st1 = archive.wait()?; let st2 = tar.wait()?;
    if !st1.success() || !st2.success() { anyhow::bail!("failed to extract skill contents"); }
    Ok(())
}
