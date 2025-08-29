use anyhow::{Context, Result, anyhow};

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration for the rzen application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub project: ProjectConfig,
    pub deploy: DeployConfig,
    pub monitor: MonitorConfig,
}

/// Project-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Path to the Rust project (relative to config file or absolute)
    #[serde(default = "default_project_path")]
    pub path: String,

    /// Name of the project (used for binary name and service name)
    pub name: String,

    /// Build mode: "debug" or "release"
    #[serde(default = "default_build_mode")]
    pub build_mode: String,
}

/// Deployment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployConfig {
    /// Deployment target type
    #[serde(default = "default_target")]
    pub target: String,

    /// VPS host address
    pub vps_host: String,

    /// SSH username
    pub vps_user: String,

    /// Path to SSH private key (optional, falls back to password auth)
    pub vps_key_path: Option<String>,

    /// SSH password (optional, used if key_path not provided)
    pub vps_password: Option<String>,

    /// Remote directory for deployment
    #[serde(default = "default_deploy_path")]
    pub deploy_path: String,

    /// Systemd service name
    pub service_name: Option<String>,

    /// SSH port
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
}

/// Monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    /// Health check endpoint URL
    pub health_endpoint: Option<String>,

    /// Remote log file path
    pub log_path: Option<String>,

    /// Monitoring poll interval in seconds
    #[serde(default = "default_monitor_interval")]
    pub interval_secs: u64,

    /// Timeout for health checks in seconds
    #[serde(default = "default_health_timeout")]
    pub health_timeout_secs: u64,
}

// Default value functions
fn default_project_path() -> String {
    ".".to_string()
}

fn default_build_mode() -> String {
    "release".to_string()
}

fn default_target() -> String {
    "vps".to_string()
}

fn default_deploy_path() -> String {
    "/opt/rzen-app".to_string()
}

fn default_ssh_port() -> u16 {
    22
}

fn default_monitor_interval() -> u64 {
    10
}

fn default_health_timeout() -> u64 {
    5
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse TOML config file: {}", path.display()))?;

        config.validate()?;
        Ok(config)
    }

    /// Load configuration from the default location (rzen.toml in current directory)
    pub fn from_default_location() -> Result<Self> {
        let paths = [
            "rzen.toml",
            ".rzen.toml",
            &format!(
                "{}/.rzen.toml",
                dirs::home_dir()
                    .ok_or_else(|| anyhow!("Could not determine home directory"))?
                    .display()
            ),
        ];

        for path in &paths {
            if Path::new(path).exists() {
                return Self::from_file(path);
            }
        }

        Err(anyhow!(
            "No configuration file found. Create rzen.toml in the current directory or provide --config path"
        ))
    }

    /// Create a default configuration file
    pub fn create_default<P: AsRef<Path>>(path: P) -> Result<()> {
        let default_config = Config {
            project: ProjectConfig {
                path: ".".to_string(),
                name: "my-rust-app".to_string(),
                build_mode: "release".to_string(),
            },
            deploy: DeployConfig {
                target: "vps".to_string(),
                vps_host: "your-vps.example.com".to_string(),
                vps_user: "deploy".to_string(),
                vps_key_path: Some("~/.ssh/id_rsa".to_string()),
                vps_password: None,
                deploy_path: "/opt/rzen-app".to_string(),
                service_name: Some("my-rust-app.service".to_string()),
                ssh_port: 22,
            },
            monitor: MonitorConfig {
                health_endpoint: Some("http://your-vps.example.com:8080/health".to_string()),
                log_path: Some("/var/log/my-rust-app.log".to_string()),
                interval_secs: 10,
                health_timeout_secs: 5,
            },
        };

        let toml_string = toml::to_string_pretty(&default_config)
            .context("Failed to serialize default config to TOML")?;

        fs::write(path.as_ref(), toml_string).with_context(|| {
            format!(
                "Failed to write default config to: {}",
                path.as_ref().display()
            )
        })?;

        Ok(())
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate project config
        if self.project.name.trim().is_empty() {
            return Err(anyhow!("Project name cannot be empty"));
        }

        if !matches!(self.project.build_mode.as_str(), "debug" | "release") {
            return Err(anyhow!(
                "Build mode must be 'debug' or 'release', got: {}",
                self.project.build_mode
            ));
        }

        // Validate deploy config
        if self.deploy.vps_host.trim().is_empty() {
            return Err(anyhow!("VPS host cannot be empty"));
        }

        if self.deploy.vps_user.trim().is_empty() {
            return Err(anyhow!("VPS user cannot be empty"));
        }

        if self.deploy.vps_key_path.is_none() && self.deploy.vps_password.is_none() {
            return Err(anyhow!("Either SSH key path or password must be provided"));
        }

        if let Some(ref key_path) = self.deploy.vps_key_path {
            if key_path.trim().is_empty() {
                return Err(anyhow!("SSH key path cannot be empty"));
            }
        }

        // Validate monitor config
        if let Some(ref endpoint) = self.monitor.health_endpoint {
            if endpoint.trim().is_empty() {
                return Err(anyhow!("Health endpoint URL cannot be empty"));
            }
            if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
                return Err(anyhow!("Health endpoint must be a valid HTTP/HTTPS URL"));
            }
        }

        if self.monitor.interval_secs == 0 {
            return Err(anyhow!("Monitor interval must be greater than 0 seconds"));
        }

        if self.monitor.health_timeout_secs == 0 {
            return Err(anyhow!("Health timeout must be greater than 0 seconds"));
        }

        Ok(())
    }

    /// Get the absolute project path
    pub fn project_path(&self) -> Result<PathBuf> {
        let config_dir = Path::new(".")
            .canonicalize()
            .context("Failed to get current directory")?;
        let project_path = Path::new(&self.project.path);

        if project_path.is_absolute() {
            Ok(project_path.to_path_buf())
        } else {
            Ok(config_dir.join(project_path))
        }
    }

    /// Get the binary name based on project configuration
    pub fn binary_name(&self) -> String {
        self.project.name.clone()
    }

    /// Get the systemd service name
    pub fn service_name(&self) -> String {
        self.deploy
            .service_name
            .clone()
            .unwrap_or_else(|| format!("{}.service", self.project.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_config_validation() {
        let valid_config = Config {
            project: ProjectConfig {
                path: ".".to_string(),
                name: "test-app".to_string(),
                build_mode: "release".to_string(),
            },
            deploy: DeployConfig {
                target: "vps".to_string(),
                vps_host: "example.com".to_string(),
                vps_user: "deploy".to_string(),
                vps_key_path: Some("~/.ssh/id_rsa".to_string()),
                vps_password: None,
                deploy_path: "/opt/app".to_string(),
                service_name: Some("test-app.service".to_string()),
                ssh_port: 22,
            },
            monitor: MonitorConfig {
                health_endpoint: Some("http://example.com/health".to_string()),
                log_path: Some("/var/log/app.log".to_string()),
                interval_secs: 10,
                health_timeout_secs: 5,
            },
        };

        assert!(valid_config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_empty_name() {
        let invalid_config = Config {
            project: ProjectConfig {
                path: ".".to_string(),
                name: "".to_string(),
                build_mode: "release".to_string(),
            },
            deploy: DeployConfig {
                target: "vps".to_string(),
                vps_host: "example.com".to_string(),
                vps_user: "deploy".to_string(),
                vps_key_path: Some("~/.ssh/id_rsa".to_string()),
                vps_password: None,
                deploy_path: "/opt/app".to_string(),
                service_name: Some("test-app.service".to_string()),
                ssh_port: 22,
            },
            monitor: MonitorConfig {
                health_endpoint: Some("http://example.com/health".to_string()),
                log_path: Some("/var/log/app.log".to_string()),
                interval_secs: 10,
                health_timeout_secs: 5,
            },
        };

        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_create_default_config() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("rzen.toml");

        Config::create_default(&config_path).unwrap();

        // Should be able to load the created config
        let loaded_config = Config::from_file(&config_path).unwrap();
        assert_eq!(loaded_config.project.name, "my-rust-app");
        assert_eq!(loaded_config.deploy.vps_host, "your-vps.example.com");
    }
}
