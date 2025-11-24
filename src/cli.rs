use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "sk", version, about = "Repo-scoped Claude Skills manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Init {
        #[arg(long)]
        root: Option<String>,
    },
    Install {
        repo: String,
        skill_name: String,
        #[arg(long)]
        alias: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        https: bool,
    },
    List {
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Where {
        installed_name: String,
    },
    #[command(hide = true, about = "DEPRECATED: use `sk doctor --summary` instead")]
    Check {
        names: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(hide = true, about = "DEPRECATED: use `sk doctor --status` instead")]
    Status {
        names: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Show diffs between local skills and their remote repos")]
    Diff {
        names: Vec<String>,
    },
    Update,
    Upgrade {
        #[arg(allow_hyphen_values = true)]
        target: String, // installed-name or --all
        #[arg(long)]
        dry_run: bool,
    },
    Remove {
        installed_name: String,
        #[arg(long)]
        force: bool,
    },
    SyncBack {
        installed_name: String,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        message: Option<String>,
        #[arg(
            long,
            help = "Target repo for new skills (URL, file://, or @owner/repo)"
        )]
        repo: Option<String>,
        #[arg(
            long = "skill-path",
            help = "Subdirectory inside the repo; defaults to installed name"
        )]
        skill_path: Option<String>,
        #[arg(
            long,
            help = "Use HTTPS when resolving @owner/repo shorthand (default SSH)"
        )]
        https: bool,
    },
    #[command(
        about = "Analyze and repair cache and lockfile",
        long_about = "Analyze project state and optionally repair problems.\n\
 - Detect duplicate installName entries in skills.lock.json.\n\
 - Report digest drift, missing cache clones, and missing locked commits.\n\
 - With --apply: rebuild missing installs from the locked commit when possible;\n\
   drop unrecoverable (orphan) lock entries and normalize lockfile ordering/timestamp;\n\
   prune unreferenced cache clones under the cache root (~/.cache/sk/repos)."
    )]
    Doctor {
        names: Vec<String>,
        #[arg(long)]
        root: Option<String>,
        #[arg(
            long,
            conflicts_with_all = ["status", "diff"],
            help = "Show the lightweight ok/modified/missing view (replacement for `sk check`)."
        )]
        summary: bool,
        #[arg(
            long,
            conflicts_with_all = ["summary", "diff"],
            help = "Show digest + upgrade info (replacement for `sk status`)."
        )]
        status: bool,
        #[arg(
            long,
            conflicts_with_all = ["summary", "status"],
            help = "Show diffs against the cached remote tip (replacement for `sk diff`)."
        )]
        diff: bool,
        #[arg(long, help = "Emit JSON (summary/status modes only).")]
        json: bool,
        #[arg(
            long,
            conflicts_with_all = ["summary", "status", "diff"],
            help = "Apply repairs: rebuild missing installs, drop orphan lock entries, prune unreferenced caches, normalize lockfile"
        )]
        apply: bool,
    },
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    Template {
        #[command(subcommand)]
        cmd: TemplateCmd,
    },
    Repo {
        #[command(subcommand)]
        cmd: RepoCmd,
    },
    #[command(about = "Pre-commit checks (warn on local sources)")]
    Precommit {
        #[arg(long, help = "Allow local file:// sources without failing")]
        allow_local: bool,
    },
    #[command(about = "Run the repo-scoped MCP skills server over stdio")]
    McpServer {
        #[arg(
            long,
            help = "Override the skills root (defaults to sk config default_root)"
        )]
        root: Option<String>,
    },
    #[command(about = "Generate shell completions")]
    Completions {
        #[arg(long, help = "The shell to generate completions for")]
        shell: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    Get { key: String },
    Set { key: String, value: String },
}

#[derive(Subcommand, Debug)]
pub enum TemplateCmd {
    Create {
        name: String,
        description: String,
        #[arg(long)]
        root: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum RepoCmd {
    #[command(about = "Cache a remote repo without installing a skill")]
    Add {
        repo: String,
        #[arg(long)]
        alias: Option<String>,
        #[arg(long, help = "Use HTTPS when resolving @owner/repo shorthand")]
        https: bool,
    },
    #[command(about = "List cached repos")]
    List {
        #[arg(long)]
        json: bool,
    },
    #[command(
        hide = true,
        about = "DEPRECATED: use `sk repo search --repo <alias-or-repo> --all` instead"
    )]
    Catalog {
        target: String,
        #[arg(long, help = "Use HTTPS when resolving @owner/repo shorthand")]
        https: bool,
        #[arg(long)]
        json: bool,
    },
    #[command(
        about = "Search cached repos for matching skills (omit the query or pass --all to list every skill)"
    )]
    Search {
        query: Option<String>,
        #[arg(long, help = "Limit search to a specific alias or repo input")]
        repo: Option<String>,
        #[arg(long, help = "Use HTTPS when resolving @owner/repo shorthand")]
        https: bool,
        #[arg(long)]
        json: bool,
        #[arg(
            long,
            help = "List every skill in the targeted repo (replacement for `sk repo catalog`)"
        )]
        all: bool,
    },
    #[command(about = "Remove a cached repo entry")]
    Remove {
        target: String,
        #[arg(long, help = "Use HTTPS when resolving @owner/repo shorthand")]
        https: bool,
        #[arg(long)]
        json: bool,
    },
}
