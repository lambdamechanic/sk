mod cli;
mod config;
mod digest;
mod doctor;
mod git;
mod install;
mod lock;
mod paths;
mod skills;
mod update;

use anyhow::{Context, Result};
use clap::Parser;

use crate::cli::{Cli, Commands, ConfigCmd};
use serde::Serialize;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { root } => cmd_init(root.as_deref()),
        Commands::List { root, json } => cmd_list(root.as_deref(), json),
        Commands::Where {
            installed_name,
            root,
        } => cmd_where(&installed_name, root.as_deref()),
        Commands::Check { names, root, json } => cmd_check(&names, root.as_deref(), json),
        Commands::Status { names, root, json } => cmd_status(&names, root.as_deref(), json),
        Commands::Update => update::run_update(),
        Commands::Upgrade {
            target,
            r#ref,
            root,
            dry_run,
            include_pinned,
        } => {
            let _ = (target, r#ref, dry_run, include_pinned);
            cmd_unimplemented("upgrade", false, root.as_deref())
        }
        Commands::Remove {
            installed_name,
            root,
            force,
        } => {
            let _ = (installed_name, force);
            cmd_unimplemented("remove", false, root.as_deref())
        }
        Commands::SyncBack {
            installed_name,
            branch,
            message,
            root,
        } => {
            let _ = (installed_name, branch, message);
            cmd_unimplemented("sync-back", false, root.as_deref())
        }
        Commands::Doctor { apply } => doctor::run_doctor(apply),
        Commands::Config { cmd } => cmd_config(cmd),
        Commands::Install {
            repo,
            skill_name,
            r#ref,
            alias,
            path,
            root,
            https,
        } => install::run_install(install::InstallArgs {
            repo: &repo,
            skill_name: &skill_name,
            r#ref: r#ref.as_deref(),
            alias: alias.as_deref(),
            path: path.as_deref(),
            root: root.as_deref(),
            https,
        }),
    }
}

fn cmd_init(root_flag: Option<&str>) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let mut cfg = config::load_or_default()?;
    let install_root_rel = root_flag.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, install_root_rel);
    std::fs::create_dir_all(&install_root)
        .with_context(|| format!("create install root at {}", install_root.display()))?;

    // Create empty lockfile if absent
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        let lf = lock::Lockfile::empty_now();
        lock::save_lockfile(&lock_path, &lf)?;
        println!(
            "Created {}",
            lock_path
                .strip_prefix(&project_root)
                .unwrap_or(&lock_path)
                .display()
        );
    }

    // Ensure user config is saved
    if root_flag.is_some() && cfg.default_root != install_root_rel {
        cfg.default_root = install_root_rel.to_string();
    }
    config::save_if_missing(&cfg)?;

    println!("Initialized. Install root: {}", install_root.display());
    Ok(())
}

fn cmd_config(cmd: ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::Get { key } => {
            let cfg = config::load_or_default()?;
            match key.as_str() {
                "default_root" => println!("{}", cfg.default_root),
                "protocol" => println!("{}", cfg.protocol),
                "default_host" => println!("{}", cfg.default_host),
                "github_user" => println!("{}", cfg.github_user),
                _ => anyhow::bail!("Unknown key: {key}"),
            }
        }
        ConfigCmd::Set { key, value } => {
            let mut cfg = config::load_or_default()?;
            match key.as_str() {
                "default_root" => cfg.default_root = value,
                "protocol" => cfg.protocol = value,
                "default_host" => cfg.default_host = value,
                "github_user" => cfg.github_user = value,
                _ => anyhow::bail!("Unknown key: {key}"),
            }
            config::save(&cfg)?;
            println!("ok");
        }
    }
    Ok(())
}

fn cmd_unimplemented(name: &str, _json: bool, _root: Option<&str>) -> Result<()> {
    anyhow::bail!("'sk {name}' not implemented yet in scaffolding phase")
}

fn cmd_list(_root_flag: Option<&str>, json: bool) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        println!("[]");
        return Ok(());
    }
    let data = std::fs::read(&lock_path)?;
    let lf: lock::Lockfile = serde_json::from_slice(&data)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&lf.skills)?);
    } else {
        for s in lf.skills {
            println!(
                "{}\t{}@{}\t{}",
                s.install_name,
                s.source.repo,
                &s.commit[..7],
                s.source.skill_path
            );
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct CheckEntry {
    install_name: String,
    state: String,    // ok|modified|missing
}

fn cmd_check(names: &[String], root_flag: Option<&str>, json: bool) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root =
        paths::resolve_project_path(&project_root, root_flag.unwrap_or(&cfg.default_root));
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        anyhow::bail!("no lockfile");
    }
    let data = std::fs::read(&lock_path)?;
    let lf: lock::Lockfile = serde_json::from_slice(&data)?;
    let target_names: Vec<String> = if names.is_empty() {
        lf.skills.iter().map(|s| s.install_name.clone()).collect()
    } else {
        names.to_vec()
    };

    let mut out: Vec<CheckEntry> = vec![];
    for skill in lf
        .skills
        .iter()
        .filter(|s| target_names.contains(&s.install_name))
    {
        let dest = install_root.join(&skill.install_name);
        let state = if !dest.exists() {
            "missing".to_string()
        } else {
            // Validate SKILL.md front-matter
            let skill_md = dest.join("SKILL.md");
            let valid = if skill_md.exists() {
                crate::skills::parse_frontmatter_file(&skill_md).is_ok()
            } else {
                false
            };
            // Digest comparison
            let digest_ok = crate::digest::digest_dir(&dest)
                .map(|h| h == skill.digest)
                .unwrap_or(false);
            if valid && digest_ok {
                "ok".to_string()
            } else {
                "modified".to_string()
            }
        };
        out.push(CheckEntry {
            install_name: skill.install_name.clone(),
            state,
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for e in out {
            println!("{}\t{}", e.install_name, e.state);
        }
    }
    Ok(())
}

fn cmd_where(name: &str, root_flag: Option<&str>) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root_rel = root_flag.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, install_root_rel);
    let path = install_root.join(name);
    if path.exists() {
        println!("{}", path.display());
        Ok(())
    } else {
        anyhow::bail!("not found: {name}")
    }
}

#[derive(Serialize)]
struct StatusEntry {
    install_name: String,
    state: String, // clean|modified|missing
    locked: Option<String>,
    current: Option<String>,
    update: Option<String>, // old->new if out of date
}

fn cmd_status(names: &[String], root_flag: Option<&str>, json: bool) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root =
        paths::resolve_project_path(&project_root, root_flag.unwrap_or(&cfg.default_root));
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        anyhow::bail!("no lockfile");
    }
    let data = std::fs::read(&lock_path)?;
    let lf: lock::Lockfile = serde_json::from_slice(&data)?;
    let target_names: Vec<String> = if names.is_empty() {
        lf.skills.iter().map(|s| s.install_name.clone()).collect()
    } else {
        names.to_vec()
    };
    let mut out_entries: Vec<StatusEntry> = vec![];

    for skill in lf
        .skills
        .iter()
        .filter(|s| target_names.contains(&s.install_name))
    {
        let dest = install_root.join(&skill.install_name);
        let (state, current_digest) = if !dest.exists() {
            ("missing".to_string(), None)
        } else {
            let d = crate::digest::digest_dir(&dest).ok();
            match d {
                Some(hash) if hash == skill.digest => ("clean".to_string(), Some(hash)),
                Some(hash) => ("modified".to_string(), Some(hash)),
                None => ("modified".to_string(), None),
            }
        };
        // Out-of-date check based on cache
        let cache_dir =
            paths::cache_repo_path(&skill.source.host, &skill.source.owner, &skill.source.repo);
        let mut update_str = None;
        if cache_dir.exists() {
            // Determine tracked tip commit
            let new_tip = match &skill.ref_ {
                Some(r) => {
                    if let Ok(Some(_)) = git::remote_branch_tip(&cache_dir, r) {
                        Some(git::rev_parse(
                            &cache_dir,
                            &format!("refs/remotes/origin/{r}"),
                        )?)
                    } else {
                        None
                    }
                }
                None => {
                    let default =
                        git::detect_or_set_default_branch(&cache_dir, &skill.source.url).ok();
                    default
                        .map(|b| git::rev_parse(&cache_dir, &format!("refs/remotes/origin/{b}")))
                        .transpose()
                        .ok()
                        .flatten()
                }
            };
            if let Some(new_sha) = new_tip {
                if new_sha != skill.commit {
                    update_str = Some(format!("{} -> {}", &skill.commit[..7], &new_sha[..7]));
                }
            }
        }
        out_entries.push(StatusEntry {
            install_name: skill.install_name.clone(),
            state,
            locked: Some(skill.digest.clone()),
            current: current_digest,
            update: update_str,
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&out_entries)?);
    } else {
        for e in out_entries {
            let upd = e.update.unwrap_or_else(|| "".to_string());
            println!("{}\t{}\t{}", e.install_name, e.state, upd);
        }
    }
    Ok(())
}
