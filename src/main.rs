use anyhow::{Context, Result};
use std::process;

mod cli;
mod commands;
mod config;
mod logging;
mod tui;
mod utils;

use clap::Parser;
use cli::{Cli, Commands};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = cli.validate() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }

    if let Err(e) = init_logging(&cli) {
        eprintln!("Failed to initialize logging: {}", e);
        process::exit(1);
    }

    if let Err(e) = run(cli).await {
        logging::log::operation_failed("Application", &e.to_string());
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

/// Initialize logging based on CLI configuration
fn init_logging(cli: &Cli) -> Result<()> {
    let log_level = cli.log_level();
    logging::init_with_level(log_level).context("Failed to initialize logging system")
}

/// Main application logic
async fn run(cli: Cli) -> Result<()> {
    let config = load_configuration(&cli)?;

    if cli.should_run_tui() {
        logging::log::operation_start("Starting TUI interface");
        tui::run_tui(config).await?;
    } else if let Some(ref command) = cli.command {
        handle_command(command.clone(), config, &cli).await?;
    }

    Ok(())
}

/// Load configuration from file or create default
fn load_configuration(cli: &Cli) -> Result<config::Config> {
    let config_path = cli.config.as_ref();

    match config_path {
        Some(path) => {
            logging::log::config_loaded(&path.display().to_string());
            config::Config::from_file(path)
        }
        None => config::Config::from_default_location().or_else(|_| {
            println!(
                "No configuration file found. Would you like to create a default rzen.toml? (y/N): "
            );
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim().to_lowercase() == "y" {
                config::Config::create_default("rzen.toml")?;
                println!("Created default configuration file: rzen.toml");
                println!("Please edit it with your project settings before running rzen again.");
                process::exit(0);
            } else {
                Err(anyhow::anyhow!("Configuration required"))
            }
        }),
    }
}

/// Handle CLI commands
async fn handle_command(command: Commands, config: config::Config, cli: &Cli) -> Result<()> {
    match command {
        Commands::Build {
            mode,
            cargo_args: _,
        } => {
            let build_mode = mode.as_deref();
            commands::build::build_project(&config, build_mode, cli.dry_run).await?;
        }
        Commands::Deploy { skip_build, force } => {
            commands::deploy::deploy_project(&config, skip_build, force, cli.dry_run).await?;
        }
        Commands::Monitor { continuous, lines } => {
            if continuous {
                commands::monitor::monitor_application(&config, continuous, lines).await?;
            } else {
                commands::monitor::monitor_application(&config, false, lines).await?;
            }
        }
        Commands::Init { path, name, host } => {
            init_configuration(path, name, host)?;
        }
        Commands::Validate { path } => {
            validate_configuration(path)?;
        }
        Commands::Clean { cargo_args: _ } => {
            commands::build::clean_project(&config, cli.dry_run).await?;
        }
        Commands::Rollback => {
            commands::deploy::rollback_deployment(&config).await?;
        }
        Commands::Logs { lines, follow } => {
            if follow {
                commands::monitor::stream_logs(&config).await?;
            } else {
                // Show last N lines without following
                let ssh_config = utils::ssh::SshConfig {
                    host: config.deploy.vps_host.clone(),
                    port: config.deploy.ssh_port,
                    username: config.deploy.vps_user.clone(),
                    key_path: config.deploy.vps_key_path.clone(),
                    password: config.deploy.vps_password.clone(),
                };

                let session = utils::ssh::connect_with_retry(&ssh_config, 3).await?;
                let log_path = config.monitor.log_path.as_deref()
                    .unwrap_or("/var/log/my-rust-app.log");

                let (output, _) = utils::ssh::execute_command(
                    &session,
                    &format!("tail -n {} {}", lines, log_path)
                )?;

                for line in output.lines() {
                    if !line.trim().is_empty() {
                        println!("📜 {}", line);
                    }
                }
            }
        }
        Commands::Status => {
            let status = commands::deploy::check_deployment_status(&config).await?;
            println!("🚀 Deployment Status:");
            println!("  Service Active: {}", if status.service_active { "✅ Yes" } else { "❌ No" });
            if let Some(deployment) = &status.last_deployment {
                println!("  Last Deployment: {}", deployment);
            }
            if let Some(version) = &status.version {
                println!("  Version Info: {}", version);
            }
        }
        Commands::CheckRebuild => {
            let needs_rebuild = commands::build::needs_rebuild(&config)?;
            if needs_rebuild {
                println!("🔄 Project needs rebuilding");
            } else {
                println!("✅ Project is up to date");
            }
        }
    }

    Ok(())
}

/// Initialize a new configuration file
fn init_configuration(
    path: std::path::PathBuf,
    name: Option<String>,
    host: Option<String>,
) -> Result<()> {
    logging::log::operation_start(&format!("Creating configuration file: {}", path.display()));

    if name.is_some() || host.is_some() {
        let mut config = config::Config {
            project: config::ProjectConfig {
                path: ".".to_string(),
                name: name.unwrap_or_else(|| "my-rust-app".to_string()),
                build_mode: "release".to_string(),
            },
            deploy: config::DeployConfig {
                target: "vps".to_string(),
                vps_host: host.unwrap_or_else(|| "your-vps.example.com".to_string()),
                vps_user: "deploy".to_string(),
                vps_key_path: Some("~/.ssh/id_rsa".to_string()),
                vps_password: None,
                deploy_path: "/opt/my-rust-app".to_string(),
                service_name: None,
                ssh_port: 22,
            },
            monitor: config::MonitorConfig {
                health_endpoint: Some("http://your-vps.example.com:8080/health".to_string()),
                log_path: Some("/var/log/my-rust-app.log".to_string()),
                interval_secs: 10,
                health_timeout_secs: 5,
            },
        };

        config.deploy.service_name = Some(format!("{}.service", config.project.name));

        let toml_string =
            toml::to_string_pretty(&config).context("Failed to serialize configuration to TOML")?;

        std::fs::write(&path, toml_string)
            .with_context(|| format!("Failed to write configuration to: {}", path.display()))?;
    } else {
        config::Config::create_default(&path)?;
    }

    logging::log::operation_success(&format!("Configuration created: {}", path.display()));
    println!("Configuration file created: {}", path.display());
    println!("Edit this file with your project settings before deploying.");

    Ok(())
}

/// Validate a configuration file
fn validate_configuration(path: std::path::PathBuf) -> Result<()> {
    logging::log::operation_start(&format!("Validating configuration: {}", path.display()));

    let config = config::Config::from_file(&path)?;
    config.validate()?;

    logging::log::config_validated();
    logging::log::operation_success("Configuration validation passed");
    println!("✅ Configuration file is valid: {}", path.display());

    println!("Project: {}", config.project.name);
    println!("Build Mode: {}", config.project.build_mode);
    println!(
        "Deploy Target: {} @ {}",
        config.deploy.vps_user, config.deploy.vps_host
    );
    if let Some(endpoint) = &config.monitor.health_endpoint {
        println!("Health Endpoint: {}", endpoint);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_cli_parsing() {
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
    fn test_log_level_filter() {
        let cli = Cli {
            config: None,
            log_level: 1,
            dry_run: false,
            command: None,
        };
        assert_eq!(cli.log_filter(), "error");

        let cli = Cli {
            config: None,
            log_level: 3,
            dry_run: false,
            command: None,
        };
        assert_eq!(cli.log_filter(), "info");
    }

    #[test]
    fn test_config_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("test.toml");

        let result = init_configuration(
            config_path.clone(),
            Some("test-app".to_string()),
            Some("test.com".to_string()),
        );
        assert!(result.is_ok());

        let config = config::Config::from_file(&config_path);
        assert!(config.is_ok());
        assert_eq!(config.unwrap().project.name, "test-app");
    }
}
