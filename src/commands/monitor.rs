use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use ssh2::Session;
use std::io::Read;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::config::Config;
use crate::logging::log;
use crate::utils;

/// Monitor the deployed application
pub async fn monitor_application(
    config: &Config,
    continuous: bool,
    lines: usize,
) -> Result<String> {
    log::operation_start("Starting application monitoring");

    let mut monitor = ApplicationMonitor::new(config.clone());

    if continuous {
        monitor.run_continuous().await
    } else {
        monitor.run_once(lines).await
    }
}

/// Application monitor structure
pub struct ApplicationMonitor {
    config: Config,
    http_client: Client,
}

impl ApplicationMonitor {
    /// Create a new monitor instance
    pub fn new(config: Config) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(config.monitor.health_timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            config,
            http_client,
        }
    }

    /// Run continuous monitoring
    pub async fn run_continuous(&mut self) -> Result<String> {
        log::monitor_event("Starting continuous monitoring");

        let mut iteration = 0;
        loop {
            iteration += 1;
            log::monitor_event(&format!("Monitoring cycle #{}", iteration));

            let status = self.check_status().await?;
            self.display_status(&status);

            if iteration >= 10 {
                break;
            }

            sleep(Duration::from_secs(self.config.monitor.interval_secs)).await;
        }

        Ok("Continuous monitoring completed".to_string())
    }

    /// Run one-time monitoring check
    pub async fn run_once(&mut self, lines: usize) -> Result<String> {
        log::monitor_event("Running one-time monitoring check");

        let status = self.check_status().await?;
        self.display_status(&status);

        if let Some(log_path) = &self.config.monitor.log_path {
            self.display_logs(log_path, lines).await?;
        }

        Ok("Monitoring check completed".to_string())
    }

    /// Check application status
    pub async fn check_status(&self) -> Result<ApplicationStatus> {
        let mut status = ApplicationStatus::default();

        if let Some(endpoint) = &self.config.monitor.health_endpoint {
            let _health_start = Instant::now();
            match self.check_health_endpoint(endpoint).await {
                Ok(response_time) => {
                    status.health_ok = true;
                    status.response_time = Some(response_time);
                    log::health_check(endpoint, true, Some(response_time.as_millis()));
                }
                Err(e) => {
                    status.health_ok = false;
                    status.last_error = Some(e.to_string());
                    log::health_check(endpoint, false, None);
                }
            }
        }

        match self.check_ssh_connection().await {
            Ok(_) => {
                status.ssh_ok = true;
                status.service_status = self.check_service_status().await.ok();
            }
            Err(e) => {
                status.ssh_ok = false;
                status.last_error = Some(format!("SSH connection failed: {}", e));
            }
        }

        Ok(status)
    }

    /// Check health endpoint
    async fn check_health_endpoint(&self, endpoint: &str) -> Result<Duration> {
        let start = Instant::now();

        let response = self
            .http_client
            .get(endpoint)
            .send()
            .await
            .with_context(|| format!("Failed to connect to health endpoint: {}", endpoint))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Health endpoint returned status: {}",
                response.status()
            ));
        }

        let elapsed = start.elapsed();
        Ok(elapsed)
    }

    /// Check SSH connection
    async fn check_ssh_connection(&self) -> Result<Session> {
        let ssh_config = utils::ssh::SshConfig {
            host: self.config.deploy.vps_host.clone(),
            port: self.config.deploy.ssh_port,
            username: self.config.deploy.vps_user.clone(),
            key_path: self.config.deploy.vps_key_path.clone(),
            password: self.config.deploy.vps_password.clone(),
        };

        utils::ssh::connect_with_retry(&ssh_config, 2).await
    }

    /// Check systemd service status
    async fn check_service_status(&self) -> Result<String> {
        let session = self.check_ssh_connection().await?;
        let service_name = self.config.service_name();

        let (output, _) = utils::ssh::execute_command(
            &session,
            &format!("sudo systemctl is-active {}", service_name),
        )?;

        Ok(output.trim().to_string())
    }

    /// Display logs from remote server
    async fn display_logs(&self, log_path: &str, lines: usize) -> Result<()> {
        let session = self.check_ssh_connection().await?;

        let (output, _) =
            utils::ssh::execute_command(&session, &format!("tail -n {} {}", lines, log_path))?;

        if output.trim().is_empty() {
            log::monitor_event("No log entries found");
        } else {
            log::monitor_event(&format!("Recent logs (last {} lines):", lines));
            for line in output.lines() {
                log::monitor_event(&format!("  {}", line));
            }
        }

        Ok(())
    }

    /// Display current status
    fn display_status(&self, status: &ApplicationStatus) {
        log::monitor_event(&format!(
            "Health Status: {}",
            if status.health_ok {
                "✅ OK"
            } else {
                "❌ FAIL"
            }
        ));
        log::monitor_event(&format!(
            "SSH Connection: {}",
            if status.ssh_ok { "✅ OK" } else { "❌ FAIL" }
        ));

        if let Some(response_time) = status.response_time {
            log::monitor_event(&format!("Response Time: {}ms", response_time.as_millis()));
        }

        if let Some(service_status) = &status.service_status {
            log::monitor_event(&format!("Service Status: {}", service_status));
        }

        if let Some(error) = &status.last_error {
            log::monitor_event(&format!("Last Error: {}", error));
        }
    }
}

/// Application status information
#[derive(Debug, Default, Clone)]
pub struct ApplicationStatus {
    pub health_ok: bool,
    pub ssh_ok: bool,
    pub response_time: Option<Duration>,
    pub service_status: Option<String>,
    pub last_error: Option<String>,
}

impl ApplicationStatus {
    /// Check if application is healthy
    pub fn is_healthy(&self) -> bool {
        self.health_ok && self.ssh_ok && matches!(self.service_status.as_deref(), Some("active"))
    }

    /// Get status summary
    pub fn summary(&self) -> String {
        if self.is_healthy() {
            "All systems operational".to_string()
        } else {
            let mut issues = Vec::new();

            if !self.health_ok {
                issues.push("Health check failing");
            }
            if !self.ssh_ok {
                issues.push("SSH connection failed");
            }
            if !matches!(self.service_status.as_deref(), Some("active")) {
                issues.push("Service not active");
            }

            if issues.is_empty() {
                "Status unknown".to_string()
            } else {
                format!("Issues: {}", issues.join(", "))
            }
        }
    }
}

/// Monitor configuration for TUI display
#[allow(dead_code)]
pub struct MonitorConfig {
    pub interval: Duration,
    pub health_endpoint: Option<String>,
    pub log_path: Option<String>,
    pub max_log_lines: usize,
}

impl From<&Config> for MonitorConfig {
    fn from(config: &Config) -> Self {
        Self {
            interval: Duration::from_secs(config.monitor.interval_secs),
            health_endpoint: config.monitor.health_endpoint.clone(),
            log_path: config.monitor.log_path.clone(),
            max_log_lines: 100, // Default for TUI
        }
    }
}

/// Stream logs in real-time
pub async fn stream_logs(config: &Config) -> Result<()> {
    log::operation_start("Streaming logs in real-time");

    // Create SSH connection
    let ssh_config = crate::utils::ssh::SshConfig {
        host: config.deploy.vps_host.clone(),
        port: config.deploy.ssh_port,
        username: config.deploy.vps_user.clone(),
        key_path: config.deploy.vps_key_path.clone(),
        password: config.deploy.vps_password.clone(),
    };

    let session = crate::utils::ssh::connect_with_retry(&ssh_config, 3).await?;

    // Get log path from config or use default
    let log_path = config
        .monitor
        .log_path
        .as_deref()
        .unwrap_or("/var/log/my-rust-app.log");

    log::monitor_event(&format!("Tailing logs from: {}", log_path));

    // Use tail -f to stream logs
    let command = format!("tail -f -n 50 {}", log_path);

    match session.channel_session() {
        Ok(mut channel) => {
            channel.exec(&command)?;

            let mut buf = [0; 1024];
            loop {
                match channel.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let log_line = String::from_utf8_lossy(&buf[..n]);
                        for line in log_line.lines() {
                            if !line.trim().is_empty() {
                                log::monitor_event(&format!("📜 {}", line));
                            }
                        }
                    }
                    Err(_) => break,
                }

                // Small delay to prevent busy waiting
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
        Err(e) => {
            return Err(anyhow!("Failed to create SSH channel: {}", e));
        }
    }

    log::operation_success("Log streaming ended");
    Ok(())
}

/// Get monitoring metrics
pub async fn get_metrics(config: &Config) -> Result<MonitoringMetrics> {
    let monitor = ApplicationMonitor::new(config.clone());
    let status = monitor.check_status().await?;

    Ok(MonitoringMetrics {
        uptime_percentage: if status.is_healthy() { 100.0 } else { 0.0 }, // Simplified
        average_response_time: status.response_time.map(|d| d.as_millis() as f64),
        total_requests: None, // Would need more sophisticated monitoring
        error_count: if status.last_error.is_some() { 1 } else { 0 },
        last_check: chrono::Utc::now(),
    })
}

/// Monitoring metrics structure
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MonitoringMetrics {
    pub uptime_percentage: f64,
    pub average_response_time: Option<f64>,
    pub total_requests: Option<u64>,
    pub error_count: u64,
    pub last_check: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_application_status_summary() {
        let healthy_status = ApplicationStatus {
            health_ok: true,
            ssh_ok: true,
            response_time: Some(Duration::from_millis(50)),
            service_status: Some("active".to_string()),
            last_error: None,
        };

        assert!(healthy_status.is_healthy());
        assert_eq!(healthy_status.summary(), "All systems operational");

        let unhealthy_status = ApplicationStatus {
            health_ok: false,
            ssh_ok: true,
            response_time: None,
            service_status: Some("failed".to_string()),
            last_error: Some("Health check failed".to_string()),
        };

        assert!(!unhealthy_status.is_healthy());
        assert!(unhealthy_status.summary().contains("Issues"));
    }

    #[test]
    fn test_monitor_config_from_config() {
        let config = Config {
            project: crate::config::ProjectConfig {
                path: ".".to_string(),
                name: "test".to_string(),
                build_mode: "release".to_string(),
            },
            deploy: crate::config::DeployConfig {
                target: "vps".to_string(),
                vps_host: "example.com".to_string(),
                vps_user: "deploy".to_string(),
                vps_key_path: None,
                vps_password: None,
                deploy_path: "/opt/app".to_string(),
                service_name: None,
                ssh_port: 22,
            },
            monitor: crate::config::MonitorConfig {
                health_endpoint: Some("http://example.com/health".to_string()),
                log_path: Some("/var/log/app.log".to_string()),
                interval_secs: 30,
                health_timeout_secs: 10,
            },
        };

        let monitor_config = MonitorConfig::from(&config);
        assert_eq!(monitor_config.interval, Duration::from_secs(30));
        assert_eq!(
            monitor_config.health_endpoint.as_deref(),
            Some("http://example.com/health")
        );
        assert_eq!(monitor_config.log_path.as_deref(), Some("/var/log/app.log"));
    }

    #[test]
    fn test_monitoring_metrics_creation() {
        let metrics = MonitoringMetrics {
            uptime_percentage: 99.9,
            average_response_time: Some(45.5),
            total_requests: Some(1000),
            error_count: 2,
            last_check: chrono::Utc::now(),
        };

        assert_eq!(metrics.uptime_percentage, 99.9);
        assert_eq!(metrics.average_response_time, Some(45.5));
        assert_eq!(metrics.total_requests, Some(1000));
        assert_eq!(metrics.error_count, 2);
    }
}
