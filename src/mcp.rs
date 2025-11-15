mod catalog;
mod transport;

use crate::{config, git, paths};
use anyhow::{anyhow, Context, Result};
use catalog::{relative_path, scan_skills};
use crossbeam_channel::{unbounded, RecvError, Sender};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::{Duration, Instant};
use transport::{
    relevant_event, send_error, send_notification, send_response, spawn_reader_thread,
};

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

#[derive(Clone, Copy, Debug)]
enum NotificationEvent {
    ToolsListChanged,
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
        let stdout = std::io::stdout();
        let mut writer = BufWriter::new(stdout.lock());

        let (frame_tx, frame_rx) = unbounded();
        spawn_reader_thread(frame_tx.clone());

        let (notify_tx, notify_rx) = unbounded();
        let _watcher = self.spawn_watcher(notify_tx.clone())?;

        loop {
            crossbeam_channel::select! {
                recv(frame_rx) -> msg => match msg {
                    Ok(value) => {
                        if let Err(err) = self.handle_frame(value, &mut writer) {
                            eprintln!("mcp server error: {err:#}");
                        }
                    }
                    Err(RecvError) => break,
                },
                recv(notify_rx) -> msg => match msg {
                    Ok(event) => {
                        if let Err(err) = self.handle_notification(event, &mut writer) {
                            eprintln!("failed to emit notification: {err:#}");
                        }
                    }
                    Err(RecvError) => break,
                }
            };
            writer.flush()?;
        }
        Ok(())
    }

    fn spawn_watcher(&self, tx: Sender<NotificationEvent>) -> Result<RecommendedWatcher> {
        let skills_root = self.skills_root.clone();
        let (watch_tx, watch_rx) = std_mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = watch_tx.send(res);
        })?;
        watcher.watch(&skills_root, RecursiveMode::Recursive)?;

        let debounce = Duration::from_millis(500);
        thread::spawn(move || {
            let mut last_emit = Instant::now()
                .checked_sub(debounce)
                .unwrap_or_else(Instant::now);
            while let Ok(event) = watch_rx.recv() {
                match event {
                    Ok(evt) => {
                        if relevant_event(&evt.kind) {
                            let now = Instant::now();
                            if now.duration_since(last_emit) >= debounce {
                                last_emit = now;
                                let _ = tx.send(NotificationEvent::ToolsListChanged);
                            }
                        }
                    }
                    Err(err) => eprintln!("watch error: {err}"),
                }
            }
        });

        Ok(watcher)
    }

    fn handle_notification(
        &mut self,
        event: NotificationEvent,
        writer: &mut BufWriter<std::io::StdoutLock<'_>>,
    ) -> Result<()> {
        match event {
            NotificationEvent::ToolsListChanged => {
                if self.initialized {
                    send_notification(
                        writer,
                        "notifications/tools/list_changed",
                        json!({
                            "reason": "skills directory changed"
                        }),
                    )?;
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
            "notifications/initialized" => {}
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
                    .filter(|skill| skill.matches_query(&needle))
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
