use crate::{config, expose::ExposureTarget, git, paths};
use anyhow::{bail, Context, Result};
use pathdiff::diff_paths;
use std::fs;
use std::path::{Path, PathBuf};

pub struct MigrateRootArgs<'a> {
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    pub expose: Option<ExposureTarget>,
    pub force: bool,
    pub keep_existing: bool,
}

pub fn run_migrate_root(args: MigrateRootArgs<'_>) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let mut cfg = config::load_or_default()?;
    let from_rel = args.from.unwrap_or(config::LEGACY_DEFAULT_ROOT);
    let to_rel = args.to.unwrap_or(config::DEFAULT_MANAGED_ROOT);
    let from_root = paths::resolve_project_path(&project_root, from_rel);
    let to_root = paths::resolve_project_path(&project_root, to_rel);

    if from_root == to_root {
        bail!(
            "source root '{}' and destination root '{}' must differ",
            from_rel,
            to_rel
        );
    }
    if !from_root.exists() {
        bail!(
            "source managed root '{}' is missing at {}",
            display_path(&from_root, &project_root),
            from_root.display()
        );
    }

    let source_canonical = from_root
        .canonicalize()
        .with_context(|| format!("canonicalize {}", from_root.display()))?;
    let mut exposures = build_exposure_specs(&project_root, &cfg, args.expose);

    for spec in &mut exposures {
        if path_points_to(&spec.link_path, &source_canonical)? {
            spec.required = true;
            if spec.link_path != to_root {
                remove_symlink(&spec.link_path)
                    .with_context(|| format!("remove {}", spec.link_path.display()))?;
            }
        }
        if spec.required
            && spec.link_path != to_root
            && fs::symlink_metadata(&spec.link_path).is_ok()
        {
            bail!(
                "{} exposure target '{}' already exists and is not managed by sk. Move it aside before migrating.",
                spec.label,
                display_path(&spec.link_path, &project_root)
            );
        }
    }

    match fs::symlink_metadata(&to_root) {
        Ok(metadata) if metadata.file_type().is_symlink() && path_points_to(&to_root, &source_canonical)? => {
            remove_symlink(&to_root).with_context(|| format!("remove {}", to_root.display()))?;
        }
        Ok(_) => {
            if args.keep_existing {
                println!(
                    "Skipping migration: destination '{}' already exists (keeping existing).",
                    display_path(&to_root, &project_root)
                );
                return Ok(());
            } else if args.force {
                eprintln!(
                    "Warning: removing existing destination '{}' (--force).",
                    display_path(&to_root, &project_root)
                );
                fs::remove_dir_all(&to_root)
                    .with_context(|| format!("remove {}", to_root.display()))?;
            } else {
                bail!(
                    "destination managed root '{}' already exists. Use --force to overwrite or --keep-existing to skip.",
                    display_path(&to_root, &project_root)
                );
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("stat {}", to_root.display())),
    }

    if let Some(parent) = to_root.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::rename(&from_root, &to_root)
        .with_context(|| format!("move {} -> {}", from_root.display(), to_root.display()))?;

    if config::normalize_root_spec(&cfg.default_root) == config::normalize_root_spec(from_rel) {
        cfg.default_root = to_rel.to_string();
        config::save(&cfg)?;
    }

    for spec in exposures {
        if !spec.required {
            continue;
        }
        if spec.link_path == to_root {
            println!(
                "{} exposure now uses the managed root directly at {}.",
                spec.label,
                display_path(&to_root, &project_root)
            );
            continue;
        }
        ensure_symlink_to(&project_root, &to_root, &spec)?;
    }

    println!(
        "Migrated managed root: {} -> {}",
        display_path(&from_root, &project_root),
        display_path(&to_root, &project_root)
    );
    Ok(())
}

struct ExposureSpec {
    label: &'static str,
    link_path: PathBuf,
    required: bool,
}

fn build_exposure_specs(
    project_root: &Path,
    cfg: &config::UserConfig,
    requested: Option<ExposureTarget>,
) -> Vec<ExposureSpec> {
    let want_codex = matches!(
        requested,
        Some(ExposureTarget::Codex | ExposureTarget::Both)
    );
    let want_claude = matches!(
        requested,
        Some(ExposureTarget::Claude | ExposureTarget::Both)
    );

    vec![
        ExposureSpec {
            label: "Codex",
            link_path: paths::resolve_project_path(project_root, &cfg.codex_root),
            required: want_codex,
        },
        ExposureSpec {
            label: "Claude",
            link_path: paths::resolve_project_path(project_root, &cfg.claude_root),
            required: want_claude,
        },
    ]
}

fn ensure_symlink_to(project_root: &Path, managed_root: &Path, spec: &ExposureSpec) -> Result<()> {
    match fs::symlink_metadata(&spec.link_path) {
        Ok(_) if path_points_to(&spec.link_path, managed_root)? => {
            println!(
                "{} exposure already points at {}.",
                spec.label,
                display_path(managed_root, project_root)
            );
            return Ok(());
        }
        Ok(_) => bail!(
            "{} exposure target '{}' already exists and is not managed by sk.",
            spec.label,
            display_path(&spec.link_path, project_root)
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("stat {}", spec.link_path.display())),
    }

    if let Some(parent) = spec.link_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let relative_target = diff_paths(managed_root, spec.link_path.parent().unwrap())
        .unwrap_or_else(|| managed_root.to_path_buf());
    create_dir_symlink(&relative_target, &spec.link_path).with_context(|| {
        format!(
            "create {} exposure symlink {} -> {}",
            spec.label,
            spec.link_path.display(),
            relative_target.display()
        )
    })?;
    println!(
        "Created {} exposure: {} -> {}",
        spec.label,
        display_path(&spec.link_path, project_root),
        relative_target.display()
    );
    Ok(())
}

fn path_points_to(path: &Path, expected: &Path) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).with_context(|| format!("stat {}", path.display())),
    };

    if metadata.file_type().is_symlink() {
        let raw_target = fs::read_link(path).with_context(|| format!("read {}", path.display()))?;
        let resolved = resolve_link_target(path, &raw_target);
        match resolved.canonicalize() {
            Ok(resolved) => Ok(resolved == expected),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err).with_context(|| format!("canonicalize {}", resolved.display())),
        }
    } else {
        match path.canonicalize() {
            Ok(resolved) => Ok(resolved == expected),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err).with_context(|| format!("canonicalize {}", path.display())),
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
fn remove_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn remove_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path)
}

#[cfg(not(any(unix, windows)))]
fn remove_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)
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
