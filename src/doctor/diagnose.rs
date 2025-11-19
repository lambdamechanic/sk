use super::{cache, manifest, report::SkillReport, update};
use crate::{config, lock, paths};
use anyhow::Result;
use chrono::Utc;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn run(args: crate::doctor::DoctorArgs) -> Result<()> {
    let project_root = crate::git::ensure_git_repo()?;
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
            match crate::digest::digest_dir(&install_dir) {
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
        } else if !crate::git::has_object(&cache_dir, &skill.commit).unwrap_or(false) {
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
        if cache_dir.exists() && crate::git::has_object(cache_dir, &skill.commit).unwrap_or(false) {
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
