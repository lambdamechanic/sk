mod catalog;
mod transport;

use crate::{config, git, paths};
use anyhow::{Context, Result};
use catalog::{relative_path, scan_skills};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters, ServerHandler},
    model::{
        CallToolResult, Content, Implementation, ListResourcesResult, PaginatedRequestParam,
        ProtocolVersion, RawResource, ReadResourceRequestParam, ReadResourceResult, Resource,
        ResourceContents, ServerCapabilities, ServerInfo,
    },
    service::{Peer, RoleServer, ServiceExt},
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc as std_mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::{
    runtime::Builder as TokioRuntimeBuilder,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};

const MAX_SEARCH_LIMIT: usize = 25;
const DEFAULT_SEARCH_LIMIT: usize = 10;
const QUICKSTART_URI: &str = "sk://quickstart";
const QUICKSTART_DOC: &str = include_str!("../docs/AGENT_QUICKSTART.md");
const SERVER_INSTRUCTIONS: &str = "Start every task with skills_search to confirm whether a repo skill applies, then use skills_list or skills_show to pull the relevant body text when needed.";

pub fn run_server(root_override: Option<&str>) -> Result<()> {
    let project_root = git::ensure_git_repo()?;
    let cfg = config::load_or_default()?;
    let install_root_rel = root_override.unwrap_or(&cfg.default_root);
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
    let server = SkMcpServer::new(project_root, skills_root);
    let runtime = TokioRuntimeBuilder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;
    runtime.block_on(async move { serve_stdio(server).await })
}

async fn serve_stdio(server: SkMcpServer) -> Result<()> {
    let running = server
        .clone()
        .serve(stdio())
        .await
        .context("failed to start MCP server")?;
    let peer = running.peer().clone();
    let initialized_flag = server.initialized.clone();
    let (notify_tx, notify_rx) = mpsc::unbounded_channel();
    let watcher = spawn_tool_watcher(server.skills_root.clone(), notify_tx)
        .context("failed to start skills watcher")?;
    let notify_task =
        tokio::spawn(async move { forward_notifications(notify_rx, peer, initialized_flag).await });

    let wait_result = running.waiting().await;
    drop(watcher);
    notify_task.abort();
    match wait_result {
        Ok(_) => Ok(()),
        Err(err) => Err(err).context("server task failed"),
    }
}

#[derive(Debug, Clone, Copy)]
enum NotificationEvent {
    ToolsListChanged,
}

async fn forward_notifications(
    mut rx: UnboundedReceiver<NotificationEvent>,
    peer: Peer<RoleServer>,
    initialized: Arc<AtomicBool>,
) {
    while let Some(event) = rx.recv().await {
        if !initialized.load(Ordering::SeqCst) {
            continue;
        }
        match event {
            NotificationEvent::ToolsListChanged => {
                if let Err(err) = peer.notify_tool_list_changed().await {
                    eprintln!("failed to emit tools/list_changed notification: {err}");
                    break;
                }
            }
        }
    }
}

fn spawn_tool_watcher(
    skills_root: PathBuf,
    tx: UnboundedSender<NotificationEvent>,
) -> Result<RecommendedWatcher> {
    let (watch_tx, watch_rx) = std_mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = watch_tx.send(res);
    })?;
    watcher.watch(&skills_root, RecursiveMode::Recursive)?;
    thread::spawn(move || {
        let debounce = Duration::from_millis(500);
        let mut last_emit = Instant::now()
            .checked_sub(debounce)
            .unwrap_or_else(Instant::now);
        while let Ok(event) = watch_rx.recv() {
            match event {
                Ok(evt) => {
                    if transport::relevant_event(&evt.kind) {
                        let now = Instant::now();
                        if now.duration_since(last_emit) >= debounce {
                            last_emit = now;
                            let _ = tx.send(NotificationEvent::ToolsListChanged);
                        }
                    }
                }
                Err(err) => {
                    eprintln!("watch error: {err}");
                    break;
                }
            }
        }
    });
    Ok(watcher)
}

#[derive(Clone)]
struct SkMcpServer {
    project_root: PathBuf,
    skills_root: PathBuf,
    tool_router: ToolRouter<Self>,
    initialized: Arc<AtomicBool>,
}

impl SkMcpServer {
    fn new(project_root: PathBuf, skills_root: PathBuf) -> Self {
        Self {
            project_root,
            skills_root,
            tool_router: Self::tool_router(),
            initialized: Arc::new(AtomicBool::new(false)),
        }
    }

    fn relative_skills_root(&self) -> String {
        relative_path(&self.skills_root, &self.project_root)
    }

    fn list_skills(&self, args: ListArgs) -> Result<CallToolResult, McpError> {
        let skills =
            scan_skills(&self.project_root, &self.skills_root).map_err(to_internal_error)?;
        let filtered: Vec<_> = if let Some(query) = args.query.as_deref().map(str::trim) {
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
        let include_body = args.include_body.unwrap_or(false);
        let summaries: Vec<_> = filtered
            .iter()
            .map(|skill| skill.to_summary(include_body))
            .collect();
        let summary_text = format!(
            "Found {} skill{} under {}",
            summaries.len(),
            if summaries.len() == 1 { "" } else { "s" },
            self.relative_skills_root()
        );
        Ok(make_tool_result(
            vec![Content::text(summary_text)],
            json!({ "skills": summaries }),
        ))
    }

    fn search_skills(&self, args: SearchArgs) -> Result<CallToolResult, McpError> {
        let query = args.query.trim();
        if query.is_empty() {
            return Err(McpError::invalid_params("query must not be empty", None));
        }
        let tokens: Vec<String> = query
            .split_whitespace()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        if tokens.is_empty() {
            return Err(McpError::invalid_params(
                "query must include at least one token",
                None,
            ));
        }
        let limit = args
            .limit
            .unwrap_or(DEFAULT_SEARCH_LIMIT)
            .clamp(1, MAX_SEARCH_LIMIT);
        let skills =
            scan_skills(&self.project_root, &self.skills_root).map_err(to_internal_error)?;
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
        Ok(make_tool_result(
            vec![Content::text(text)],
            json!({
                "query": query,
                "limit": limit,
                "total": total,
                "results": results
            }),
        ))
    }

    fn show_skill(&self, args: ShowArgs) -> Result<CallToolResult, McpError> {
        let raw = args.skill_name.trim();
        if raw.is_empty() {
            return Err(McpError::invalid_params(
                "skillName must not be empty",
                None,
            ));
        }
        let skills =
            scan_skills(&self.project_root, &self.skills_root).map_err(to_internal_error)?;
        let Some(record) = skills
            .into_iter()
            .find(|skill| skill.meta.name.eq_ignore_ascii_case(raw))
        else {
            return Err(McpError::invalid_params(
                format!("unknown skill: {raw}"),
                None,
            ));
        };
        let detail = record.to_detail();
        let heading = format!(
            "{} ({}) — {}",
            detail.install_name, detail.name, detail.description
        );
        Ok(make_tool_result(
            vec![Content::text(heading), Content::text(detail.body.clone())],
            json!({ "skill": detail }),
        ))
    }

    fn quickstart_resource(&self) -> Resource {
        let mut raw = RawResource::new(QUICKSTART_URI, "sk-quickstart");
        raw.title = Some("sk Quickstart".into());
        raw.description =
            Some("Agent quickstart: install sk, cache Anthropic, publish + sync skills.".into());
        raw.mime_type = Some("text/markdown".into());
        raw.size = Some(QUICKSTART_DOC.len() as u32);
        Resource::new(raw, None)
    }
}

#[tool_router]
impl SkMcpServer {
    #[tool(
        name = "skills_list",
        description = "List installed skills, optionally filtered by name"
    )]
    async fn route_skills_list(
        &self,
        Parameters(args): Parameters<ListArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.list_skills(args)
    }

    #[tool(
        name = "skills_search",
        description = "Search skills stored under the repo's skills/ directory"
    )]
    async fn route_skills_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.search_skills(args)
    }

    #[tool(
        name = "skills_show",
        description = "Show the full SKILL.md body for a named skill"
    )]
    async fn route_skills_show(
        &self,
        Parameters(args): Parameters<ShowArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.show_skill(args)
    }
}

#[tool_handler]
impl ServerHandler for SkMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .enable_resources()
                .build(),
            server_info: server_implementation(),
            instructions: Some(SERVER_INSTRUCTIONS.to_string()),
        }
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        let resource = self.quickstart_resource();
        std::future::ready(Ok(ListResourcesResult::with_all_items(vec![resource])))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: rmcp::service::RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResult, McpError>> + Send + '_ {
        std::future::ready(if request.uri == QUICKSTART_URI {
            Ok(ReadResourceResult {
                contents: vec![ResourceContents::TextResourceContents {
                    uri: QUICKSTART_URI.to_string(),
                    mime_type: Some("text/markdown".into()),
                    text: QUICKSTART_DOC.to_string(),
                    meta: None,
                }],
            })
        } else {
            Err(McpError::resource_not_found(
                format!("unknown resource: {}", request.uri),
                None,
            ))
        })
    }

    fn on_initialized(
        &self,
        _context: rmcp::service::NotificationContext<RoleServer>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.initialized.store(true, Ordering::SeqCst);
        std::future::ready(())
    }
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ListArgs {
    query: Option<String>,
    include_body: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ShowArgs {
    skill_name: String,
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

fn make_tool_result(contents: Vec<Content>, structured: Value) -> CallToolResult {
    CallToolResult {
        content: contents,
        structured_content: Some(structured),
        is_error: Some(false),
        meta: None,
    }
}

fn to_internal_error(err: anyhow::Error) -> McpError {
    McpError::internal_error(err.to_string(), None)
}

fn server_implementation() -> Implementation {
    Implementation {
        name: "sk".into(),
        title: Some("sk".into()),
        version: env!("CARGO_PKG_VERSION").into(),
        icons: None,
        website_url: None,
    }
}
