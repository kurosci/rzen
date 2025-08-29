use anyhow::Result;
use std::io;

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize the logging system with the specified filter
pub fn init(filter: &str) -> Result<()> {
    let filter = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .with_writer(io::stderr);

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();

    Ok(())
}

// /// Initialize logging for TUI mode (minimal output)
// pub fn init_tui() -> Result<()> {
//     init("warn")?;
//     tracing::info!("Starting rzen in TUI mode");
//     Ok(())
// }

// /// Initialize logging for CLI mode with specified level (deprecated)
// pub fn init_cli(log_level: &str) -> Result<()> {
//     init(log_level)?;
//     tracing::debug!("Logging initialized with level: {}", log_level);
//     Ok(())
// }

/// Initialize logging with LogLevel enum
pub fn init_with_level(level: LogLevel) -> Result<()> {
    let filter = level.as_filter();
    init(filter)?;
    tracing::debug!("Logging initialized with level: {}", filter);
    Ok(())
}

/// Log levels for CLI display
#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// Convert numeric level to LogLevel enum
    pub fn from_number(level: u8) -> Self {
        match level {
            0 => LogLevel::Error,
            1 => LogLevel::Error,
            2 => LogLevel::Warn,
            3 => LogLevel::Info,
            4 => LogLevel::Debug,
            5 => LogLevel::Trace,
            _ => LogLevel::Info,
        }
    }

    /// Get the string representation for filtering
    pub fn as_filter(&self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }

    /// Convert LogLevel to numeric representation
    #[allow(dead_code)]
    pub fn as_number(&self) -> u8 {
        match self {
            LogLevel::Error => 1,
            LogLevel::Warn => 2,
            LogLevel::Info => 3,
            LogLevel::Debug => 4,
            LogLevel::Trace => 5,
        }
    }
}

/// Utility functions for consistent logging
pub mod log {

    /// Log an operation start
    pub fn operation_start(operation: &str) {
        tracing::info!("🚀 Starting: {}", operation);
    }

    /// Log an operation success
    pub fn operation_success(operation: &str) {
        tracing::info!("✅ Completed: {}", operation);
    }

    /// Log an operation failure
    pub fn operation_failed(operation: &str, error: &str) {
        tracing::error!("❌ Failed: {} - {}", operation, error);
    }

    // /// Log progress with percentage
    // pub fn progress(operation: &str, current: usize, total: usize) {
    //     let percentage = if total > 0 {
    //         (current * 100) / total
    //     } else {
    //         100
    //     };
    //     tracing::info!("📊 {}: {}% ({} of {})", operation, percentage, current, total);
    // }

    /// Log build step
    pub fn build_step(step: &str) {
        tracing::info!("🔨 Build: {}", step);
    }

    /// Log deployment step
    pub fn deploy_step(step: &str) {
        tracing::info!("🚀 Deploy: {}", step);
    }

    /// Log monitoring event
    pub fn monitor_event(event: &str) {
        tracing::info!("👀 Monitor: {}", event);
    }

    /// Log SSH operation
    pub fn ssh_operation(operation: &str, host: &str) {
        tracing::debug!("🔐 SSH {} on {}", operation, host);
    }

    /// Log file transfer
    pub fn file_transfer(file: &str, direction: &str) {
        tracing::info!("📁 {}: {}", direction, file);
    }

    /// Log health check result
    pub fn health_check(endpoint: &str, status: bool, response_time_ms: Option<u128>) {
        if status {
            if let Some(ms) = response_time_ms {
                tracing::info!("💚 Health OK: {} ({}ms)", endpoint, ms);
            } else {
                tracing::info!("💚 Health OK: {}", endpoint);
            }
        } else {
            tracing::warn!("💔 Health FAIL: {}", endpoint);
        }
    }

    /// Log dry run message
    pub fn dry_run(operation: &str) {
        tracing::info!("🌵 DRY RUN: Would execute '{}'", operation);
    }

    /// Log configuration loading
    pub fn config_loaded(path: &str) {
        tracing::info!("📋 Configuration loaded from: {}", path);
    }

    /// Log configuration validation
    pub fn config_validated() {
        tracing::debug!("✅ Configuration validation passed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_conversion() {
        assert_eq!(LogLevel::from_number(1).as_filter(), "error");
        assert_eq!(LogLevel::from_number(3).as_filter(), "info");
        assert_eq!(LogLevel::from_number(5).as_filter(), "trace");
        assert_eq!(LogLevel::from_number(10).as_filter(), "info"); // default
    }

    #[test]
    fn test_log_level_numbers() {
        assert_eq!(LogLevel::Error.as_number(), 1);
        assert_eq!(LogLevel::Info.as_number(), 3);
        assert_eq!(LogLevel::Trace.as_number(), 5);
    }
}
