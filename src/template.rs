use crate::{config, git, paths, skills};
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use serde_yaml::{Mapping, Value};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct TemplateCreateArgs<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub root: Option<&'a str>,
}

pub fn run_template_create(args: TemplateCreateArgs) -> Result<()> {
    validate_skill_name(args.name)?;

    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root_rel = args.root.unwrap_or(&cfg.default_root);
    let install_root = paths::resolve_project_path(&project_root, install_root_rel);
    fs::create_dir_all(&install_root)
        .with_context(|| format!("create install root at {}", install_root.display()))?;

    let dest = install_root.join(args.name);
    if dest.exists() {
        bail!(
            "Destination '{}' already exists. Remove it first or pick a new skill name.",
            dest.display()
        );
    }

    let template_src = TemplateSource::parse(&cfg.template_source)?;
    let prefer_https = cfg.protocol.eq_ignore_ascii_case("https");
    let spec = git::parse_repo_input(&template_src.repo_input, prefer_https, &cfg.default_host)?;
    let cache_dir =
        paths::resolve_or_primary_cache_path(&spec.url, &spec.host, &spec.owner, &spec.repo);
    git::ensure_cached_repo(&cache_dir, &spec)?;
    let default_branch = git::detect_or_set_default_branch(&cache_dir, &spec)?;
    let commit = git::rev_parse(&cache_dir, &format!("refs/remotes/origin/{default_branch}"))?;

    let skill_path =
        resolve_template_skill_path(&cache_dir, &commit, &template_src.skill_selector)?;
    extract_skill_subdir(&cache_dir, &commit, &skill_path, &dest)?;
    rewrite_skill_metadata(&dest.join("SKILL.md"), args.name, args.description)?;

    println!(
        "Created skill '{}' at {} using {}/{}",
        args.name,
        dest.display(),
        spec.owner,
        spec.repo
    );
    Ok(())
}

struct TemplateSource {
    repo_input: String,
    skill_selector: String,
}

impl TemplateSource {
    fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("template_source is empty. Set it via 'sk config set template_source <repo> <skill>'.");
        }
        let mut parts = trimmed.split_whitespace();
        let repo = parts
            .next()
            .ok_or_else(|| anyhow!("template_source missing repo input"))?;
        let skill = parts
            .next()
            .ok_or_else(|| anyhow!("template_source missing skill selector"))?;
        if parts.next().is_some() {
            bail!("template_source may only contain '<repo> <skill>'");
        }
        Ok(Self {
            repo_input: repo.to_string(),
            skill_selector: skill.to_string(),
        })
    }
}

fn validate_skill_name(input: &str) -> Result<()> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("skill name must not be empty");
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        bail!("skill name may not contain path separators");
    }
    if trimmed == "." || trimmed == ".." {
        bail!("skill name may not be '.' or '..'");
    }
    Ok(())
}

fn resolve_template_skill_path(cache_dir: &Path, commit: &str, selector: &str) -> Result<String> {
    let normalized = normalize_skill_selector(selector);
    let skills = skills::list_skills_in_repo(cache_dir, commit)?;
    if let Some(skill) = skills
        .iter()
        .find(|s| normalize_skill_selector(&s.skill_path) == normalized)
    {
        return Ok(skill.skill_path.clone());
    }
    if let Some(skill) = skills.iter().find(|s| s.meta.name == selector) {
        return Ok(skill.skill_path.clone());
    }
    bail!(
        "Skill '{selector}' not found in template repo. Update template_source via 'sk config set template_source <repo> <skill>'."
    );
}

fn normalize_skill_selector(input: &str) -> String {
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

fn extract_skill_subdir(
    cache_dir: &Path,
    commit: &str,
    skill_path: &str,
    dest: &Path,
) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("create destination {}", dest.display()))?;
    let strip_components = if skill_path == "." {
        0
    } else {
        skill_path.split('/').count()
    };
    let src_path = if skill_path.trim().is_empty() {
        "."
    } else {
        skill_path
    };
    let mut archive = Command::new("git")
        .args([
            "-C",
            &cache_dir.to_string_lossy(),
            "archive",
            "--format=tar",
            commit,
            src_path,
        ])
        .stdout(Stdio::piped())
        .spawn()
        .context("spawn git archive")?;
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
        .context("spawn tar")?;
    let st1 = archive.wait()?;
    let st2 = tar.wait()?;
    if !st1.success() || !st2.success() {
        bail!("failed to extract template skill contents");
    }
    Ok(())
}

fn rewrite_skill_metadata(skill_md: &Path, name: &str, description: &str) -> Result<()> {
    let data =
        fs::read_to_string(skill_md).with_context(|| format!("reading {}", skill_md.display()))?;
    let frontmatter = Regex::new(r"(?s)^---\r?\n(.*?)\r?\n---\r?\n?").expect("valid regex");
    let captures = frontmatter
        .captures(&data)
        .ok_or_else(|| anyhow!("SKILL.md missing YAML front-matter"))?;
    let block = captures
        .get(0)
        .ok_or_else(|| anyhow!("missing front-matter block"))?;
    let yaml_section = captures
        .get(1)
        .map(|m| m.as_str())
        .ok_or_else(|| anyhow!("missing YAML payload"))?;

    let mut parsed: Value =
        serde_yaml::from_str(yaml_section).context("parsing SKILL front-matter YAML")?;
    let map = parsed
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("SKILL front-matter must be a YAML mapping"))?;
    upsert_yaml_string(map, "name", name);
    upsert_yaml_string(map, "description", description);

    let mut yaml = serde_yaml::to_string(&parsed)?;
    trim_trailing_newlines(&mut yaml);

    let mut new_content = String::new();
    new_content.push_str("---\n");
    new_content.push_str(&yaml);
    if !yaml.is_empty() {
        new_content.push('\n');
    }
    new_content.push_str("---\n");
    new_content.push_str(&data[block.end()..]);
    fs::write(skill_md, new_content)
        .with_context(|| format!("rewriting {}", skill_md.display()))?;
    Ok(())
}

fn upsert_yaml_string(map: &mut Mapping, key: &str, value: &str) {
    let yaml_key = Value::String(key.to_string());
    if let Some(existing) = map.get_mut(&yaml_key) {
        *existing = Value::String(value.to_string());
    } else {
        map.insert(yaml_key, Value::String(value.to_string()));
    }
}

fn trim_trailing_newlines(yaml: &mut String) {
    while yaml.ends_with(['\n', '\r']) {
        yaml.pop();
    }
}
