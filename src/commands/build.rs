use anyhow::{Context, Result, anyhow};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command as TokioCommand;

use crate::config::Config;
use crate::logging::log;
use crate::utils;

/// Build the Rust project using Cargo
pub async fn build_project(
    config: &Config,
    build_mode: Option<&str>,
    dry_run: bool,
) -> Result<String> {
    let project_path = config.project_path()?;
    let build_mode = build_mode.unwrap_or(&config.project.build_mode);
    let binary_name = config.binary_name();

    log::operation_start(&format!(
        "Building project '{}' in {} mode",
        binary_name, build_mode
    ));

    if dry_run {
        log::dry_run(&format!(
            "cargo build --{} --bin {}",
            build_mode, binary_name
        ));
        return Ok(format!(
            "Would build {} in {} mode",
            binary_name, build_mode
        ));
    }

    if !needs_rebuild(config)? {
        log::build_step("Project is up to date, skipping build");
        return Ok(format!("Project '{}' is already built", binary_name));
    }

    let cargo_toml = project_path.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(anyhow!(
            "Cargo.toml not found in project directory: {}",
            project_path.display()
        ));
    }

    let (result, duration) = utils::timing::measure(|| async {
        execute_cargo_build(&project_path, build_mode, &binary_name).await
    })
    .await;

    match result {
        Ok(output) => {
            log::operation_success(&format!(
                "Build completed in {}",
                utils::timing::format_duration(duration)
            ));
            log::build_step("Binary ready for deployment");
            Ok(output)
        }
        Err(e) => {
            log::operation_failed("Build", &e.to_string());
            Err(e)
        }
    }
}

/// Execute cargo build command
async fn execute_cargo_build(
    project_path: &Path,
    build_mode: &str,
    binary_name: &str,
) -> Result<String> {
    let mut args = vec!["build", "--bin", binary_name];

    match build_mode {
        "release" => args.push("--release"),
        "debug" => {}
        _ => {
            return Err(anyhow!(
                "Invalid build mode: {}. Use 'debug' or 'release'",
                build_mode
            ));
        }
    }

    log::build_step(&format!("Running: cargo {}", args.join(" ")));

    let output = TokioCommand::new("cargo")
        .args(&args)
        .current_dir(project_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| "Failed to execute cargo build".to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    for line in stdout.lines() {
        if !line.trim().is_empty() {
            log::build_step(line);
        }
    }

    if !output.status.success() {
        return Err(anyhow!("Cargo build failed:\n{}", stderr));
    }

    let binary_path = utils::fs::find_binary(project_path, binary_name, build_mode)
        .with_context(|| format!("Binary '{}' not found after build", binary_name))?;

    let file_size = utils::fs::get_file_size(&binary_path)?;
    log::build_step(&format!(
        "Binary created: {} ({} bytes)",
        binary_path.display(),
        file_size
    ));

    Ok(format!(
        "Successfully built {} in {} mode",
        binary_name, build_mode
    ))
}

/// Check if project needs rebuilding
pub fn needs_rebuild(config: &Config) -> Result<bool> {
    let project_path = config.project_path()?;

    let target_dir = project_path.join("target").join(&config.project.build_mode);
    if !target_dir.exists() {
        return Ok(true);
    }

    let binary_path = utils::fs::find_binary(
        &project_path,
        &config.binary_name(),
        &config.project.build_mode,
    );
    match binary_path {
        Ok(path) => {
            let binary_modified = path.metadata()?.modified()?;
            let src_modified = get_latest_src_modification(&project_path)?;

            Ok(binary_modified < src_modified)
        }
        Err(_) => Ok(true),
    }
}

/// Get the latest modification time of source files
fn get_latest_src_modification(project_path: &Path) -> Result<std::time::SystemTime> {
    let src_dir = project_path.join("src");
    if !src_dir.exists() {
        return Err(anyhow!("src directory not found"));
    }

    let mut latest_time = std::time::SystemTime::UNIX_EPOCH;

    fn visit_dir(dir: &Path, latest: &mut std::time::SystemTime) -> Result<()> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    visit_dir(&path, latest)?;
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let modified = path.metadata()?.modified()?;
                    if modified > *latest {
                        *latest = modified;
                    }
                }
            }
        }
        Ok(())
    }

    visit_dir(&src_dir, &mut latest_time)?;
    Ok(latest_time)
}

/// Clean build artifacts
pub async fn clean_project(config: &Config, dry_run: bool) -> Result<()> {
    let project_path = config.project_path()?;

    log::operation_start("Cleaning build artifacts");

    if dry_run {
        log::dry_run("cargo clean");
        return Ok(());
    }

    let output = TokioCommand::new("cargo")
        .arg("clean")
        .current_dir(&project_path)
        .output()
        .await
        .with_context(|| "Failed to execute cargo clean")?;

    if output.status.success() {
        log::operation_success("Clean completed");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("Cargo clean failed: {}", stderr))
    }
}

/// Get build information
pub fn get_build_info(config: &Config) -> Result<BuildInfo> {
    let project_path = config.project_path()?;
    let binary_path = utils::fs::find_binary(
        &project_path,
        &config.binary_name(),
        &config.project.build_mode,
    );

    let binary_exists = binary_path.is_ok();
    let file_size = if let Ok(ref path) = binary_path {
        Some(utils::fs::get_file_size(path)?)
    } else {
        None
    };

    Ok(BuildInfo {
        binary_exists,
        file_size,
        build_mode: config.project.build_mode.clone(),
        project_name: config.binary_name(),
    })
}

/// Build information structure
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BuildInfo {
    pub binary_exists: bool,
    pub file_size: Option<u64>,
    pub build_mode: String,
    pub project_name: String,
}

impl BuildInfo {
    /// Format file size for display
    pub fn format_size(&self) -> String {
        match self.file_size {
            Some(size) => {
                const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
                let mut size = size as f64;
                let mut unit_index = 0;

                while size >= 1024.0 && unit_index < UNITS.len() - 1 {
                    size /= 1024.0;
                    unit_index += 1;
                }

                format!("{:.1} {}", size, UNITS[unit_index])
            }
            None => "N/A".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_build_nonexistent_project() {
        let temp_dir = tempdir().unwrap();
        let config = Config {
            project: crate::config::ProjectConfig {
                path: temp_dir.path().to_string_lossy().to_string(),
                name: "test".to_string(),
                build_mode: "debug".to_string(),
            },
            deploy: crate::config::DeployConfig {
                target: "vps".to_string(),
                vps_host: "localhost".to_string(),
                vps_user: "test".to_string(),
                vps_key_path: None,
                vps_password: None,
                deploy_path: "/tmp".to_string(),
                service_name: Some("test.service".to_string()),
                ssh_port: 22,
            },
            monitor: crate::config::MonitorConfig {
                health_endpoint: None,
                log_path: None,
                interval_secs: 10,
                health_timeout_secs: 5,
            },
        };

        let result = build_project(&config, None, false).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Cargo.toml not found")
        );
    }

    #[test]
    fn test_build_info_formatting() {
        let info = BuildInfo {
            binary_exists: true,
            file_size: Some(1024 * 1024), // 1MB
            build_mode: "release".to_string(),
            project_name: "test".to_string(),
        };

        assert_eq!(info.format_size(), "1.0 MB");

        let info_small = BuildInfo {
            binary_exists: true,
            file_size: Some(512),
            build_mode: "debug".to_string(),
            project_name: "test".to_string(),
        };

        assert_eq!(info_small.format_size(), "512.0 B");
    }
}
