use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "waylog")]
#[command(about = "Automatically sync AI chat history from various CLI tools", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress all output (except errors)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Output format
    #[arg(long, default_value = "text", global = true)]
    pub output: OutputFormat,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run an AI CLI tool and automatically sync its chat history
    Run {
        /// The AI tool to run (antigravity, codex, claude, gemini, opencode)
        agent: Option<String>,

        /// Additional arguments to pass to the agent
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Pull chat history from providers
    Pull {
        /// Specific provider to pull (if not specified, pulls all)
        #[arg(short, long)]
        provider: Option<String>,

        /// Force re-pull even if up to date
        #[arg(short, long)]
        force: bool,

        /// Include descendant projects in one output
        #[arg(short, long)]
        recursive: bool,

        /// Include hidden descendants when using --recursive
        #[arg(long, visible_alias = "hiden", requires = "recursive")]
        hidden: bool,

        /// Pull only this provider session ID
        #[arg(
            long,
            requires = "provider",
            conflicts_with = "recursive",
            group = "target"
        )]
        session: Option<String>,

        /// Parse a supported provider session file or directory tree
        #[arg(
            long,
            value_name = "PATH",
            requires = "provider",
            conflicts_with = "recursive",
            group = "target"
        )]
        source: Option<std::path::PathBuf>,

        /// Write Markdown directly to this directory (default: .waylog/history)
        #[arg(long, value_name = "DIR")]
        output_dir: Option<std::path::PathBuf>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_directory_is_available_for_standard_and_recursive_pulls() {
        assert!(Cli::try_parse_from(["waylog", "pull", "--output-dir", "/tmp/out"]).is_ok());
        assert!(Cli::try_parse_from(
            ["waylog", "pull", "--recursive", "--output-dir", "/tmp/out",]
        )
        .is_ok());
    }

    #[test]
    fn one_session_accepts_a_custom_output_directory() {
        assert!(Cli::try_parse_from([
            "waylog",
            "pull",
            "--provider",
            "opencode",
            "--session",
            "ses_test",
            "--output-dir",
            "/tmp/out",
        ])
        .is_ok());
    }

    #[test]
    fn source_conflicts_with_session_lookup() {
        assert!(Cli::try_parse_from([
            "waylog",
            "pull",
            "--provider",
            "opencode",
            "--session",
            "ses_test",
            "--source",
            "/tmp/raw",
        ])
        .is_err());
    }

    #[test]
    fn hidden_directories_require_recursive_mode() {
        assert!(Cli::try_parse_from(["waylog", "pull", "--hidden"]).is_err());
        assert!(Cli::try_parse_from(["waylog", "pull", "--recursive", "--hidden"]).is_ok());
    }
}
