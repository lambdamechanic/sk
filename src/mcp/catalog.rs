use crate::skills;
use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Clone)]
pub(crate) struct SkillRecord {
    pub(crate) install_name: String,
    pub(crate) skill_path: String,
    pub(crate) skill_file: String,
    pub(crate) meta: skills::SkillMeta,
    body: String,
    body_ascii_lower: String,
    search_blob: String,
}

impl SkillRecord {
    pub(crate) fn to_summary(&self, include_body: bool) -> SkillSummary {
        SkillSummary {
            install_name: self.install_name.clone(),
            name: self.meta.name.clone(),
            description: self.meta.description.clone(),
            skill_path: self.skill_path.clone(),
            skill_file: self.skill_file.clone(),
            body: include_body.then(|| self.body.clone()),
        }
    }

    pub(crate) fn to_detail(&self) -> SkillDetail {
        SkillDetail {
            install_name: self.install_name.clone(),
            name: self.meta.name.clone(),
            description: self.meta.description.clone(),
            skill_path: self.skill_path.clone(),
            skill_file: self.skill_file.clone(),
            body: self.body.clone(),
        }
    }

    pub(crate) fn score_for_tokens(&self, tokens: &[String]) -> Option<SearchMatch> {
        let mut score = 0;
        for token in tokens {
            if self.search_blob.contains(token) {
                score += 1;
            } else {
                return None;
            }
        }
        let excerpt = snippet_for_tokens(&self.body, &self.body_ascii_lower, tokens)
            .unwrap_or_else(|| self.meta.description.clone());
        Some(SearchMatch {
            install_name: self.install_name.clone(),
            skill_path: self.skill_path.clone(),
            skill_file: self.skill_file.clone(),
            meta: self.meta.clone(),
            score,
            excerpt,
        })
    }

    pub(crate) fn matches_query(&self, needle: &str) -> bool {
        self.search_blob.contains(needle)
    }
}

#[derive(Serialize)]
pub(crate) struct SkillSummary {
    pub(crate) install_name: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) skill_path: String,
    pub(crate) skill_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) body: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SkillDetail {
    pub(crate) install_name: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) skill_path: String,
    pub(crate) skill_file: String,
    pub(crate) body: String,
}

pub(crate) struct SearchMatch {
    pub(crate) install_name: String,
    pub(crate) skill_path: String,
    pub(crate) skill_file: String,
    pub(crate) meta: skills::SkillMeta,
    pub(crate) score: usize,
    pub(crate) excerpt: String,
}

pub(crate) fn scan_skills(project_root: &Path, skills_root: &Path) -> Result<Vec<SkillRecord>> {
    let mut records = Vec::new();
    for entry in WalkDir::new(skills_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("SKILL.md")
        })
    {
        let data = match fs::read_to_string(entry.path()) {
            Ok(data) => data,
            Err(err) => {
                eprintln!("warning: unable to read {}: {err}", entry.path().display());
                continue;
            }
        };
        let meta = match skills::parse_skill_frontmatter_str(&data) {
            Ok(meta) => meta,
            Err(err) => {
                eprintln!(
                    "warning: unable to parse front-matter for {}: {err}",
                    entry.path().display()
                );
                continue;
            }
        };
        let body = strip_frontmatter(&data).trim().to_string();
        let body_ascii_lower = body.to_ascii_lowercase();
        let install_name = entry
            .path()
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|f| f.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| meta.name.clone());
        let skill_path = relative_path(entry.path().parent().unwrap_or(skills_root), project_root);
        let skill_file = relative_path(entry.path(), project_root);
        let mut search_blob = format!(
            "{}\n{}\n{}\n{}",
            install_name, meta.name, meta.description, body
        );
        search_blob.make_ascii_lowercase();
        records.push(SkillRecord {
            install_name,
            skill_path,
            skill_file,
            meta,
            body,
            body_ascii_lower,
            search_blob,
        });
    }
    records.sort_by(|a, b| a.install_name.cmp(&b.install_name));
    Ok(records)
}

pub(crate) fn relative_path(path: &Path, project_root: &Path) -> String {
    let rel = path.strip_prefix(project_root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

fn strip_frontmatter(text: &str) -> &str {
    if !text.starts_with("---") {
        return text;
    }
    let mut offset = match text.find('\n') {
        Some(idx) => idx + 1,
        None => return text,
    };
    while offset < text.len() {
        let remainder = &text[offset..];
        match remainder.find('\n') {
            Some(rel_end) => {
                let line = &remainder[..rel_end];
                if line.trim_end_matches('\r') == "---" {
                    let mut body = &text[offset + rel_end + 1..];
                    body = body.strip_prefix('\r').unwrap_or(body);
                    body = body.strip_prefix('\n').unwrap_or(body);
                    return body;
                }
                offset += rel_end + 1;
            }
            None => {
                if remainder.trim_end_matches('\r') == "---" {
                    return "";
                }
                break;
            }
        }
    }
    text
}

fn snippet_for_tokens<'a>(
    body: &'a str,
    body_ascii_lower: &'a str,
    tokens: &[String],
) -> Option<String> {
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        if let Some(idx) = body_ascii_lower.find(token) {
            return Some(snippet_for_range(body, idx, token.len()));
        }
    }
    None
}

fn snippet_for_range(text: &str, idx: usize, token_len: usize) -> String {
    if text.is_empty() {
        return String::new();
    }
    let start = idx.saturating_sub(80);
    let end = (idx + token_len + 80).min(text.len());
    let snippet = text[start..end].trim();
    snippet.replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_skill(dir: &Path, name: &str, description: &str, body: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let path = skill_dir.join("SKILL.md");
        let contents = format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n");
        std::fs::write(&path, contents).unwrap();
    }

    #[test]
    fn scans_skill_metadata() {
        let project = tempdir().unwrap();
        let skills_root = project.path().join("skills");
        write_skill(&skills_root, "alpha", "Alpha skill", "Alpha body");
        write_skill(&skills_root, "beta", "Beta skill", "Use this skill.");

        let records = scan_skills(project.path(), &skills_root).unwrap();
        assert_eq!(records.len(), 2);
        let first = &records[0];
        assert_eq!(first.install_name, "alpha");
        assert!(first.search_blob.contains("alpha"));
        assert_eq!(first.body.trim(), "Alpha body");
    }

    #[test]
    fn search_scores_by_tokens() {
        let project = tempdir().unwrap();
        let skills_root = project.path().join("skills");
        write_skill(
            &skills_root,
            "notes",
            "Note keeper",
            "Use bd ready to find issues.",
        );
        write_skill(&skills_root, "sync", "Sync helper", "Sync skills via gh.");

        let records = scan_skills(project.path(), &skills_root).unwrap();
        let query = vec!["bd".to_string(), "ready".to_string()];
        let hits: Vec<_> = records
            .iter()
            .filter_map(|skill| skill.score_for_tokens(&query))
            .collect();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].excerpt.contains("bd ready"));
    }
}
