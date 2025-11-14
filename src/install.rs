use crate::{config, digest, git, lock, paths, skills};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct InstallArgs<'a> {
    pub repo: &'a str,
    pub skill_name: &'a str,
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
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, &spec)?;
    let default_branch = git::detect_or_set_default_branch(&cache_dir, &spec.url)?;

    // Resolve commit from the remote default branch
    let commit = {
        let rev = format!("refs/remotes/origin/{default_branch}");
        git::rev_parse(&cache_dir, &rev)?
    };

    // Discover skills
    let skills_found = skills::list_skills_in_repo(&cache_dir, &commit)?;
    let chosen = if let Some(path_flag) = args.path {
        let normalized = normalize_skill_subdir(path_flag);
        pick_skill_by_path(
            &skills_found,
            &cache_dir,
            &commit,
            &normalized,
            args.skill_name,
        )?
    } else {
        pick_skill_by_name(&skills_found, args.skill_name, &spec)?
    };

    let install_name = args.alias.unwrap_or(&chosen.meta.name);
    let dest = install_root.join(install_name);
    if dest.exists() {
        let dest_s = dest.display().to_string();
        bail!("Install destination '{dest_s}' already exists");
    }

    // Extract subdir from commit to dest via git archive | tar
    fs::create_dir_all(&dest)?;
    let strip_components = chosen.skill_path.split('/').count();
    let mut archive = Command::new("git")
        .args([
            "-C",
            &cache_dir.to_string_lossy(),
            "archive",
            "--format=tar",
            &commit,
            &chosen.skill_path,
        ])
        .stdout(Stdio::piped())
        .spawn()
        .context("spawn git archive failed")?;
    let mut tar = Command::new("tar")
        .args([
            "-x",
            "--strip-components",
            &strip_components.to_string(),
            "-C",
            &dest.to_string_lossy(),
        ])
        .stdin(archive.stdout.take().unwrap())
        .spawn()
        .context("spawn tar failed")?;
    let st1 = archive.wait()?;
    let st2 = tar.wait()?;
    if !st1.success() || !st2.success() {
        bail!("failed to extract skill contents");
    }

    // Compute digest
    let digest = digest::digest_dir(&dest)?;

    // Update lockfile
    let lock_path = project_root.join("skills.lock.json");
    let mut lf = lock::Lockfile::load_or_empty(&lock_path)?;

    if lf.skills.iter().any(|s| s.install_name == install_name) {
        bail!("Lockfile already contains skill with installName '{install_name}'");
    }

    let entry = lock::LockSkill {
        install_name: install_name.to_string(),
        source: lock::Source {
            url: spec.url.clone(),
            host: spec.host.clone(),
            owner: spec.owner.clone(),
            repo: spec.repo.clone(),
            skill_path: chosen.skill_path.clone(),
        },
        legacy_ref: None,
        commit: commit.clone(),
        digest: digest.clone(),
        installed_at: Utc::now().to_rfc3339(),
    };
    lf.skills.push(entry);
    lf.generated_at = Utc::now().to_rfc3339();
    crate::lock::save_lockfile(&lock_path, &lf)?;

    let dest_s = dest.display().to_string();
    println!("Installed '{install_name}' to {dest_s} @ {}", &commit[..7]);
    Ok(())
}

fn pick_skill_by_name(
    skills_found: &[skills::DiscoveredSkill],
    requested_name: &str,
    spec: &git::RepoSpec,
) -> Result<skills::DiscoveredSkill> {
    let mut candidates: Vec<_> = skills_found
        .iter()
        .filter(|s| s.meta.name == requested_name)
        .cloned()
        .collect();
    if candidates.is_empty() {
        bail!(
            "No skill named '{requested_name}' found in {}",
            repo_identifier(spec)
        );
    }
    if candidates.len() > 1 {
        let paths = candidates
            .iter()
            .map(|s| s.skill_path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "Multiple skills named '{requested_name}' found in {}. Re-run with --path one of: {paths}",
            repo_identifier(spec)
        );
    }
    Ok(candidates.remove(0))
}

fn pick_skill_by_path(
    skills_found: &[skills::DiscoveredSkill],
    cache_dir: &Path,
    commit: &str,
    normalized_path: &str,
    requested_name: &str,
) -> Result<skills::DiscoveredSkill> {
    if let Some(skill) = skills_found
        .iter()
        .find(|s| s.skill_path == normalized_path)
    {
        ensure_skill_name_matches(&skill.meta.name, requested_name, normalized_path)?;
        return Ok(skill.clone());
    }
    let meta = load_skill_meta_from_path(cache_dir, commit, normalized_path)?;
    ensure_skill_name_matches(&meta.name, requested_name, normalized_path)?;
    Ok(skills::DiscoveredSkill {
        skill_path: normalized_path.to_string(),
        meta,
    })
}

fn ensure_skill_name_matches(actual: &str, requested: &str, subdir: &str) -> Result<()> {
    if actual != requested {
        let display = skill_dir_display(subdir);
        bail!(
            "Skill at '{}' is named '{}', but you requested '{}'. Update the SKILL.md or use the actual skill name.",
            display,
            actual,
            requested
        );
    }
    Ok(())
}

fn load_skill_meta_from_path(
    cache_dir: &Path,
    commit: &str,
    subdir: &str,
) -> Result<skills::SkillMeta> {
    let rel = skill_md_rel_path(subdir);
    let object = format!("{commit}:{rel}");
    let content = Command::new("git")
        .args(["-C", &cache_dir.to_string_lossy(), "show", &object])
        .output()
        .context("git show failed")?;
    if !content.status.success() {
        bail!(skill_not_found_message(&rel));
    }
    let text = String::from_utf8_lossy(&content.stdout);
    match skills::parse_skill_frontmatter_str(&text) {
        Ok(meta) => Ok(meta),
        Err(_) => bail!(skill_not_found_message(&rel)),
    }
}

fn normalize_skill_subdir(input: &str) -> String {
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

fn skill_md_rel_path(subdir: &str) -> String {
    if subdir == "." {
        "SKILL.md".to_string()
    } else {
        format!("{subdir}/SKILL.md")
    }
}

fn skill_not_found_message(rel: &str) -> String {
    format!(
        "'{rel}' not found or invalid. A Claude Skill must contain SKILL.md with 'name' and 'description'."
    )
}

fn skill_dir_display(subdir: &str) -> String {
    if subdir == "." {
        ".".to_string()
    } else {
        subdir.to_string()
    }
}

fn repo_identifier(spec: &git::RepoSpec) -> String {
    if spec.owner.is_empty() {
        spec.repo.clone()
    } else {
        format!("{}/{}", spec.owner, spec.repo)
    }
}

pub fn extract_subdir_from_commit(
    cache_dir: &Path,
    commit: &str,
    subdir: &str,
    dest: &Path,
) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let strip_components = subdir.split('/').count();
    let mut archive = Command::new("git")
        .args([
            "-C",
            &cache_dir.to_string_lossy(),
            "archive",
            "--format=tar",
            commit,
            subdir,
        ])
        .stdout(Stdio::piped())
        .spawn()
        .context("spawn git archive failed")?;
    let mut tar = Command::new("tar")
        .args([
            "-x",
            "--strip-components",
            &strip_components.to_string(),
            "-C",
            &dest.to_string_lossy(),
        ])
        .stdin(archive.stdout.take().unwrap())
        .spawn()
        .context("spawn tar failed")?;
    let st1 = archive.wait()?;
    let st2 = tar.wait()?;
    if !st1.success() || !st2.success() {
        anyhow::bail!("failed to extract skill contents");
    }
    Ok(())
}
