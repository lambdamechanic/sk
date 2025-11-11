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
        if let Some(meta) = parse_skill_frontmatter(&text) {
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

fn parse_skill_frontmatter(text: &str) -> Option<SkillMeta> {
    // Expect leading --- YAML --- frontmatter
    let re = Regex::new(r"(?s)^---\n(.*?)\n---").ok()?;
    let caps = re.captures(text)?;
    let yaml = caps.get(1)?.as_str();
    serde_yaml::from_str::<SkillMeta>(yaml).ok()
}

pub fn parse_frontmatter_file(path: &Path) -> Result<SkillMeta> {
    let data = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_skill_frontmatter(&data).context("invalid or missing SKILL.md front-matter")
}
