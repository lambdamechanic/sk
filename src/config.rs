use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const LEGACY_DEFAULT_ROOT: &str = "./skills";
pub const DEFAULT_MANAGED_ROOT: &str = "./.agents/skills";
pub const DEFAULT_CODEX_ROOT: &str = "./.agents/skills";
pub const DEFAULT_CLAUDE_ROOT: &str = "./.claude/skills";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UserConfig {
    pub default_root: String, // managed skills root, e.g. "./.agents/skills"
    pub codex_root: String,   // e.g. "./.agents/skills"
    pub claude_root: String,  // e.g. "./.claude/skills"
    pub protocol: String,     // "ssh" | "https"
    pub default_host: String, // e.g., "github.com"
    pub github_user: String,
    pub default_repo: String,
    pub template_source: String,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            default_root: DEFAULT_MANAGED_ROOT.to_string(),
            codex_root: DEFAULT_CODEX_ROOT.to_string(),
            claude_root: DEFAULT_CLAUDE_ROOT.to_string(),
            protocol: "ssh".to_string(),
            default_host: "github.com".to_string(),
            github_user: String::new(),
            default_repo: String::new(),
            template_source: default_template_source(),
        }
    }
}

fn default_template_source() -> String {
    "@anthropics/skills template-skill".to_string()
}

pub fn config_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("SK_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let pd = ProjectDirs::from("", "", "sk").context("unable to determine config dir")?;
    Ok(pd.config_dir().to_path_buf())
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load_or_default() -> Result<UserConfig> {
    let path = config_path()?;
    if path.exists() {
        let data = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: UserConfig =
            serde_json::from_slice(&data).with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    } else {
        Ok(UserConfig::default())
    }
}

pub fn save_if_missing(cfg: &UserConfig) -> Result<()> {
    let path = config_path()?;
    if !path.exists() {
        save(cfg)?;
    }
    Ok(())
}

pub fn save(cfg: &UserConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pretty = serde_json::to_string_pretty(cfg)?;
    fs::write(&path, pretty).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ManagedRoot {
    pub absolute: PathBuf,
}

pub fn normalize_root_spec(raw: &str) -> String {
    let trimmed = raw.trim().replace('\\', "/");
    if Path::new(&trimmed).is_absolute() {
        return trimmed.trim_end_matches('/').to_string();
    }

    let mut path = trimmed.as_str();
    while let Some(rest) = path.strip_prefix("./") {
        path = rest;
    }
    path = path.trim_matches('/');
    if path.is_empty() || path == "." {
        ".".to_string()
    } else {
        format!("./{path}")
    }
}

pub fn is_legacy_default_root(raw: &str) -> bool {
    normalize_root_spec(raw) == LEGACY_DEFAULT_ROOT
}

pub fn is_default_managed_root(raw: &str) -> bool {
    normalize_root_spec(raw) == DEFAULT_MANAGED_ROOT
}

pub fn resolve_managed_root(
    project_root: &Path,
    cfg: &UserConfig,
    root_override: Option<&str>,
) -> ManagedRoot {
    let configured = root_override.unwrap_or(&cfg.default_root).to_string();
    let configured_path = crate::paths::resolve_project_path(project_root, &configured);
    if root_override.is_some() {
        return ManagedRoot {
            absolute: configured_path,
        };
    }

    let canonical_path = crate::paths::resolve_project_path(project_root, DEFAULT_MANAGED_ROOT);
    let legacy_path = crate::paths::resolve_project_path(project_root, LEGACY_DEFAULT_ROOT);

    if is_legacy_default_root(&configured) {
        if !configured_path.exists() && canonical_path.exists() {
            eprintln!(
                "warning: sk config default_root still points to './skills', but this repo uses './.agents/skills'. Using './.agents/skills' for now. Run `sk config set default_root ./.agents/skills` to update your default."
            );
            return ManagedRoot {
                absolute: canonical_path,
            };
        }
        if configured_path.exists() {
            eprintln!(
                "warning: using legacy managed root './skills'. Run `sk migrate-root` to move this repo to './.agents/skills'."
            );
        }
    }

    if is_default_managed_root(&configured) && !configured_path.exists() && legacy_path.exists() {
        eprintln!(
            "warning: this repo still uses the legacy './skills' layout. Falling back to './skills' for now. Run `sk migrate-root` to move it to './.agents/skills'."
        );
        return ManagedRoot {
            absolute: legacy_path,
        };
    }

    ManagedRoot {
        absolute: configured_path,
    }
}
