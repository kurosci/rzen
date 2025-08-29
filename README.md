# rzen

A comprehensive TUI-based CLI tool for building, deploying, and monitoring Rust applications. Designed specifically for developers who want to easily manage the complete lifecycle of their Rust projects, from development to production deployment.

## Features

- **🏗️ Build**: Compile your Rust projects with Cargo integration
- **🚀 Deploy**: Deploy to VPS via SSH with systemd service management
- **👀 Monitor**: Real-time health monitoring and log tailing
- **🖥️ TUI Interface**: Beautiful terminal user interface with Ratatui
- **⚙️ Configuration**: Simple TOML-based configuration
- **🔧 CLI Mode**: Command-line interface for automation
- **📊 Progress Tracking**: Visual progress bars for long operations
- **🔒 Security**: SSH key-based authentication with fallback to passwords
- **♻️ Reliability**: Robust error handling and retry mechanisms

## Quick Start

### 1. Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/rzen.git
cd rzen

# Build the project
cargo build --release

# Install (optional)
cargo install --path .
```

### 2. Initialize Configuration

```bash
# Create a default configuration file
rzen init

# Or specify custom values
rzen init --name my-api --host my-server.com
```

### 3. Configure Your Project

Edit `rzen.toml` with your project settings:

```toml
[project]
name = "my-api"
path = "."
build_mode = "release"

[deploy]
vps_host = "your-server.com"
vps_user = "deploy"
vps_key_path = "~/.ssh/id_rsa"
deploy_path = "/opt/my-api"

[monitor]
health_endpoint = "http://your-server.com:8080/health"
log_path = "/var/log/my-api.log"
```

### 4. Use rzen

```bash
# Start TUI interface (default)
rzen

# Build your project
rzen build

# Deploy to production
rzen deploy

# Monitor your application
rzen monitor

# Validate configuration
rzen validate
```

## Usage

### TUI Mode

Run `rzen` without arguments to launch the interactive TUI:

```
┌─────────────────────────────────────────────────────────────────┐
│ rzen - Rust Project Manager                                   │
├─────────────────────────────────────────────────────────────────┤
│ Build │ Deploy │ Monitor │ Config │ Exit                       │
├─────────────────────────────────────────────────────────────────┤
│ Build Status                                                   │
│ ┌─────────────────────────────────────────────────────────┐    │
│ │ Build Progress                                          │    │
│ │ ████████████████████████████████ 100%                 │    │
│ └─────────────────────────────────────────────────────────┘    │
│                                                                │
│ Build Logs                                                    │
│ ┌─────────────────────────────────────────────────────────┐    │
│ │ Compiling my-api v0.1.0 (/path/to/project)             │    │
│ │ Finished release [optimized] target(s) in 12.34s       │    │
│ └─────────────────────────────────────────────────────────┘    │
│                                                                │
│ Build Info                                                    │
│ ┌─────────────────────────────────────────────────────────┐    │
│ │ Binary: my-api | Size: 8.5 MB | Mode: release          │    │
│ └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

**Navigation:**
- `h` / `←` : Previous tab
- `l` / `→` : Next tab
- `b` : Start build
- `d` : Start deploy
- `m` : Start monitoring
- `q` / `Esc` : Quit

### CLI Commands

#### Build
```bash
rzen build                    # Build in default mode
rzen build --mode debug       # Build in debug mode
rzen build --dry-run          # Simulate build
```

#### Deploy
```bash
rzen deploy                   # Build and deploy
rzen deploy --skip-build      # Deploy existing binary
rzen deploy --force           # Force redeployment
rzen deploy --dry-run         # Simulate deployment
```

#### Monitor
```bash
rzen monitor                  # One-time status check
rzen monitor --continuous     # Continuous monitoring
rzen monitor --lines 50       # Show last 50 log lines
```

#### Configuration
```bash
rzen init                     # Create default config
rzen init my-config.toml      # Create config with custom name
rzen validate                 # Validate current config
rzen validate custom.toml     # Validate specific config
```

### Global Options

```bash
rzen --config custom.toml     # Use custom config file
rzen --log-level 4            # Set log level (0-5)
rzen --dry-run                # Simulate operations
rzen --help                   # Show help
rzen --version                # Show version
```

## Configuration

The `rzen.toml` configuration file supports the following sections:

### [project]
- `path`: Path to your Rust project
- `name`: Project name (used for binary and service names)
- `build_mode`: "debug" or "release"

### [deploy]
- `target`: Deployment target ("vps" for now)
- `vps_host`: Server hostname or IP
- `vps_user`: SSH username
- `vps_key_path`: Path to SSH private key
- `vps_password`: SSH password (alternative to key)
- `deploy_path`: Remote installation directory
- `service_name`: Systemd service name
- `ssh_port`: SSH port (default: 22)

### [monitor]
- `health_endpoint`: HTTP endpoint for health checks
- `log_path`: Remote log file path
- `interval_secs`: Monitoring poll interval
- `health_timeout_secs`: Health check timeout

## Architecture

```
src/
├── main.rs           # Application entry point and CLI routing
├── cli.rs            # Command-line argument parsing
├── config.rs         # TOML configuration handling
├── logging.rs        # Structured logging system
├── tui.rs           # Terminal user interface
├── commands/         # Command implementations
│   ├── build.rs     # Build functionality
│   ├── deploy.rs    # Deployment functionality
│   └── monitor.rs   # Monitoring functionality
└── utils/           # Shared utilities (SSH, progress, etc.)
```

## Requirements

- **Rust**: 1.70+ (2021 edition)
- **Cargo**: Latest stable
- **SSH**: For deployment (key-based auth recommended)
- **Systemd**: For service management on target server

### Target Server Requirements

- Linux with systemd
- SSH access
- sudo privileges for service management
- Rust/Cargo (optional, binaries can be cross-compiled)

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run
```

### Testing

```bash
# Unit tests
cargo test

# Integration tests
cargo test --test integration

# With coverage
cargo tarpaulin
```

### Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Run `cargo fmt` and `cargo clippy`
6. Submit a pull request

## Troubleshooting

### Common Issues

**SSH Connection Failed**
- Verify SSH key permissions: `chmod 600 ~/.ssh/id_rsa`
- Check SSH agent: `ssh-add ~/.ssh/id_rsa`
- Test connection: `ssh user@host`

**Build Failed**
- Ensure `Cargo.toml` exists in project directory
- Check Rust version: `rustc --version`
- Clean and rebuild: `cargo clean && cargo build`

**Service Won't Start**
- Check service status: `sudo systemctl status your-service`
- View logs: `sudo journalctl -u your-service`
- Verify binary permissions: `ls -la /opt/your-app/`

**Configuration Errors**
- Validate config: `rzen validate`
- Check TOML syntax
- Ensure all required fields are present

### Debug Mode

Enable verbose logging:

```bash
rzen --log-level 5 build
```

Or set environment variable:

```bash
RUST_LOG=trace rzen build
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Roadmap

### MVP (Current)
- ✅ TUI interface
- ✅ Build system integration
- ✅ SSH-based VPS deployment
- ✅ Systemd service management
- ✅ Health monitoring
- ✅ TOML configuration

### Future Enhancements
- [ ] Multi-target deployment (AWS, Kubernetes)
- [ ] Docker container support
- [ ] Advanced monitoring with metrics
- [ ] Rollback functionality
- [ ] Plugin system
- [ ] Web dashboard
- [ ] CI/CD integration

## Acknowledgments

- [Ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI framework
- [Clap](https://github.com/clap-rs/clap) - CLI argument parsing
- [Tokio](https://tokio.rs/) - Async runtime
- [SSH2](https://github.com/alexcrichton/ssh2-rs) - SSH client library

---
