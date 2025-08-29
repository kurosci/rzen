use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// rzen - A TUI-based CLI tool for building, deploying, and monitoring Rust projects
#[derive(Parser, Debug)]
#[command(name = "rzen")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Build, deploy, and monitor Rust projects with ease")]
#[command(
    long_about = "rzen is a comprehensive tool that helps Rust developers build, deploy, and monitor their applications.
It provides both an interactive TUI interface and command-line functionality for managing the complete lifecycle of Rust projects."
)]
pub struct Cli {
    /// Path to the rzen.toml configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Log level (0=off, 1=error, 2=warn, 3=info, 4=debug, 5=trace)
    #[arg(long, default_value = "3")]
    pub log_level: u8,

    /// Dry run mode - simulate operations without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Build the Rust project
    Build {
        /// Build mode (overrides config)
        #[arg(long)]
        mode: Option<String>,

        /// Additional cargo arguments
        #[arg(last = true)]
        cargo_args: Vec<String>,
    },

    /// Deploy the project to a remote server
    Deploy {
        /// Skip building and use existing binary
        #[arg(long)]
        skip_build: bool,

        /// Force redeployment even if already deployed
        #[arg(long)]
        force: bool,
    },

    /// Monitor the deployed application
    Monitor {
        /// Continuous monitoring mode
        #[arg(long)]
        continuous: bool,

        /// Number of log lines to show initially
        #[arg(long, default_value = "50")]
        lines: usize,
    },

    /// Initialize a new rzen configuration file
    Init {
        /// Path where to create the configuration file
        #[arg(default_value = "rzen.toml")]
        path: PathBuf,

        /// Project name
        #[arg(long)]
        name: Option<String>,

        /// Target deployment host
        #[arg(long)]
        host: Option<String>,
    },

    /// Validate configuration file
    Validate {
        /// Path to configuration file to validate
        #[arg(default_value = "rzen.toml")]
        path: PathBuf,
    },

    /// Clean build artifacts
    Clean {
        /// Additional cargo clean arguments
        #[arg(last = true)]
        cargo_args: Vec<String>,
    },

    /// Rollback deployment to previous version
    Rollback,

    /// Stream logs in real-time
    Logs {
        /// Number of initial log lines to show
        #[arg(short, long, default_value = "50")]
        lines: usize,

        /// Follow logs in real-time
        #[arg(short, long)]
        follow: bool,
    },

    /// Check deployment status
    Status,

    /// Check if project needs rebuilding
    CheckRebuild,
}

impl Cli {
    /// Get the log level as a tracing filter string
    #[allow(dead_code)]
    pub fn log_filter(&self) -> String {
        crate::logging::LogLevel::from_number(self.log_level).as_filter().to_string()
    }

    /// Get the current log level as enum
    pub fn log_level(&self) -> crate::logging::LogLevel {
        crate::logging::LogLevel::from_number(self.log_level)
    }

    /// Check if we should run in TUI mode (no subcommand specified)
    pub fn should_run_tui(&self) -> bool {
        self.command.is_none()
    }

    /// Validate log level
    pub fn validate(&self) -> Result<(), String> {
        if self.log_level > 5 {
            return Err("Log level must be between 0 and 5".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_filter_mapping() {
        let cli = Cli {
            config: None,
            log_level: 0,
            dry_run: false,
            command: None,
        };
        assert_eq!(cli.log_filter(), "off");

        let cli = Cli {
            config: None,
            log_level: 3,
            dry_run: false,
            command: None,
        };
        assert_eq!(cli.log_filter(), "info");
    }

    #[test]
    fn test_tui_mode_detection() {
        let cli = Cli {
            config: None,
            log_level: 3,
            dry_run: false,
            command: None,
        };
        assert!(cli.should_run_tui());

        let cli = Cli {
            config: None,
            log_level: 3,
            dry_run: false,
            command: Some(Commands::Build {
                mode: None,
                cargo_args: vec![],
            }),
        };
        assert!(!cli.should_run_tui());
    }

    #[test]
    fn test_log_level_validation() {
        let cli = Cli {
            config: None,
            log_level: 3,
            dry_run: false,
            command: None,
        };
        assert!(cli.validate().is_ok());

        let cli = Cli {
            config: None,
            log_level: 10,
            dry_run: false,
            command: None,
        };
        assert!(cli.validate().is_err());
    }
}
