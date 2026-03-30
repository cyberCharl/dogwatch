use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "dogwatch",
    version,
    about = "Monitor open GitHub PRs and Drydock runs",
    arg_required_else_help = false
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        help = "Preview write actions without executing them"
    )]
    pub dry_run: bool,
    #[arg(long, global = true, short, help = "Enable verbose command logging")]
    pub verbose: bool,
    #[arg(long, global = true, help = "Run a single cycle and exit")]
    pub once: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Check,
    Status,
    Logs(LogsArgs),
    Pause,
    Unpause,
}

#[derive(Debug, Clone, Args)]
pub struct LogsArgs {
    #[arg(long, default_value_t = 50, help = "Show the last N log rows")]
    pub last: usize,
    #[arg(long, help = "Filter by Drydock run id")]
    pub run_id: Option<i64>,
    #[arg(long, value_enum, help = "Filter by log level")]
    pub level: Option<LogLevelArg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogLevelArg {
    Info,
    Warn,
    Error,
}
