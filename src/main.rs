mod cli;
mod config;
mod digest;
mod doctor;
mod git;
mod install;
mod lock;
mod mcp;
mod paths;
mod precommit;
mod remove;
mod repo;
mod skills;
mod sync;
mod template;
mod update;
mod upgrade;

use anyhow::{bail, Context, Result};
use clap::Parser;
use owo_colors::OwoColorize;

use crate::cli::{Cli, Commands, ConfigCmd, RepoCmd, TemplateCmd};
use crate::doctor::{DoctorArgs, DoctorMode};
use serde::Serialize;
use std::io::IsTerminal;
use unicode_width::UnicodeWidthStr;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { root } => cmd_init(root.as_deref()),
        Commands::List { root, json } => cmd_list(root.as_deref(), json),
        Commands::Where {
            installed_name,
            root,
        } => cmd_where(&installed_name, root.as_deref()),
        Commands::Check { names, root, json } => {
            warn_deprecated("check", "sk doctor --summary");
            cmd_doctor(CmdDoctorConfig {
                names: &names,
                root_flag: root.as_deref(),
                summary: true,
                status: false,
                diff: false,
                json,
                apply: false,
            })
        }
        Commands::Status { names, root, json } => {
            warn_deprecated("status", "sk doctor --status");
            cmd_doctor(CmdDoctorConfig {
                names: &names,
                root_flag: root.as_deref(),
                summary: false,
                status: true,
                diff: false,
                json,
                apply: false,
            })
        }
        Commands::Diff { names, root } => {
            warn_deprecated("diff", "sk doctor --diff");
            cmd_doctor(CmdDoctorConfig {
                names: &names,
                root_flag: root.as_deref(),
                summary: false,
                status: false,
                diff: true,
                json: false,
                apply: false,
            })
        }
        Commands::Update => update::run_update(),
        Commands::Upgrade {
            target,
            root,
            dry_run,
        } => upgrade::run_upgrade(upgrade::UpgradeArgs {
            target: &target,
            root: root.as_deref(),
            dry_run,
        }),
        Commands::Remove {
            installed_name,
            root,
            force,
        } => remove::run_remove(remove::RemoveArgs {
            installed_name: &installed_name,
            root: root.as_deref(),
            force,
        }),
        Commands::SyncBack {
            installed_name,
            branch,
            message,
            root,
            repo,
            skill_path,
            https,
        } => sync::run_sync_back(sync::SyncBackArgs {
            installed_name: &installed_name,
            branch: branch.as_deref(),
            message: message.as_deref(),
            root: root.as_deref(),
            repo: repo.as_deref(),
            skill_path: skill_path.as_deref(),
            https,
        }),
        Commands::Doctor {
            names,
            root,
            summary,
            status,
            diff,
            json,
            apply,
        } => cmd_doctor(CmdDoctorConfig {
            names: &names,
            root_flag: root.as_deref(),
            summary,
            status,
            diff,
            json,
            apply,
        }),
        Commands::Config { cmd } => cmd_config(cmd),
        Commands::Template { cmd } => cmd_template(cmd),
        Commands::Repo { cmd } => cmd_repo(cmd),
        Commands::Precommit { allow_local } => precommit::run_precommit(allow_local),
        Commands::McpServer { root } => mcp::run_server(root.as_deref()),
        Commands::Install {
            repo,
            skill_name,
            alias,
            path,
            root,
            https,
        } => install::run_install(install::InstallArgs {
            repo: &repo,
            skill_name: &skill_name,
            alias: alias.as_deref(),
            path: path.as_deref(),
            root: root.as_deref(),
            https,
        }),
    }
}

fn cmd_template(cmd: TemplateCmd) -> Result<()> {
    match cmd {
        TemplateCmd::Create {
            name,
            description,
            root,
        } => template::run_template_create(template::TemplateCreateArgs {
            name: &name,
            description: &description,
            root: root.as_deref(),
        }),
    }
}

fn cmd_repo(cmd: RepoCmd) -> Result<()> {
    match cmd {
        RepoCmd::Add { repo, alias, https } => repo::run_repo_add(repo::RepoAddArgs {
            repo: &repo,
            alias: alias.as_deref(),
            https,
        }),
        RepoCmd::List { json } => repo::run_repo_list(repo::RepoListArgs { json }),
        RepoCmd::Catalog {
            target,
            https,
            json,
        } => {
            warn_deprecated(
                "repo catalog",
                "sk repo search --repo <alias-or-repo> --all",
            );
            repo::run_repo_search(repo::RepoSearchArgs {
                query: None,
                target: Some(&target),
                https,
                json,
                list_all: true,
            })
        }
        RepoCmd::Search {
            query,
            repo: target,
            https,
            json,
            all,
        } => repo::run_repo_search(repo::RepoSearchArgs {
            query: query.as_deref(),
            target: target.as_deref(),
            https,
            json,
            list_all: all,
        }),
        RepoCmd::Remove {
            target,
            https,
            json,
        } => repo::run_repo_remove(repo::RepoRemoveArgs {
            target: &target,
            https,
            json,
        }),
    }
}

fn warn_deprecated(cmd: &str, replacement: &str) {
    eprintln!(
        "warning: `sk {cmd}` is deprecated; use `{replacement}` instead.",
        replacement = replacement
    );
}

struct CmdDoctorConfig<'a> {
    names: &'a [String],
    root_flag: Option<&'a str>,
    summary: bool,
    status: bool,
    diff: bool,
    json: bool,
    apply: bool,
}

fn cmd_doctor(cfg: CmdDoctorConfig<'_>) -> Result<()> {
    let mode_flags = cfg.summary as u8 + cfg.status as u8 + cfg.diff as u8;
    if mode_flags > 1 {
        bail!("Choose only one of --summary, --status, or --diff.");
    }

    let mode = if cfg.summary {
        DoctorMode::Summary
    } else if cfg.status {
        DoctorMode::Status
    } else if cfg.diff {
        DoctorMode::Diff
    } else {
        DoctorMode::Diagnose
    };

    if cfg.json && !matches!(mode, DoctorMode::Summary | DoctorMode::Status) {
        bail!("--json is only supported with --summary or --status.");
    }
    if cfg.apply && !matches!(mode, DoctorMode::Diagnose) {
        bail!("--apply cannot be combined with --summary/--status/--diff.");
    }

    doctor::run_doctor(DoctorArgs {
        names: cfg.names,
        root: cfg.root_flag,
        mode,
        json: cfg.json,
        apply: cfg.apply,
    })
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
                "default_repo" => println!("{}", cfg.default_repo),
                "template_source" => println!("{}", cfg.template_source),
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
                "default_repo" => cfg.default_repo = value,
                "template_source" => cfg.template_source = value,
                _ => anyhow::bail!("Unknown key: {key}"),
            }
            config::save(&cfg)?;
            println!("ok");
        }
    }
    Ok(())
}

fn cmd_list(_root_flag: Option<&str>, json: bool) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root_rel = _root_flag.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, install_root_rel);
    let lock_path = project_root.join("skills.lock.json");
    if !lock_path.exists() {
        println!("[]");
        return Ok(());
    }
    let lf = lock::Lockfile::load(&lock_path)?;
    let rows: Vec<ListRow> = lf
        .skills
        .iter()
        .map(|skill| {
            let (display_name, description) = match load_skill_meta(&install_root, skill) {
                Some(meta) => (meta.name, meta.description),
                None => (skill.install_name.clone(), String::new()),
            };
            ListRow {
                install_name: skill.install_name.clone(),
                display_name,
                repo: format_repo_id(skill),
                skill_path: skill.source.skill_path().to_string(),
                description,
            }
        })
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        let stdout_is_tty = std::io::stdout().is_terminal();
        let max_name_width = rows
            .iter()
            .map(|row| UnicodeWidthStr::width(row.display_name.as_str()))
            .max()
            .unwrap_or(0);
        for row in rows {
            let name_width = UnicodeWidthStr::width(row.display_name.as_str());
            let colored_name = if stdout_is_tty {
                row.display_name.clone().bold().bright_cyan().to_string()
            } else {
                row.display_name.clone()
            };
            if row.description.is_empty() {
                println!("{}", colored_name);
            } else {
                let gap = max_name_width.saturating_sub(name_width) + 2;
                let padding = " ".repeat(gap);
                println!("{}{}{}", colored_name, padding, row.description);
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct ListRow {
    #[serde(rename = "installName")]
    install_name: String,
    #[serde(skip_serializing)]
    display_name: String,
    repo: String,
    #[serde(rename = "skillPath")]
    skill_path: String,
    description: String,
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

fn load_skill_meta(
    install_root: &std::path::Path,
    skill: &lock::LockSkill,
) -> Option<skills::SkillMeta> {
    let skill_md = install_root.join(&skill.install_name).join("SKILL.md");
    crate::skills::parse_frontmatter_file(&skill_md).ok()
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
