use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    pub skill_path: String, // subdir path containing SKILL.md
    pub meta: SkillMeta,
}

pub fn list_skills_in_repo(cache_dir: &Path, commit: &str) -> Result<Vec<DiscoveredSkill>> {
    // Use git ls-tree to find SKILL.md paths, then read contents
    let out = Command::new("git")
        .args([
            "-C",
            &cache_dir.to_string_lossy(),
            "ls-tree",
            "-r",
            "--name-only",
            commit,
        ])
        .output()
        .context("git ls-tree failed")?;
    if !out.status.success() {
        bail!("ls-tree failed for commit {commit}");
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut skills = vec![];
    for line in stdout.lines() {
        if !line.ends_with("/SKILL.md") && line != "SKILL.md" {
            continue;
        }
        let file_path = line;
        let content = Command::new("git")
            .args([
                "-C",
                &cache_dir.to_string_lossy(),
                "show",
                &format!("{commit}:{file_path}"),
            ])
            .output()
            .context("git show failed")?;
        if !content.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&content.stdout);
        if let Ok(meta) = parse_skill_frontmatter_str(&text) {
            let skill_dir = if file_path == "SKILL.md" {
                ".".to_string()
            } else {
                file_path.trim_end_matches("/SKILL.md").to_string()
            };
            skills.push(DiscoveredSkill {
                skill_path: skill_dir,
                meta,
            });
        }
    }
    Ok(skills)
}

pub fn parse_skill_frontmatter_str(text: &str) -> Result<SkillMeta> {
    // Expect leading --- YAML --- frontmatter; tolerate CRLF on Windows
    let regex = Regex::new(r"(?s)^---\r?\n(.*?)\r?\n---")?;
    let captures = regex
        .captures(text)
        .context("missing YAML front-matter block")?;
    let yaml = captures
        .get(1)
        .map(|m| m.as_str())
        .context("empty YAML front-matter")?;
    match serde_yaml::from_str::<SkillMeta>(yaml) {
        Ok(meta) => Ok(meta),
        Err(err) => {
            if let Some(meta) = parse_frontmatter_kv_lines(yaml) {
                return Ok(meta);
            }
            Err(err).context("unable to parse SKILL front-matter as YAML")?
        }
    }
}

fn parse_frontmatter_kv_lines(src: &str) -> Option<SkillMeta> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    for raw_line in src.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches(['"', '\'']);
        match key {
            "name" => {
                if name.is_none() && !value.is_empty() {
                    name = Some(value.to_string());
                }
            }
            "description" => {
                if description.is_none() && !value.is_empty() {
                    description = Some(value.to_string());
                }
            }
            _ => {}
        }
    }
    match (name, description) {
        (Some(name), Some(description)) => Some(SkillMeta { name, description }),
        _ => None,
    }
}

pub fn parse_frontmatter_file(path: &Path) -> Result<SkillMeta> {
    let data = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_skill_frontmatter_str(&data).context("invalid or missing SKILL.md front-matter")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_yaml_frontmatter() {
        let text = "---\nname: demo\ndescription: something\n---\nbody";
        let meta = parse_skill_frontmatter_str(text).unwrap();
        assert_eq!(meta.name, "demo");
        assert_eq!(meta.description, "something");
    }

    #[test]
    fn parses_plain_key_value_with_colon_in_value() {
        let text = "---\nname: starting-the-task\ndescription: A short checklist: plan, branch, test.\n---\nbody";
        let meta = parse_skill_frontmatter_str(text).unwrap();
        assert_eq!(meta.name, "starting-the-task");
        assert_eq!(meta.description, "A short checklist: plan, branch, test.");
    }
}
