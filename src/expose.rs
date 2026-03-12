use crate::{config, git, paths};
use anyhow::{bail, Context, Result};
use pathdiff::diff_paths;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, Debug)]
pub enum ExposureTarget {
    Codex,
    Claude,
    Both,
}

impl From<crate::cli::ExposeTargetArg> for ExposureTarget {
    fn from(value: crate::cli::ExposeTargetArg) -> Self {
        match value {
            crate::cli::ExposeTargetArg::Codex => Self::Codex,
            crate::cli::ExposeTargetArg::Claude => Self::Claude,
            crate::cli::ExposeTargetArg::Both => Self::Both,
        }
    }
}

pub struct ExposeArgs<'a> {
    pub target: ExposureTarget,
    pub root: Option<&'a str>,
}

pub fn run_expose(args: ExposeArgs<'_>) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let managed_root_rel = args.root.unwrap_or(&cfg.default_root);
    let managed_root = paths::resolve_project_path(&project_root, managed_root_rel);
    if !managed_root.exists() {
        bail!(
            "managed skills root '{}' is missing (expected at {}). Run `sk init` first.",
            display_path(&managed_root, &project_root),
            managed_root.display()
        );
    }
    let managed_root = managed_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", managed_root.display()))?;
    let exposures = build_exposures(&project_root, &cfg, args.target)?;

    for exposure in &exposures {
        match target_state(&managed_root, &exposure.link_path)? {
            TargetState::Missing => {}
            TargetState::AlreadyExposed => {}
            TargetState::SameDirectory => {}
            TargetState::Conflict => {
                bail!(
                    "{} exposure target '{}' already exists and is not managed by sk. Move it aside or configure `{}` to a different path.",
                    exposure.label,
                    display_path(&exposure.link_path, &project_root),
                    exposure.config_key
                );
            }
        }
    }

    for exposure in &exposures {
        let status = match target_state(&managed_root, &exposure.link_path)? {
            TargetState::Missing => {
                if let Some(parent) = exposure.link_path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                let relative_target = diff_paths(&managed_root, exposure.parent())
                    .unwrap_or_else(|| managed_root.clone());
                create_dir_symlink(&relative_target, &exposure.link_path).with_context(|| {
                    format!(
                        "create {} exposure symlink {} -> {}",
                        exposure.label,
                        exposure.link_path.display(),
                        relative_target.display()
                    )
                })?;
                ExposureStatus::Created(relative_target)
            }
            TargetState::AlreadyExposed => ExposureStatus::AlreadyExposed,
            TargetState::SameDirectory => ExposureStatus::SameDirectory,
            TargetState::Conflict => unreachable!("preflight rejected conflicting targets"),
        };

        match status {
            ExposureStatus::Created(target) => println!(
                "Created {} exposure: {} -> {}",
                exposure.label,
                display_path(&exposure.link_path, &project_root),
                target.display()
            ),
            ExposureStatus::AlreadyExposed => println!(
                "{} exposure already points at {}.",
                exposure.label,
                display_path(&managed_root, &project_root)
            ),
            ExposureStatus::SameDirectory => println!(
                "{} exposure already uses the managed root directly at {}.",
                exposure.label,
                display_path(&exposure.link_path, &project_root)
            ),
        }
    }

    Ok(())
}

#[derive(Copy, Clone, Debug)]
enum TargetState {
    Missing,
    AlreadyExposed,
    SameDirectory,
    Conflict,
}

enum ExposureStatus {
    Created(PathBuf),
    AlreadyExposed,
    SameDirectory,
}

struct Exposure {
    label: &'static str,
    config_key: &'static str,
    link_path: PathBuf,
}

impl Exposure {
    fn parent(&self) -> &Path {
        self.link_path
            .parent()
            .expect("exposure link path should always have a parent")
    }
}

fn build_exposures(
    project_root: &Path,
    cfg: &config::UserConfig,
    target: ExposureTarget,
) -> Result<Vec<Exposure>> {
    match target {
        ExposureTarget::Codex => Ok(vec![Exposure {
            label: "Codex",
            config_key: "codex_root",
            link_path: resolve_target_root(project_root, &cfg.codex_root, "codex_root")?,
        }]),
        ExposureTarget::Claude => Ok(vec![Exposure {
            label: "Claude",
            config_key: "claude_root",
            link_path: resolve_target_root(project_root, &cfg.claude_root, "claude_root")?,
        }]),
        ExposureTarget::Both => Ok(vec![
            Exposure {
                label: "Codex",
                config_key: "codex_root",
                link_path: resolve_target_root(project_root, &cfg.codex_root, "codex_root")?,
            },
            Exposure {
                label: "Claude",
                config_key: "claude_root",
                link_path: resolve_target_root(project_root, &cfg.claude_root, "claude_root")?,
            },
        ]),
    }
}

fn resolve_target_root(project_root: &Path, raw: &str, key: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("config key `{key}` must not be empty");
    }
    let path = paths::resolve_project_path(project_root, trimmed);
    if path == *project_root {
        bail!("config key `{key}` resolves to the project root; choose a subdirectory");
    }
    Ok(path)
}

fn target_state(managed_root: &Path, link_path: &Path) -> Result<TargetState> {
    let metadata = match fs::symlink_metadata(link_path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(TargetState::Missing),
        Err(err) => return Err(err).with_context(|| format!("stat {}", link_path.display())),
    };

    if metadata.file_type().is_symlink() {
        let raw_target =
            fs::read_link(link_path).with_context(|| format!("read {}", link_path.display()))?;
        let resolved = resolve_link_target(link_path, &raw_target);
        let resolved = resolved
            .canonicalize()
            .with_context(|| format!("canonicalize {}", resolved.display()))?;
        if resolved == managed_root {
            Ok(TargetState::AlreadyExposed)
        } else {
            Ok(TargetState::Conflict)
        }
    } else {
        let resolved = link_path
            .canonicalize()
            .with_context(|| format!("canonicalize {}", link_path.display()))?;
        if resolved == managed_root {
            Ok(TargetState::SameDirectory)
        } else {
            Ok(TargetState::Conflict)
        }
    }
}

fn resolve_link_target(link_path: &Path, raw_target: &Path) -> PathBuf {
    if raw_target.is_absolute() {
        raw_target.to_path_buf()
    } else {
        link_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(raw_target)
    }
}

fn display_path(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link_path: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link_path)
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link_path: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link_path)
}

#[cfg(not(any(unix, windows)))]
fn create_dir_symlink(_target: &Path, link_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!(
            "directory symlinks are not supported on {}",
            link_path.display()
        ),
    ))
}
