use super::{cache, manifest, report::SkillReport, update};
use crate::{config, digest, git, install, lock, paths, skills};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use owo_colors::OwoColorize;
use serde::Serialize;
use serde_json;
use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::thread;

pub enum DoctorMode {
    Diagnose,
    Summary,
    Status,
    Diff,
}

pub struct DoctorArgs<'a> {
    pub names: &'a [String],
    pub root: Option<&'a str>,
    pub mode: DoctorMode,
    pub json: bool,
    pub apply: bool,
}

pub fn run_doctor(args: DoctorArgs) -> Result<()> {
    match args.mode {
        DoctorMode::Diagnose => run_diagnose(args),
        DoctorMode::Summary => run_summary(args),
        DoctorMode::Status => run_status(args),
        DoctorMode::Diff => run_diff(args),
    }
}

fn run_diagnose(args: DoctorArgs) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        println!("No lockfile found.");
        return Ok(());
    }

    let cfg = config::load_or_default()?;
    let install_root_rel = args.root.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, install_root_rel);
    let lockfile = lock::Lockfile::load(&lock_path)?;
    let mut state = DoctorState::new(args.apply, lock_path, install_root, lockfile, args.names);

    state.check_duplicate_install_names();
    state.inspect_skills();
    state.print_cache_messages();
    state.apply_lockfile_repairs()?;
    state.finish();

    Ok(())
}

fn run_summary(args: DoctorArgs) -> Result<()> {
    let ctx = load_project_context(args.root)?;
    let targets = select_skills(&ctx.lockfile.skills, args.names);
    let entries: Vec<CheckEntry> = targets
        .into_iter()
        .map(|skill| build_check_entry(&ctx.install_root, skill))
        .collect();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        for entry in entries {
            println!("{}\t{}", entry.install_name, entry.state);
        }
    }
    Ok(())
}

fn run_status(args: DoctorArgs) -> Result<()> {
    let ctx = load_project_context(args.root)?;
    let targets = select_skills(&ctx.lockfile.skills, args.names);
    let entries: Vec<StatusEntry> = targets
        .into_iter()
        .map(|skill| build_status_entry(&ctx.install_root, skill))
        .collect();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        for entry in entries {
            println!(
                "{}\t{}\t{}",
                entry.install_name,
                entry.state,
                entry.update.unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn run_diff(args: DoctorArgs) -> Result<()> {
    let ctx = load_project_context(args.root)?;
    let targets = select_skills(&ctx.lockfile.skills, args.names);
    if !args.names.is_empty() && targets.len() != args.names.len() {
        let resolved: HashSet<&str> = targets.iter().map(|s| s.install_name.as_str()).collect();
        let missing: Vec<&String> = args
            .names
            .iter()
            .filter(|name| !resolved.contains(name.as_str()))
            .collect();
        if !missing.is_empty() {
            bail!(
                "No installed skills matched: {}",
                missing
                    .into_iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    let repo_tips = resolve_remote_tips_for_targets(&targets);
    let stdout_is_tty = std::io::stdout().is_terminal();
    let mut printed_any = false;
    for skill in targets {
        let key = build_repo_key(skill);
        let remote = match repo_tips.get(&key).cloned() {
            Some(Ok(tip)) => tip,
            Some(Err(err)) => {
                eprintln!("{}: {}", skill.install_name, err);
                continue;
            }
            None => {
                eprintln!("{}: missing repo metadata", skill.install_name);
                continue;
            }
        };
        match diff_skill(&ctx.install_root, skill, &remote) {
            Ok(DiffOutcome::Diff(diff_text)) => {
                let display_name = if stdout_is_tty {
                    skill.install_name.as_str().bold().bright_cyan().to_string()
                } else {
                    skill.install_name.clone()
                };
                if printed_any {
                    println!();
                }
                println!("==> {}", display_name);
                println!(
                    "remote: {} @ {} (origin/{})",
                    format_repo_id(skill),
                    &remote.commit[..7],
                    remote.branch
                );
                print!("{diff_text}");
                printed_any = true;
            }
            Ok(DiffOutcome::NoDiff) => {}
            Err(err) => eprintln!("{}: {:#}", skill.install_name, err),
        }
    }
    Ok(())
}

struct DoctorState {
    apply: bool,
    lock_path: PathBuf,
    install_root: PathBuf,
    lockfile: lock::Lockfile,
    referenced_caches: HashSet<PathBuf>,
    orphans_to_drop: HashSet<String>,
    had_issues: bool,
    filters: Option<HashSet<String>>,
}

impl DoctorState {
    fn new(
        apply: bool,
        lock_path: PathBuf,
        install_root: PathBuf,
        lockfile: lock::Lockfile,
        names: &[String],
    ) -> Self {
        let filters = if names.is_empty() {
            None
        } else {
            Some(names.iter().cloned().collect())
        };
        Self {
            apply,
            lock_path,
            install_root,
            lockfile,
            referenced_caches: HashSet::new(),
            orphans_to_drop: HashSet::new(),
            had_issues: false,
            filters,
        }
    }

    fn check_duplicate_install_names(&mut self) {
        let mut seen = HashSet::new();
        for skill in &self.lockfile.skills {
            if !seen.insert(skill.install_name.clone()) {
                self.had_issues = true;
                println!(
                    "- Duplicate installName in lockfile: {}",
                    skill.install_name
                );
            }
        }
    }

    fn inspect_skills(&mut self) {
        let skills = self.lockfile.skills.clone();
        for skill in &skills {
            if let Some(filters) = &self.filters {
                if !filters.contains(skill.install_name.as_str()) {
                    continue;
                }
            }
            if let Some(report) = self.inspect_skill(skill) {
                println!("== {} ==", skill.install_name);
                for msg in &report.messages {
                    println!("{msg}");
                }
                if report.has_issue {
                    self.had_issues = true;
                }
            }
        }
    }

    fn inspect_skill(&mut self, skill: &lock::LockSkill) -> Option<SkillReport> {
        let mut report = SkillReport::default();
        let install_dir = self.install_root.join(&skill.install_name);
        let spec = skill.source.repo_spec();
        let cache_dir =
            paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
        self.referenced_caches.insert(cache_dir.clone());

        let mut local_modified = false;
        if !install_dir.exists() {
            report.add_issue(format!(
                "- Missing installed dir: {}",
                install_dir.display()
            ));
            if self.apply {
                self.rebuild_missing_install(&cache_dir, skill, &install_dir, &mut report);
            }
        } else {
            if let Err(msg) = manifest::validate_skill_manifest(&install_dir) {
                report.add_issue(format!("- {msg}"));
            }
            match digest::digest_dir(&install_dir) {
                Ok(hash) if hash == skill.digest => {}
                Ok(_) => {
                    report.add_issue("- Digest mismatch (modified)".to_string());
                    local_modified = true;
                }
                Err(_) => {
                    report.add_issue("- Digest compute failed".to_string());
                    local_modified = true;
                }
            }
        }

        let upstream_update = update::compute_upstream_update(
            &cache_dir,
            spec,
            &skill.commit,
            skill.source.skill_path(),
        );
        if !cache_dir.exists() {
            report.add_issue(format!("- Cache clone missing: {}", cache_dir.display()));
        } else if !git::has_object(&cache_dir, &skill.commit).unwrap_or(false) {
            report.add_issue("- Locked commit missing from cache (force-push?)".to_string());
        }

        match (local_modified, upstream_update.as_ref()) {
            (true, Some(update)) => {
                report.add_note(format!(
                    "- Local edits present and upstream advanced ({}). Run 'sk sync-back {name}' to publish or revert changes, then 'sk upgrade {name}' to pick up the remote tip.",
                    update,
                    name = skill.install_name
                ));
            }
            (true, None) => {
                report.add_note(format!(
                    "- Local edits are ahead of the lockfile. Run 'sk sync-back {name}' if intentional, or discard them to restore the locked digest.",
                    name = skill.install_name
                ));
            }
            (false, Some(update)) => {
                report.add_note(format!(
                    "- Upgrade available ({}). Run 'sk upgrade {name}' to sync.",
                    update,
                    name = skill.install_name
                ));
            }
            (false, None) => {}
        }

        if report.messages.is_empty() {
            None
        } else {
            Some(report)
        }
    }

    fn rebuild_missing_install(
        &mut self,
        cache_dir: &Path,
        skill: &lock::LockSkill,
        install_dir: &Path,
        report: &mut SkillReport,
    ) {
        if cache_dir.exists() && git::has_object(cache_dir, &skill.commit).unwrap_or(false) {
            match crate::install::extract_subdir_from_commit(
                cache_dir,
                &skill.commit,
                skill.source.skill_path(),
                install_dir,
            ) {
                Ok(_) => report.add_note("  Rebuilt from locked commit.".to_string()),
                Err(err) => report.add_note(format!("  Rebuild failed: {err}")),
            }
        } else {
            report.add_note("  Cannot rebuild: cache/commit missing.".to_string());
            self.orphans_to_drop.insert(update::lock_entry_key(skill));
        }
    }

    fn print_cache_messages(&mut self) {
        let cache_messages = cache::gather_cache_messages(&self.referenced_caches, self.apply);
        if !cache_messages.is_empty() {
            self.had_issues = true;
            println!("== Cache ==");
            for msg in cache_messages {
                println!("{msg}");
            }
        }
    }

    fn apply_lockfile_repairs(&mut self) -> Result<()> {
        if !self.apply {
            return Ok(());
        }

        let orphans = self.orphans_to_drop.clone();
        let (removed, normalized) = lock::edit_lockfile(&self.lock_path, |lf| {
            let before = lf.skills.len();
            if !orphans.is_empty() {
                lf.skills
                    .retain(|s| !orphans.contains(&update::lock_entry_key(s)));
            }
            let removed = before - lf.skills.len();
            let before_names: Vec<_> = lf.skills.iter().map(|s| s.install_name.clone()).collect();
            lf.skills
                .sort_by(|a, b| a.install_name.cmp(&b.install_name));
            let after_names: Vec<_> = lf.skills.iter().map(|s| s.install_name.clone()).collect();
            let normalized = removed > 0 || after_names != before_names;
            if normalized {
                lf.generated_at = Utc::now().to_rfc3339();
            }
            Ok((removed, normalized))
        })?;

        if removed > 0 {
            println!("Removed {removed} orphan lock entries.");
            self.had_issues = true;
        }
        if normalized {
            println!("Normalized lockfile (ordering/timestamps).");
            self.had_issues = true;
        }
        if removed > 0 || normalized {
            self.lockfile = lock::Lockfile::load(&self.lock_path)?;
        }
        Ok(())
    }

    fn finish(&self) {
        if !self.had_issues {
            println!("All checks passed.");
        }
    }
}

#[derive(Serialize)]
struct CheckEntry {
    install_name: String,
    state: String, // ok|modified|missing
}

#[derive(Serialize)]
struct StatusEntry {
    install_name: String,
    state: String, // clean|modified|missing
    locked: Option<String>,
    current: Option<String>,
    update: Option<String>, // old->new if out of date
}

struct ProjectContext {
    install_root: PathBuf,
    lockfile: lock::Lockfile,
}

fn load_project_context(root_flag: Option<&str>) -> Result<ProjectContext> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root_rel = root_flag.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, install_root_rel);
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        bail!("no lockfile");
    }
    let lockfile = lock::Lockfile::load(&lock_path)?;
    Ok(ProjectContext {
        install_root,
        lockfile,
    })
}

fn select_skills<'a>(skills: &'a [lock::LockSkill], names: &[String]) -> Vec<&'a lock::LockSkill> {
    if names.is_empty() {
        return skills.iter().collect();
    }
    let wanted: HashSet<&String> = names.iter().collect();
    skills
        .iter()
        .filter(|skill| wanted.contains(&skill.install_name))
        .collect()
}

fn build_check_entry(install_root: &Path, skill: &lock::LockSkill) -> CheckEntry {
    let dest = install_root.join(&skill.install_name);
    let state = if !dest.exists() {
        "missing".to_string()
    } else {
        let skill_md = dest.join("SKILL.md");
        let manifest_ok = skill_md.exists() && skills::parse_frontmatter_file(&skill_md).is_ok();
        let digest_ok = digest::digest_dir(&dest)
            .map(|h| h == skill.digest)
            .unwrap_or(false);
        if manifest_ok && digest_ok {
            "ok".to_string()
        } else {
            "modified".to_string()
        }
    };
    CheckEntry {
        install_name: skill.install_name.clone(),
        state,
    }
}

fn build_status_entry(install_root: &Path, skill: &lock::LockSkill) -> StatusEntry {
    let dest = install_root.join(&skill.install_name);
    let (state, current_digest) = compute_install_state(&dest, &skill.digest);
    let update = compute_remote_update(skill);
    StatusEntry {
        install_name: skill.install_name.clone(),
        state,
        locked: Some(skill.digest.clone()),
        current: current_digest,
        update,
    }
}

fn compute_install_state(dir: &Path, expected_digest: &str) -> (String, Option<String>) {
    if !dir.exists() {
        return ("missing".to_string(), None);
    }
    match digest::digest_dir(dir).ok() {
        Some(hash) if hash == expected_digest => ("clean".to_string(), Some(hash)),
        Some(hash) => ("modified".to_string(), Some(hash)),
        None => ("modified".to_string(), None),
    }
}

fn compute_remote_update(skill: &lock::LockSkill) -> Option<String> {
    let spec = skill.source.repo_spec();
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    if !cache_dir.exists() {
        return None;
    }
    let owned = skill.source.repo_spec_owned();
    let branch = git::detect_or_set_default_branch(&cache_dir, &owned).ok()?;
    let tip = git::rev_parse(&cache_dir, &format!("refs/remotes/origin/{branch}")).ok()?;
    if tip == skill.commit {
        None
    } else {
        Some(format!("{} -> {}", &skill.commit[..7], &tip[..7]))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct RepoKey {
    url: String,
    host: String,
    owner: String,
    repo: String,
}

fn build_repo_key(skill: &lock::LockSkill) -> RepoKey {
    let spec = skill.source.repo_spec();
    RepoKey {
        url: spec.url.clone(),
        host: spec.host.clone(),
        owner: spec.owner.clone(),
        repo: spec.repo.clone(),
    }
}

fn resolve_remote_tips_for_targets(
    skills: &[&lock::LockSkill],
) -> HashMap<RepoKey, Result<RemoteTip, String>> {
    let mut uniq: HashSet<RepoKey> = HashSet::new();
    for skill in skills {
        uniq.insert(build_repo_key(skill));
    }
    let mut handles = Vec::new();
    for key in uniq.into_iter() {
        handles.push(thread::spawn(move || {
            let result = resolve_remote_tip_for_key(&key).map_err(|err| format!("{err:#}"));
            (key, result)
        }));
    }
    let mut map = HashMap::new();
    for handle in handles {
        let (key, result) = handle.join().expect("repo refresh thread panicked");
        map.insert(key, result);
    }
    map
}

fn resolve_remote_tip_for_key(key: &RepoKey) -> Result<RemoteTip> {
    let cache_dir =
        paths::resolve_or_primary_cache_path(&key.url, &key.host, &key.owner, &key.repo);
    let spec = git::RepoSpec {
        url: key.url.clone(),
        host: key.host.clone(),
        owner: key.owner.clone(),
        repo: key.repo.clone(),
    };
    git::ensure_cached_repo(&cache_dir, &spec)
        .with_context(|| format!("refreshing cache for {}/{}", spec.owner, spec.repo))?;
    let branch = git::detect_or_set_default_branch(&cache_dir, &spec)?;
    let tip = git::rev_parse(&cache_dir, &format!("refs/remotes/origin/{branch}"))?;
    Ok(RemoteTip {
        cache_dir,
        branch,
        commit: tip,
    })
}

enum DiffOutcome {
    Diff(String),
    NoDiff,
}

#[derive(Clone)]
struct RemoteTip {
    cache_dir: PathBuf,
    branch: String,
    commit: String,
}

fn diff_skill(
    install_root: &Path,
    skill: &lock::LockSkill,
    remote: &RemoteTip,
) -> Result<DiffOutcome> {
    let local_dir = install_root.join(&skill.install_name);
    if !local_dir.exists() {
        bail!("local install missing at {}", local_dir.display());
    }
    let checkout = tempfile::tempdir().context("create temporary directory for remote contents")?;
    install::extract_subdir_from_commit(
        &remote.cache_dir,
        &remote.commit,
        skill.source.skill_path(),
        checkout.path(),
    )
    .with_context(|| {
        format!(
            "extracting '{}' from {}",
            skill.source.skill_path(),
            &remote.commit[..7]
        )
    })?;
    let output = std::process::Command::new("git")
        .arg("--no-pager")
        .arg("-c")
        .arg("core.autocrlf=false")
        .arg("diff")
        .arg("--no-index")
        .arg("--src-prefix=local/")
        .arg("--dst-prefix=remote/")
        .arg("--")
        .arg(&local_dir)
        .arg(checkout.path())
        .output()
        .context("git diff --no-index failed to run")?;
    match output.status.code() {
        Some(0) => Ok(DiffOutcome::NoDiff),
        Some(1) => {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            if text.trim().is_empty() {
                Ok(DiffOutcome::NoDiff)
            } else {
                Ok(DiffOutcome::Diff(text))
            }
        }
        Some(code) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git diff exited with status {code}: {stderr}");
        }
        None => bail!("git diff terminated by signal"),
    }
}

fn format_repo_id(skill: &lock::LockSkill) -> String {
    let spec = skill.source.repo_spec();
    let base = if spec.host == "local" {
        spec.url.clone()
    } else {
        format!("{}/{}", spec.owner, spec.repo)
    };
    if skill.source.skill_path() == "." {
        base
    } else {
        format!("{}:{}", base, skill.source.skill_path())
    }
}
