use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "resguard",
    about = "Linux resource guard using systemd slices",
    version = env!("CARGO_PKG_VERSION")
)]
pub struct Cli {
    #[arg(long, global = true, default_value = "table")]
    pub format: String,
    #[arg(long, global = true, help = "Emit structured logs to stderr")]
    pub json_log: bool,
    #[arg(long, global = true)]
    pub verbose: bool,
    #[arg(long, global = true)]
    pub quiet: bool,
    #[arg(long, global = true)]
    pub no_color: bool,
    #[arg(long, global = true, default_value = "/")]
    pub root: String,
    #[arg(long, global = true, default_value = "/etc/resguard")]
    pub config_dir: String,
    #[arg(long, global = true, default_value = "/var/lib/resguard")]
    pub state_dir: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Init {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        out: Option<String>,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        dry_run: bool,
    },
    Setup {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        apply: bool,
        #[arg(
            long,
            default_value_t = true,
            action = clap::ArgAction::Set,
            help = "Run safe suggest preview after bootstrap"
        )]
        suggest: bool,
        #[arg(
            long,
            default_value_t = true,
            action = clap::ArgAction::Set,
            help = "Plan auto-wrap candidates for strong-confidence matches"
        )]
        plan_wraps: bool,
    },
    Apply {
        profile: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_oomd: bool,
        #[arg(long)]
        no_cpu: bool,
        #[arg(long)]
        no_classes: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        user_daemon_reload: bool,
    },
    Diff {
        profile: String,
    },
    Rollback {
        #[arg(long)]
        last: bool,
        #[arg(long)]
        to: Option<String>,
    },
    Doctor,
    Metrics,
    Monitor {
        #[arg(long, help = "Live refresh mode")]
        watch: bool,
        #[arg(
            long,
            default_value_t = 1000,
            help = "Refresh interval in milliseconds"
        )]
        interval: u64,
        #[arg(long, help = "Plain script-safe output (no ANSI color)")]
        plain: bool,
    },
    Top {
        #[arg(
            long,
            default_value_t = 3,
            help = "Notable active scopes shown per class"
        )]
        scopes: usize,
        #[arg(long, help = "Plain script-safe output (no ANSI color)")]
        plain: bool,
    },
    #[cfg(feature = "tui")]
    Tui {
        #[arg(long, default_value_t = 1000)]
        interval: u64,
        #[arg(long)]
        no_top: bool,
    },
    Panic {
        #[arg(long, help = "Temporary panic duration like 30s, 10m, 1h")]
        duration: Option<String>,
    },
    Status,
    Suggest {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        apply: bool,
        #[arg(
            long,
            help = "Safe zero-config automation for strong desktop matches only"
        )]
        auto: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value_t = 70)]
        confidence_threshold: u8,
    },
    Run {
        #[arg(
            long,
            help = "Resource class (recommended). If omitted, strong auto-detect is used."
        )]
        class: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        slice: Option<String>,
        #[arg(long, help = "Skip slice existence check (unsafe, poweruser only)")]
        no_check: bool,
        #[arg(long)]
        wait: bool,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    Rescue {
        #[arg(long, default_value = "rescue")]
        class: String,
        #[arg(long, help = "Custom shell command to run instead of htop/top")]
        command: Option<String>,
        #[arg(long, help = "Start an interactive shell without launching htop/top")]
        no_ui: bool,
        #[arg(
            long,
            help = "If rescue class/slice is missing, fallback to system.slice (unsafe, poweruser only)"
        )]
        no_check: bool,
    },
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },
    Desktop {
        #[command(subcommand)]
        cmd: DesktopCmd,
    },
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
    },
    Completion {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    Version,
}

#[derive(Subcommand, Debug)]
pub enum ProfileCmd {
    List,
    Show {
        name: String,
    },
    Import {
        file: String,
    },
    Export {
        name: String,
        #[arg(long)]
        out: String,
    },
    Validate {
        target: String,
    },
    New {
        name: String,
        #[arg(long)]
        from: Option<String>,
    },
    Edit {
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum DesktopCmd {
    List {
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, value_enum, default_value_t = DesktopOrigin::All)]
        origin: DesktopOrigin,
    },
    Wrap {
        desktop_id: String,
        #[arg(long)]
        class: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long = "print")]
        print_only: bool,
        #[arg(long = "override")]
        override_mode: bool,
        #[arg(long)]
        force: bool,
    },
    Unwrap {
        desktop_id: String,
        #[arg(long)]
        class: String,
        #[arg(long = "override")]
        override_mode: bool,
    },
    Doctor,
}

#[derive(Subcommand, Debug)]
pub enum DaemonCmd {
    Enable,
    Disable,
    Status,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum DesktopOrigin {
    User,
    System,
    All,
}

#[derive(Debug)]
pub struct ApplyOptions {
    pub dry_run: bool,
    pub no_oomd: bool,
    pub no_cpu: bool,
    pub no_classes: bool,
    pub force: bool,
    pub user_daemon_reload: bool,
}

#[derive(Debug)]
pub struct RunRequest {
    pub class: Option<String>,
    pub profile_override: Option<String>,
    pub slice_override: Option<String>,
    pub no_check: bool,
    pub wait: bool,
    pub command: Vec<String>,
}

#[derive(Debug)]
pub struct SuggestRequest {
    pub format: String,
    pub root: String,
    pub config_dir: String,
    pub state_dir: String,
    pub profile: Option<String>,
    pub apply: bool,
    pub auto: bool,
    pub dry_run: bool,
    pub confidence_threshold: u8,
}

#[derive(Debug)]
pub struct SetupRequest {
    pub format: String,
    pub root: String,
    pub config_dir: String,
    pub state_dir: String,
    pub name: Option<String>,
    pub apply: bool,
    pub suggest: bool,
    pub plan_wraps: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DesktopWrapOptions {
    pub force: bool,
    pub dry_run: bool,
    pub print_only: bool,
    pub override_mode: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DesktopUnwrapOptions {
    pub override_mode: bool,
}

pub fn parse() -> Cli {
    Cli::parse()
}
