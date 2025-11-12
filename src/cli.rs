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
        #[arg(long = "ref")]
        r#ref: Option<String>,
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
        #[arg(long)]
        root: Option<String>,
    },
    Check {
        names: Vec<String>,
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Status {
        names: Vec<String>,
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Update,
    Upgrade {
        #[arg(allow_hyphen_values = true)]
        target: String, // installed-name or --all
        #[arg(long = "ref")]
        r#ref: Option<String>,
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        include_pinned: bool,
    },
    Remove {
        installed_name: String,
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        force: bool,
    },
    SyncBack {
        installed_name: String,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        message: Option<String>,
        #[arg(long)]
        root: Option<String>,
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
        #[arg(
            long,
            help = "Apply repairs: rebuild missing installs, drop orphan lock entries, prune unreferenced caches, normalize lockfile"
        )]
        apply: bool,
    },
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    Get { key: String },
    Set { key: String, value: String },
}
