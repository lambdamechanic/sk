use crate::{config, git, paths, skills};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const DEFAULT_PROTOCOL_VERSION: &str = "2025-03-26";
const MAX_SEARCH_LIMIT: usize = 25;
const DEFAULT_SEARCH_LIMIT: usize = 10;

pub fn run_server(root_override: Option<&str>) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let default_root = cfg.default_root.clone();
    let install_root_rel = root_override.unwrap_or(&default_root);
    let skills_root = paths::resolve_project_path(&project_root, install_root_rel);
    if !skills_root.exists() {
        anyhow::bail!(
            "skills root {} does not exist",
            skills_root
                .strip_prefix(&project_root)
                .unwrap_or(&skills_root)
                .display()
        );
    }
    let mut server = McpServer::new(project_root, skills_root);
    server.run()
}

struct McpServer {
    project_root: PathBuf,
    skills_root: PathBuf,
    initialized: bool,
}

impl McpServer {
    fn new(project_root: PathBuf, skills_root: PathBuf) -> Self {
        Self {
            project_root,
            skills_root,
            initialized: false,
        }
    }

    fn run(&mut self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let reader = BufReader::new(stdin.lock());
        let mut writer = BufWriter::new(stdout.lock());
        let stream = serde_json::Deserializer::from_reader(reader).into_iter::<Value>();
        for frame in stream {
            match frame {
                Ok(value) => {
                    if let Err(err) = self.handle_frame(value, &mut writer) {
                        eprintln!("mcp server error: {err:#}");
                    }
                }
                Err(err) => {
                    eprintln!("failed to decode MCP frame: {err}");
                }
            }
        }
        Ok(())
    }

    fn handle_frame(
        &mut self,
        value: Value,
        writer: &mut BufWriter<std::io::StdoutLock<'_>>,
    ) -> Result<()> {
        let Some(method) = value
            .get("method")
            .and_then(|m| m.as_str())
            .map(str::to_string)
        else {
            // Response destined for the client; ignore.
            return Ok(());
        };
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        let id = value.get("id").cloned();
        match method.as_str() {
            "initialize" => {
                if let Some(id) = id {
                    self.initialized = true;
                    let result = json!({
                        "protocolVersion": DEFAULT_PROTOCOL_VERSION,
                        "capabilities": {
                            "tools": {
                                "listChanged": false
                            }
                        },
                        "serverInfo": {
                            "name": "sk-mcp",
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                        "instructions": "Use skills.list to enumerate available repo skills and skills.search to find a relevant skill by keyword."
                    });
                    send_response(id, result, writer)?;
                }
            }
            "notifications/initialized" => {
                // Acknowledge silently; no response needed.
            }
            "tools/list" => {
                if let Some(id) = id {
                    let result = json!({
                        "tools": tool_definitions(),
                    });
                    send_response(id, result, writer)?;
                }
            }
            "tools/call" => {
                if let Some(id) = id {
                    match self.handle_tools_call(params) {
                        Ok(resp) => send_response(id, resp, writer)?,
                        Err(err) => send_error(
                            id,
                            -32602,
                            &format!("tool invocation failed: {err}"),
                            None,
                            writer,
                        )?,
                    }
                }
            }
            _ => {
                if let Some(id) = id {
                    send_error(
                        id,
                        -32601,
                        &format!("unsupported method: {method}"),
                        None,
                        writer,
                    )?;
                }
            }
        }
        writer.flush()?;
        Ok(())
    }

    fn handle_tools_call(&self, params: Value) -> Result<Value> {
        let parsed: ToolCall = serde_json::from_value(params.clone())
            .context("invalid tools/call payload (expected name + arguments)")?;
        match parsed.name.as_str() {
            "skills.list" => {
                let args: ListArgs =
                    serde_json::from_value(parsed.arguments.unwrap_or(Value::Null))
                        .context("invalid arguments for skills.list")?;
                let payload = self.list_skills(args)?;
                Ok(payload)
            }
            "skills.search" => {
                let args: SearchArgs =
                    serde_json::from_value(parsed.arguments.unwrap_or(Value::Null))
                        .context("invalid arguments for skills.search")?;
                let payload = self.search_skills(args)?;
                Ok(payload)
            }
            other => Err(anyhow!("unknown tool: {other}")),
        }
    }

    fn list_skills(&self, args: ListArgs) -> Result<Value> {
        let skills = scan_skills(&self.project_root, &self.skills_root)?;
        let filtered: Vec<_> = if let Some(query) = args.query.as_ref().map(|s| s.trim()) {
            if query.is_empty() {
                skills
            } else {
                let needle = query.to_ascii_lowercase();
                skills
                    .into_iter()
                    .filter(|skill| skill.search_blob.contains(&needle))
                    .collect()
            }
        } else {
            skills
        };
        let summaries: Vec<_> = filtered
            .iter()
            .map(|skill| skill.to_summary(args.include_body.unwrap_or(false)))
            .collect();
        let summary_text = format!(
            "Found {} skill{} under {}",
            summaries.len(),
            if summaries.len() == 1 { "" } else { "s" },
            self.relative_skills_root()
        );
        Ok(json!({
            "content": [
                {
                    "type": "text",
                    "text": summary_text
                }
            ],
            "structuredContent": {
                "skills": summaries
            }
        }))
    }

    fn search_skills(&self, args: SearchArgs) -> Result<Value> {
        let query = args.query.trim();
        if query.is_empty() {
            anyhow::bail!("query must not be empty");
        }
        let tokens: Vec<String> = query
            .split_whitespace()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        if tokens.is_empty() {
            anyhow::bail!("query must include at least one token");
        }
        let limit = args
            .limit
            .unwrap_or(DEFAULT_SEARCH_LIMIT)
            .clamp(1, MAX_SEARCH_LIMIT);
        let skills = scan_skills(&self.project_root, &self.skills_root)?;
        let mut hits: Vec<_> = skills
            .iter()
            .filter_map(|skill| skill.score_for_tokens(&tokens))
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.install_name.cmp(&b.install_name))
        });
        let total = hits.len();
        hits.truncate(limit);
        let results: Vec<_> = hits
            .into_iter()
            .map(|hit| SearchHitPayload {
                name: hit.meta.name,
                description: hit.meta.description,
                install_name: hit.install_name,
                skill_path: hit.skill_path,
                skill_file: hit.skill_file,
                score: hit.score,
                excerpt: hit.excerpt,
            })
            .collect();
        let text = if results.is_empty() {
            format!("No skills matched \"{query}\".")
        } else {
            format!(
                "{} result{} ({} total matches) for \"{query}\".",
                results.len(),
                if results.len() == 1 { "" } else { "s" },
                total
            )
        };
        Ok(json!({
            "content": [
                {
                    "type": "text",
                    "text": text
                }
            ],
            "structuredContent": {
                "query": query,
                "limit": limit,
                "total": total,
                "results": results
            }
        }))
    }

    fn relative_skills_root(&self) -> String {
        relative_path(&self.skills_root, &self.project_root)
    }
}

#[derive(Deserialize)]
struct ToolCall {
    name: String,
    #[serde(default)]
    arguments: Option<Value>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListArgs {
    query: Option<String>,
    include_body: Option<bool>,
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Clone)]
struct SkillRecord {
    install_name: String,
    skill_path: String,
    skill_file: String,
    meta: skills::SkillMeta,
    body: String,
    body_ascii_lower: String,
    search_blob: String,
}

impl SkillRecord {
    fn to_summary(&self, include_body: bool) -> SkillSummary {
        SkillSummary {
            install_name: self.install_name.clone(),
            name: self.meta.name.clone(),
            description: self.meta.description.clone(),
            skill_path: self.skill_path.clone(),
            skill_file: self.skill_file.clone(),
            body: include_body.then(|| self.body.clone()),
        }
    }

    fn score_for_tokens(&self, tokens: &[String]) -> Option<SearchMatch> {
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
}

#[derive(Serialize)]
struct SkillSummary {
    install_name: String,
    name: String,
    description: String,
    skill_path: String,
    skill_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
}

struct SearchMatch {
    install_name: String,
    skill_path: String,
    skill_file: String,
    meta: skills::SkillMeta,
    score: usize,
    excerpt: String,
}

#[derive(Serialize)]
struct SearchHitPayload {
    name: String,
    description: String,
    install_name: String,
    skill_path: String,
    skill_file: String,
    score: usize,
    excerpt: String,
}

fn scan_skills(project_root: &Path, skills_root: &Path) -> Result<Vec<SkillRecord>> {
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

fn relative_path(path: &Path, project_root: &Path) -> String {
    let rel = path.strip_prefix(project_root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

fn send_response(
    id: Value,
    result: Value,
    writer: &mut BufWriter<std::io::StdoutLock<'_>>,
) -> Result<()> {
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    let mut buffer = Vec::new();
    serde_json::to_writer(&mut buffer, &response)?;
    buffer.push(b'\n');
    writer.write_all(&buffer)?;
    Ok(())
}

fn send_error(
    id: Value,
    code: i32,
    message: &str,
    data: Option<Value>,
    writer: &mut BufWriter<std::io::StdoutLock<'_>>,
) -> Result<()> {
    let mut error = json!({
        "code": code,
        "message": message,
    });
    if let Some(data) = data {
        error["data"] = data;
    }
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error
    });
    let mut buffer = Vec::new();
    serde_json::to_writer(&mut buffer, &response)?;
    buffer.push(b'\n');
    writer.write_all(&buffer)?;
    Ok(())
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "skills.list",
            "title": "List repo skills",
            "description": "Enumerate every SKILL.md under the repo's skills root with metadata and optional body text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Optional substring filter applied to install name, SKILL name, or description."
                    },
                    "includeBody": {
                        "type": "boolean",
                        "description": "Include the SKILL.md body (without YAML front-matter) in the structured response."
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "skills.search",
            "title": "Search repo skills",
            "description": "Search SKILL metadata and body text for keywords to quickly find relevant instructions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keywords to match (all tokens must appear).",
                        "minLength": 1
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of hits to return.",
                        "minimum": 1,
                        "maximum": MAX_SEARCH_LIMIT,
                        "default": DEFAULT_SEARCH_LIMIT
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_skill(dir: &Path, name: &str, description: &str, body: &str) -> PathBuf {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        let path = skill_dir.join("SKILL.md");
        let contents = format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n");
        fs::write(&path, contents).unwrap();
        path
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
