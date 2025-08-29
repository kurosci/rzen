use anyhow::{Result, anyhow};
use ssh2::Session;
use std::path::Path;

use crate::commands::build;
use crate::config::Config;
use crate::logging::log;
use crate::utils;

/// Deploy the project to a remote server
pub async fn deploy_project(
    config: &Config,
    skip_build: bool,
    _force: bool,
    dry_run: bool,
) -> Result<String> {
    deploy_project_with_progress(config, skip_build, _force, dry_run, None).await
}

/// Deploy the project to a remote server with progress callback
#[allow(clippy::type_complexity)]
pub async fn deploy_project_with_progress(
    config: &Config,
    skip_build: bool,
    _force: bool,
    dry_run: bool,
    progress_callback: Option<&(dyn Fn(f64, &str) + Send + Sync)>,
) -> Result<String> {
    let binary_name = config.binary_name();

    log::operation_start(&format!(
        "Deploying '{}' to {}",
        binary_name, config.deploy.vps_host
    ));

    if !dry_run {
        validate_deployment_prerequisites(config)?;
    }

    if dry_run {
        return simulate_deployment(config).await;
    }

    if !skip_build {
        build::build_project(config, None, dry_run).await?;
    } else {
        log::build_step("Skipping build as requested");
    }

    let project_path = config.project_path()?;
    let binary_path =
        utils::fs::find_binary(&project_path, &binary_name, &config.project.build_mode)?;
    if !binary_path.exists() {
        return Err(anyhow!(
            "Binary not found: {}. Run build first.",
            binary_path.display()
        ));
    }

    let (result, duration) = utils::timing::measure(|| async {
        execute_deployment(config, &binary_path, progress_callback).await
    })
    .await;

    match result {
        Ok(output) => {
            log::operation_success(&format!(
                "Deployment completed in {}",
                utils::timing::format_duration(duration)
            ));
            Ok(output)
        }
        Err(e) => {
            log::operation_failed("Deployment", &e.to_string());
            Err(e)
        }
    }
}

/// Execute the actual deployment process
#[allow(clippy::type_complexity)]
async fn execute_deployment(
    config: &Config,
    binary_path: &Path,
    progress_callback: Option<&(dyn Fn(f64, &str) + Send + Sync)>,
) -> Result<String> {
    let progress = utils::progress::deploy_progress(6);

    let message = "Connecting to server...";
    progress.set_message(message);
    if let Some(callback) = progress_callback {
        callback(16.67, message);
    }

    let ssh_config = utils::ssh::SshConfig {
        host: config.deploy.vps_host.clone(),
        port: config.deploy.ssh_port,
        username: config.deploy.vps_user.clone(),
        key_path: config.deploy.vps_key_path.clone(),
        password: config.deploy.vps_password.clone(),
    };

    let session = utils::ssh::connect_with_retry(&ssh_config, 3).await?;
    progress.inc(1);

    let message = "Creating remote directory...";
    progress.set_message(message);
    if let Some(callback) = progress_callback {
        callback(33.33, message);
    }
    utils::ssh::create_remote_directory(&session, &config.deploy.deploy_path)?;
    progress.inc(1);

    let message = "Uploading binary...";
    progress.set_message(message);
    if let Some(callback) = progress_callback {
        callback(50.0, message);
    }
    let remote_binary_path = format!("{}/{}", config.deploy.deploy_path, config.binary_name());
    let backup_binary_path = format!(
        "{}/{}.backup",
        config.deploy.deploy_path,
        config.binary_name()
    );

    // Create backup of existing binary if it exists
    let binary_exists = utils::ssh::remote_file_exists(&session, &remote_binary_path)?;
    if binary_exists {
        log::deploy_step("Creating backup of existing binary");
        utils::ssh::execute_command(
            &session,
            &format!("cp {} {}", remote_binary_path, backup_binary_path),
        )?;
    }

    utils::ssh::upload_file(&session, binary_path, &remote_binary_path)?;
    progress.inc(1);

    let message = "Setting executable permissions...";
    progress.set_message(message);
    if let Some(callback) = progress_callback {
        callback(66.67, message);
    }
    utils::ssh::execute_command(&session, &format!("chmod +x {}", remote_binary_path))?;
    progress.inc(1);

    let message = "Creating systemd service...";
    progress.set_message(message);
    if let Some(callback) = progress_callback {
        callback(83.33, message);
    }
    create_systemd_service(&session, config)?;
    progress.inc(1);

    let message = "Starting service...";
    progress.set_message(message);
    if let Some(callback) = progress_callback {
        callback(100.0, message);
    }
    start_service(&session, &config.service_name())?;
    progress.inc(1);

    progress.finish_with_message("Deployment completed successfully!");
    Ok(format!(
        "Successfully deployed {} to {}",
        config.binary_name(),
        config.deploy.vps_host
    ))
}

/// Create systemd service file
fn create_systemd_service(session: &Session, config: &Config) -> Result<()> {
    let service_name = config.service_name();
    let service_content = generate_systemd_service(config);

    let temp_service_path = format!("/tmp/{}", service_name);
    utils::ssh::execute_command(
        session,
        &format!(
            "cat > {} << 'EOF'\n{}\nEOF",
            temp_service_path, service_content
        ),
    )?;

    utils::ssh::execute_command(
        session,
        &format!("sudo mv {} /etc/systemd/system/", temp_service_path),
    )?;

    utils::ssh::execute_command(session, "sudo systemctl daemon-reload")?;

    log::deploy_step(&format!("Created systemd service: {}", service_name));
    Ok(())
}

/// Generate systemd service file content
fn generate_systemd_service(config: &Config) -> String {
    let binary_path = format!("{}/{}", config.deploy.deploy_path, config.binary_name());
    let working_directory = config.deploy.deploy_path.clone();

    format!(
        r#"[Unit]
Description={0} - Rust Application
After=network.target

[Service]
Type=simple
User={1}
WorkingDirectory={2}
ExecStart={3}
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal
SyslogIdentifier={0}

# Security settings
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ReadWritePaths={2}
ProtectHome=yes

[Install]
WantedBy=multi-user.target
"#,
        config.binary_name(),
        config.deploy.vps_user,
        working_directory,
        binary_path
    )
}

/// Start systemd service
fn start_service(session: &Session, service_name: &str) -> Result<()> {
    let _ = utils::ssh::execute_command(session, &format!("sudo systemctl stop {}", service_name));

    utils::ssh::execute_command(session, &format!("sudo systemctl enable {}", service_name))?;
    utils::ssh::execute_command(session, &format!("sudo systemctl start {}", service_name))?;

    let (output, _) = utils::ssh::execute_command(
        session,
        &format!("sudo systemctl is-active {}", service_name),
    )?;
    if output.trim() != "active" {
        return Err(anyhow!("Service {} failed to start", service_name));
    }

    log::deploy_step(&format!("Service {} started successfully", service_name));
    Ok(())
}

/// Simulate deployment for dry run
async fn simulate_deployment(config: &Config) -> Result<String> {
    log::dry_run("SSH connection to server");
    log::dry_run(&format!("Create directory: {}", config.deploy.deploy_path));
    log::dry_run(&format!("Upload binary: {}", config.binary_name()));
    log::dry_run("Set executable permissions");
    log::dry_run(&format!(
        "Create systemd service: {}",
        config.service_name()
    ));
    log::dry_run(&format!("Start systemd service: {}", config.service_name()));

    Ok(format!(
        "DRY RUN: Would deploy {} to {}",
        config.binary_name(),
        config.deploy.vps_host
    ))
}

/// Check deployment status
pub async fn check_deployment_status(config: &Config) -> Result<DeploymentStatus> {
    // Create SSH connection
    let ssh_config = utils::ssh::SshConfig {
        host: config.deploy.vps_host.clone(),
        port: config.deploy.ssh_port,
        username: config.deploy.vps_user.clone(),
        key_path: config.deploy.vps_key_path.clone(),
        password: config.deploy.vps_password.clone(),
    };

    let session = match utils::ssh::connect_with_retry(&ssh_config, 3).await {
        Ok(sess) => sess,
        Err(_) => {
            return Ok(DeploymentStatus {
                service_active: false,
                last_deployment: None,
                version: None,
            });
        }
    };

    let service_name = config.service_name();

    // Check service status
    let service_active = match utils::ssh::execute_command(
        &session,
        &format!("sudo systemctl is-active {}", service_name),
    ) {
        Ok((output, _)) => output.trim() == "active",
        Err(_) => false,
    };

    // Get service file modification time as last deployment time
    let service_file = format!("/etc/systemd/system/{}", service_name);
    let last_deployment =
        match utils::ssh::execute_command(&session, &format!("stat -c %Y {}", service_file)) {
            Ok((output, _)) => {
                if let Ok(timestamp) = output.trim().parse::<i64>() {
                    Some(
                        chrono::DateTime::from_timestamp(timestamp, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                            .unwrap_or_else(|| "Unknown".to_string()),
                    )
                } else {
                    None
                }
            }
            Err(_) => None,
        };

    // Get binary version/size info
    let deploy_path = &config.deploy.deploy_path;
    let binary_name = config.binary_name();
    let binary_path = format!("{}/{}", deploy_path, binary_name);

    let version = match utils::ssh::execute_command(&session, &format!("ls -lh {}", binary_path)) {
        Ok((output, _)) => {
            let parts: Vec<&str> = output.split_whitespace().collect();
            if parts.len() >= 5 {
                Some(format!("Size: {}, Modified: {}", parts[4], parts[5]))
            } else {
                Some("Version info unavailable".to_string())
            }
        }
        Err(_) => None,
    };

    Ok(DeploymentStatus {
        service_active,
        last_deployment,
        version,
    })
}

/// Deployment status information
#[derive(Debug, Clone)]
pub struct DeploymentStatus {
    pub service_active: bool,
    pub last_deployment: Option<String>,
    pub version: Option<String>,
}

/// Rollback deployment to previous version
pub async fn rollback_deployment(config: &Config) -> Result<()> {
    let service_name = config.service_name();

    log::operation_start("Rolling back deployment");

    // Create SSH connection
    let ssh_config = utils::ssh::SshConfig {
        host: config.deploy.vps_host.clone(),
        port: config.deploy.ssh_port,
        username: config.deploy.vps_user.clone(),
        key_path: config.deploy.vps_key_path.clone(),
        password: config.deploy.vps_password.clone(),
    };

    let session = utils::ssh::connect_with_retry(&ssh_config, 3).await?;

    // Stop current service
    log::deploy_step("Stopping current service");
    let _ = utils::ssh::execute_command(&session, &format!("sudo systemctl stop {}", service_name));

    // Check if backup exists
    let deploy_path = &config.deploy.deploy_path;
    let binary_name = config.binary_name();
    let current_binary = format!("{}/{}", deploy_path, binary_name);
    let backup_binary = format!("{}/{}.backup", deploy_path, binary_name);

    let backup_exists = utils::ssh::remote_file_exists(&session, &backup_binary)?;

    if !backup_exists {
        return Err(anyhow!(
            "No backup found for rollback. Backup file: {}",
            backup_binary
        ));
    }

    // Restore backup
    log::deploy_step("Restoring backup");
    utils::ssh::execute_command(
        &session,
        &format!("cp {} {}", backup_binary, current_binary),
    )?;
    utils::ssh::execute_command(&session, &format!("chmod +x {}", current_binary))?;

    // Restart service
    log::deploy_step("Restarting service");
    utils::ssh::execute_command(&session, &format!("sudo systemctl start {}", service_name))?;

    // Verify service is running
    let (output, _) = utils::ssh::execute_command(
        &session,
        &format!("sudo systemctl is-active {}", service_name),
    )?;

    if output.trim() != "active" {
        return Err(anyhow!("Service failed to start after rollback"));
    }

    log::operation_success("Rollback completed successfully");
    Ok(())
}

/// Validate deployment prerequisites
pub fn validate_deployment_prerequisites(config: &Config) -> Result<()> {
    let project_path = config.project_path()?;
    let binary_path = utils::fs::find_binary(
        &project_path,
        &config.binary_name(),
        &config.project.build_mode,
    )?;

    if !binary_path.exists() {
        return Err(anyhow!(
            "Binary not found: {}. Run build first.",
            binary_path.display()
        ));
    }

    let file_size = utils::fs::get_file_size(&binary_path)?;
    if file_size == 0 {
        return Err(anyhow!("Binary file is empty: {}", binary_path.display()));
    }

    if config.deploy.vps_key_path.is_none() && config.deploy.vps_password.is_none() {
        return Err(anyhow!(
            "SSH authentication not configured. Provide either key_path or password."
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_systemd_service_generation() {
        let config = Config {
            project: crate::config::ProjectConfig {
                path: ".".to_string(),
                name: "test-app".to_string(),
                build_mode: "release".to_string(),
            },
            deploy: crate::config::DeployConfig {
                target: "vps".to_string(),
                vps_host: "example.com".to_string(),
                vps_user: "deploy".to_string(),
                vps_key_path: Some("~/.ssh/id_rsa".to_string()),
                vps_password: None,
                deploy_path: "/opt/test-app".to_string(),
                service_name: Some("test-app.service".to_string()),
                ssh_port: 22,
            },
            monitor: crate::config::MonitorConfig {
                health_endpoint: Some("http://example.com/health".to_string()),
                log_path: Some("/var/log/test-app.log".to_string()),
                interval_secs: 10,
                health_timeout_secs: 5,
            },
        };

        let service_content = generate_systemd_service(&config);
        assert!(service_content.contains("Description=test-app - Rust Application"));
        assert!(service_content.contains("User=deploy"));
        assert!(service_content.contains("ExecStart=/opt/test-app/test-app"));
        assert!(service_content.contains("WorkingDirectory=/opt/test-app"));
    }

    #[test]
    fn test_deployment_status_creation() {
        let status = DeploymentStatus {
            service_active: true,
            last_deployment: Some("2024-01-01".to_string()),
            version: Some("1.0.0".to_string()),
        };

        assert!(status.service_active);
        assert_eq!(status.last_deployment.as_deref(), Some("2024-01-01"));
        assert_eq!(status.version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn test_validate_deployment_prerequisites_no_binary() {
        let temp_dir = tempdir().unwrap();
        let config = Config {
            project: crate::config::ProjectConfig {
                path: temp_dir.path().to_string_lossy().to_string(),
                name: "nonexistent".to_string(),
                build_mode: "debug".to_string(),
            },
            deploy: crate::config::DeployConfig {
                target: "vps".to_string(),
                vps_host: "example.com".to_string(),
                vps_user: "deploy".to_string(),
                vps_key_path: Some("~/.ssh/id_rsa".to_string()),
                vps_password: None,
                deploy_path: "/opt/app".to_string(),
                service_name: Some("app.service".to_string()),
                ssh_port: 22,
            },
            monitor: crate::config::MonitorConfig {
                health_endpoint: None,
                log_path: None,
                interval_secs: 10,
                health_timeout_secs: 5,
            },
        };

        let result = validate_deployment_prerequisites(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Binary not found"));
    }
}
