use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
#[derive(Debug, Parser)]
#[command(name="ShadowCode", bin_name="shadowcode", version=crate::VERSION, propagate_version=true, about="A native workspace for local coding models")]
pub struct Options {
    /// Project folder (defaults to the current directory for CLI commands).
    #[arg(long, visible_alias = "project", short = 'p', global = true)]
    pub workspace: Option<PathBuf>,
    /// Isolated settings and history directory.
    #[arg(long, global = true)]
    pub profile: Option<PathBuf>,
    /// Print one structured JSON result on stdout.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open the desktop window (also the default).
    #[command(display_name = "ShadowCode")]
    Ui,
    /// Open the native full-screen terminal workspace.
    Tui {
        #[arg(long, short = 's')]
        session: Option<String>,
    },
    /// Run a task, stream its progress, and return its actual exit status.
    Run(Run),
    /// Run an exact user-supplied terminal command.
    Exec {
        command: String,
        #[arg(long, default_value_t = 60)]
        timeout: u64,
    },
    /// Run a slash command, or list available commands with no argument.
    Command {
        name: Option<String>,
        #[arg(default_value = "")]
        args: String,
        #[command(flatten)]
        task: TaskOptions,
    },
    /// List or run a project skill.
    Skill {
        name: Option<String>,
        #[arg(default_value = "")]
        args: String,
        #[arg(long)]
        list: bool,
        #[command(flatten)]
        task: TaskOptions,
    },
    /// Show the selected project and active work.
    Status,
    /// List tables or run one read-only query against an existing project database.
    Sqlite {
        path: String,
        sql: Option<String>,
        /// JSON array of values bound to positional SQL placeholders.
        #[arg(long, default_value = "[]", requires = "sql")]
        params: String,
        #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u16).range(1..=1000))]
        limit: u16,
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..=10000))]
        timeout_ms: u64,
    },
    /// Inspect native runtime and configured provider health.
    Health {
        #[arg(long)]
        test_model: bool,
    },
    /// Inspect native installation, profile, project and optional model response.
    Doctor {
        #[arg(long)]
        test_model: bool,
    },
    /// Read or append project notes, or select an exact task's notes.
    Memory {
        note: Option<String>,
        #[arg(long)]
        task: Option<String>,
        /// Replace notes (an empty string clears them) after reading their hash.
        #[arg(long, requires_all = ["note", "expected_hash"])]
        replace: bool,
        #[arg(long, requires = "replace")]
        expected_hash: Option<String>,
    },
    /// Inspect the project without running code or a model.
    Understand {
        #[arg(long)]
        save: bool,
    },
    /// Show recorded commit history and pending changes for a path.
    Why {
        path: Option<String>,
        #[arg(long, default_value_t=8, value_parser=clap::value_parser!(u16).range(1..=50))]
        count: u16,
    },
    /// List, register, or select models.
    Models {
        #[arg(long = "use", short = 'u')]
        selected: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long, requires = "provider")]
        endpoint: Option<String>,
        #[arg(long, requires = "provider")]
        api_key_env: Option<String>,
        #[arg(long, requires = "provider")]
        context_limit: Option<usize>,
        #[arg(long)]
        no_detect: bool,
    },
    /// Read settings or set a validated dotted key to a JSON value or text.
    Config {
        key: Option<String>,
        value: Option<String>,
    },
    /// Explicitly trust the current project for task execution.
    Trust,
    /// List lifecycle commands, or explicitly activate an unchanged project hook.
    Hooks {
        #[arg(long, conflicts_with = "disable", requires = "hash")]
        enable: Option<String>,
        #[arg(long, conflicts_with = "enable")]
        disable: Option<String>,
        /// Content hash shown by `hooks --json`; required when enabling.
        #[arg(long, requires = "enable")]
        hash: Option<String>,
    },
    /// List inert MCP definitions, register one, or review a project activation.
    Mcp {
        #[command(subcommand)]
        action: Option<Mcp>,
    },
    /// Review, install or remove declarative plugins in the selected project.
    Plugin {
        #[command(subcommand)]
        action: Option<Plugin>,
    },
    /// List/search saved conversations, or rename/delete a unique ID prefix.
    Sessions {
        query: Option<String>,
        #[arg(long, conflicts_with = "delete")]
        rename: Option<String>,
        #[arg(long)]
        delete: bool,
    },
    /// Export a complete saved conversation to stdout or an atomic file.
    Export {
        #[arg(long, short = 's')]
        session: Option<String>,
        #[arg(long, default_value="md", value_parser=["md","json"])]
        format: String,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    /// List jobs, inspect one, watch its events, or cancel it.
    Jobs {
        id: Option<String>,
        #[arg(long, conflicts_with = "cancel")]
        watch: bool,
        #[arg(long)]
        cancel: bool,
        #[arg(long)]
        interactive: bool,
    },
    /// List pending approvals or decide one explicitly.
    Approvals {
        #[arg(long, short = 's')]
        session: Option<String>,
        #[arg(long)]
        id: Option<String>,
        #[arg(long, requires="id", value_parser=["approve","deny"])]
        decision: Option<String>,
    },
    /// Create a durable goal and optionally run its milestones.
    Goal {
        instruction: String,
        #[arg(long)]
        run: bool,
        #[arg(long, requires = "run")]
        detach: bool,
        #[arg(long)]
        interactive: bool,
    },
    /// List goals, resume one, or pause it.
    Goals {
        #[arg(long, conflicts_with = "pause")]
        resume: Option<String>,
        #[arg(long)]
        pause: Option<String>,
        #[arg(long)]
        interactive: bool,
    },
    /// Manage servers and watchers owned by the active desktop or headless server.
    Background {
        #[command(subcommand)]
        action: Background,
    },
    /// List or restore this conversation's latest file-tool checkpoint.
    Checkpoints {
        #[arg(long, short = 's')]
        session: String,
        #[arg(long)]
        restore: Option<String>,
        #[arg(long, conflicts_with = "restore")]
        undo: bool,
    },
    /// Run the shared engine without a window until Ctrl-C or SIGTERM.
    Serve,
}

#[derive(Debug, Subcommand)]
pub enum Plugin {
    /// Show all generated files before installation.
    Inspect {
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        name: Option<String>,
        /// Read a local JSON bundle; no network download or code execution.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Install a bundle whose hash was returned by inspect.
    Install {
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        name: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        hash: String,
    },
    /// Remove unchanged owned files and preserve local edits.
    Remove {
        name: String,
        /// Current installation hash from the plugin catalog.
        #[arg(long)]
        hash: String,
    },
}
#[derive(Debug, Subcommand)]
pub enum Mcp {
    /// Expose this project over stdio or authenticated loopback HTTP, without a window.
    Serve {
        /// Delegate changes within the configured project permissions.
        #[arg(long)]
        allow_write: bool,
        /// Delegate approval decisions for this MCP owner's tasks.
        #[arg(long, requires = "allow_write")]
        allow_approvals: bool,
        /// Bind a loopback IP:port for Streamable HTTP instead of stdin/stdout.
        #[arg(long, requires = "token_env")]
        http: Option<std::net::SocketAddr>,
        /// Environment/profile-secret name containing a random bearer token (32–512 characters).
        #[arg(long, requires = "http")]
        token_env: Option<String>,
    },
    /// Print MCP client configuration without editing another application's settings.
    Register {
        #[arg(long, conflicts_with = "url")]
        allow_write: bool,
        #[arg(long, requires = "allow_write")]
        allow_approvals: bool,
        /// Output format: generic JSON, Claude Code, Cursor, or Codex TOML.
        #[arg(long, value_enum, default_value = "generic")]
        client: McpClient,
        /// An already-running local HTTP gateway's /mcp URL (otherwise use stdio).
        #[arg(long, requires = "token_env")]
        url: Option<String>,
        /// Credential variable name for the calling application; never reads its value.
        #[arg(long, requires = "url")]
        token_env: Option<String>,
    },

    /// Register a JSON/YAML definition; does not launch or enable it.
    Add {
        definition: PathBuf,
        /// Current definition hash when replacing an existing registration.
        #[arg(long)]
        hash: Option<String>,
    },
    /// Allow this exact server definition to start for new Build tasks here.
    Enable {
        server: String,
        #[arg(long)]
        hash: String,
    },
    /// Revoke this project's activation for future tasks.
    Disable { server: String },
    /// Remove a global registration and its project activations.
    Remove {
        server: String,
        #[arg(long)]
        hash: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum McpClient {
    Generic,
    Claude,
    Cursor,
    Codex,
}
#[derive(Debug, Args)]
pub struct Run {
    pub task: String,
    #[command(flatten)]
    pub options: TaskOptions,
}
#[derive(Debug, Default, Args)]
pub struct TaskOptions {
    #[arg(long, short = 's')]
    pub session: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long, visible_alias = "agent", default_value = "coder")]
    pub purpose: String,
    /// Queue behind a task already running in this project.
    #[arg(long)]
    pub queue: bool,
    /// Ask for tool approval in this terminal (requires a TTY).
    #[arg(long)]
    pub interactive: bool,
    /// Emit newline-delimited event JSON followed by the final result.
    #[arg(long, conflicts_with = "json")]
    pub events: bool,
    /// Leave the task running in a persistent desktop/server engine.
    #[arg(long)]
    pub detach: bool,
    /// How to handle a tool approval without an interactive terminal.
    #[arg(long, value_enum, default_value = "cancel")]
    pub approval: ApprovalMode,
}
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum ApprovalMode {
    #[default]
    Cancel,
    Wait,
}
#[derive(Debug, Subcommand)]
pub enum Background {
    List,
    Start {
        #[arg(long, default_value = "background")]
        name: String,
        #[arg(long)]
        command: String,
    },
    Stop {
        id: String,
    },
    Logs {
        id: String,
    },
}
