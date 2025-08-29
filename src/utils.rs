use anyhow::{anyhow, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use ssh2::Session;
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// SSH connection utilities
pub mod ssh {
    use super::*;

    /// SSH connection configuration
    #[derive(Debug, Clone)]
    pub struct SshConfig {
        pub host: String,
        pub port: u16,
        pub username: String,
        pub key_path: Option<String>,
        pub password: Option<String>,
    }

    /// Establish SSH connection with retry logic
    pub async fn connect_with_retry(config: &SshConfig, max_retries: u32) -> Result<Session> {
        let mut last_error = None;

        for attempt in 1..=max_retries {
            match connect_ssh(config) {
                Ok(session) => {
                    crate::logging::log::ssh_operation("connected", &config.host);
                    return Ok(session);
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < max_retries {
                        let delay = Duration::from_secs(2_u64.pow(attempt - 1)); // exponential backoff
                        crate::logging::log::ssh_operation(
                            &format!("connection failed (attempt {}/{}), retrying in {:?}", attempt, max_retries, delay),
                            &config.host
                        );
                        sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("SSH connection failed after {} attempts", max_retries)))
    }

    /// Establish SSH connection
    fn connect_ssh(config: &SshConfig) -> Result<Session> {
        let tcp = TcpStream::connect(format!("{}:{}", config.host, config.port))
            .with_context(|| format!("Failed to connect to {}:{}", config.host, config.port))?;

        let mut sess = Session::new().context("Failed to create SSH session")?;
        sess.set_tcp_stream(tcp);
        sess.handshake().context("SSH handshake failed")?;

        // Try key-based authentication first, then password
        let authenticated = if let Some(key_path) = &config.key_path {
            let key_path = shellexpand::tilde(key_path).to_string();
            if Path::new(&key_path).exists() {
                sess.userauth_pubkey_file(&config.username, None, Path::new(&key_path), None).is_ok()
            } else {
                false
            }
        } else {
            false
        };

        // If key auth failed, try password auth
        let authenticated = authenticated || if let Some(password) = &config.password {
            sess.userauth_password(&config.username, password).is_ok()
        } else {
            false
        };

        if !authenticated {
            return Err(anyhow!("SSH authentication failed for user {}", config.username));
        }

        Ok(sess)
    }

    /// Execute a command on the remote server
    pub fn execute_command(session: &Session, command: &str) -> Result<(String, String)> {
        let mut channel = session.channel_session()
            .with_context(|| format!("Failed to open channel for command: {}", command))?;

        channel.exec(command)
            .with_context(|| format!("Failed to execute command: {}", command))?;

        let mut stdout = String::new();
        let mut stderr = String::new();

        channel.read_to_string(&mut stdout)?;
        channel.stderr().read_to_string(&mut stderr)?;

        let exit_status = channel.exit_status()?;
        channel.wait_close()?;

        if exit_status != 0 {
            return Err(anyhow!("Command failed with exit code {}: {}\nstderr: {}",
                             exit_status, command, stderr));
        }

        Ok((stdout, stderr))
    }

    /// Upload a file via SCP
    pub fn upload_file(session: &Session, local_path: &Path, remote_path: &str) -> Result<()> {
        let mut file = File::open(local_path)
            .with_context(|| format!("Failed to open local file: {}", local_path.display()))?;

        let mut channel = session.scp_send(local_path, 0o644, file.metadata()?.len(), None)
            .with_context(|| format!("Failed to initiate SCP upload to: {}", remote_path))?;

        let mut buffer = [0; 8192];
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            channel.write_all(&buffer[..bytes_read])?;
        }

        channel.send_eof()?;
        channel.wait_eof()?;
        channel.close()?;
        channel.wait_close()?;

        crate::logging::log::file_transfer(remote_path, "uploaded");
        Ok(())
    }

    /// Create remote directory
    pub fn create_remote_directory(session: &Session, path: &str) -> Result<()> {
        execute_command(session, &format!("mkdir -p {}", path))?;
        crate::logging::log::ssh_operation(&format!("created directory {}", path), "");
        Ok(())
    }

    /// Check if remote file exists
    pub fn remote_file_exists(session: &Session, path: &str) -> Result<bool> {
        match execute_command(session, &format!("[ -f {} ] && echo 'exists' || echo 'not exists'", path)) {
            Ok((output, _)) => Ok(output.trim() == "exists"),
            Err(_) => Ok(false),
        }
    }
}

/// Progress bar utilities
pub mod progress {
    use super::*;

    // /// Create a progress bar for build operations
    // pub fn build_progress() -> ProgressBar {
    //     let pb = ProgressBar::new_spinner();
    //     pb.set_style(
    //         ProgressStyle::default_spinner()
    //             .template("{spinner:.green} {msg}")
    //             .unwrap()
    //     );
    //     pb
    // }

    /// Create a progress bar for deployment operations
    pub fn deploy_progress(total_steps: u64) -> ProgressBar {
        let pb = ProgressBar::new(total_steps);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("#>-")
        );
        pb
    }

    // /// Create a progress bar for file transfers
    // pub fn transfer_progress(file_size: u64) -> ProgressBar {
    //     let pb = ProgressBar::new(file_size);
    //     pb.set_style(
    //         ProgressStyle::default_bar()
    //             .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} {msg}")
    //             .unwrap()
    //             .progress_chars("#>-")
    //     );
    //     pb
    // }

    // /// Create a progress bar for monitoring
    // pub fn monitor_progress() -> ProgressBar {
    //     let pb = ProgressBar::new_spinner();
    //     pb.set_style(
    //         ProgressStyle::default_spinner()
    //             .template("{spinner:.blue} {msg}")
    //             .unwrap()
    //     );
    //     pb
    // }
}

/// File system utilities
pub mod fs {
    use super::*;

    /// Find the binary in the target directory
    pub fn find_binary(project_path: &Path, project_name: &str, build_mode: &str) -> Result<std::path::PathBuf> {
        let target_path = project_path.join("target").join(build_mode).join(project_name);

        if target_path.exists() {
            Ok(target_path)
        } else {
            // Try with .exe extension on Windows
            let target_path_exe = target_path.with_extension("exe");
            if target_path_exe.exists() {
                Ok(target_path_exe)
            } else {
                Err(anyhow!("Binary not found at: {}", target_path.display()))
            }
        }
    }

    // /// Ensure directory exists
    // pub fn ensure_directory(path: &Path) -> Result<()> {
    //     if !path.exists() {
    //         std::fs::create_dir_all(path)
    //             .with_context(|| format!("Failed to create directory: {}", path.display()))?;
    //     }
    //     Ok(())
    // }

    /// Get file size
    pub fn get_file_size(path: &Path) -> Result<u64> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("Failed to get metadata for: {}", path.display()))?;
        Ok(metadata.len())
    }
}

// /// Retry utilities
// pub mod retry {
//     use super::*;

//     /// Retry a fallible operation with exponential backoff
//     pub async fn with_backoff<F, Fut, T>(
//         mut operation: F,
//         max_attempts: u32,
//         base_delay: Duration,
//     ) -> Result<T>
//     where
//         F: FnMut() -> Fut,
//         Fut: std::future::Future<Output = Result<T>>,
//     {
//         let mut last_error = None;

//         for attempt in 1..=max_attempts {
//             match operation().await {
//                 Ok(result) => return Ok(result),
//                 Err(e) => {
//                     last_error = Some(e);
//                     if attempt < max_attempts {
//                         let delay = base_delay * 2_u32.pow(attempt - 1);
//                         sleep(delay).await;
//                     }
//                 }
//             }
//         }

//         Err(last_error.unwrap_or_else(|| anyhow!("Operation failed after {} attempts", max_attempts)))
//     }
// }

/// Timing utilities
pub mod timing {
    use super::*;

    /// Measure execution time of an operation
    pub async fn measure<F, Fut, T>(operation: F) -> (Result<T>, Duration)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let start = Instant::now();
        let result = operation().await;
        let duration = start.elapsed();
        (result, duration)
    }

    /// Format duration for display
    pub fn format_duration(duration: Duration) -> String {
        if duration.as_millis() < 1000 {
            format!("{}ms", duration.as_millis())
        } else if duration.as_secs() < 60 {
            format!("{:.1}s", duration.as_secs_f64())
        } else if duration.as_secs() < 3600 {
            format!("{}m {}s", duration.as_secs() / 60, duration.as_secs() % 60)
        } else {
            format!("{}h {}m", duration.as_secs() / 3600, (duration.as_secs() % 3600) / 60)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_format() {
        assert_eq!(timing::format_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(timing::format_duration(Duration::from_secs(30)), "30.0s");
        assert_eq!(timing::format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(timing::format_duration(Duration::from_secs(3660)), "1h 1m");
    }

    #[test]
    fn test_ssh_config_creation() {
        let config = ssh::SshConfig {
            host: "example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            key_path: Some("~/.ssh/id_rsa".to_string()),
            password: None,
        };

        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 22);
        assert_eq!(config.username, "user");
    }
}
