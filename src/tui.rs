use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Tabs, Wrap},
    Frame, Terminal,
};
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::logging::log;

/// Actions for the event loop
enum Action {
    Quit,
    NextTab,
    PrevTab,
    StartBuild,
    StartDeploy,
    StartMonitor,
    ClearStatus,
}

/// Main TUI application
pub struct App {
    pub config: Config,
    pub current_tab: Tab,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub build_state: BuildState,
    pub deploy_state: DeployState,
    pub monitor_state: MonitorState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tab {
    Build,
    Deploy,
    Monitor,
    Config,
    Exit,
}

impl Tab {
    pub fn next(&self) -> Self {
        match self {
            Tab::Build => Tab::Deploy,
            Tab::Deploy => Tab::Monitor,
            Tab::Monitor => Tab::Config,
            Tab::Config => Tab::Exit,
            Tab::Exit => Tab::Build,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Tab::Build => Tab::Exit,
            Tab::Deploy => Tab::Build,
            Tab::Monitor => Tab::Deploy,
            Tab::Config => Tab::Monitor,
            Tab::Exit => Tab::Config,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Tab::Build => "Build",
            Tab::Deploy => "Deploy",
            Tab::Monitor => "Monitor",
            Tab::Config => "Config",
            Tab::Exit => "Exit",
        }
    }
}

/// Build tab state
#[derive(Debug, Clone)]
pub struct BuildState {
    pub is_building: bool,
    pub progress: f64,
    pub logs: Vec<String>,
    pub build_info: Option<crate::commands::build::BuildInfo>,
}

/// Deploy tab state
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DeployState {
    pub is_deploying: bool,
    pub progress: f64,
    pub current_step: String,
    pub logs: Vec<String>,
    pub deployment_status: Option<crate::commands::deploy::DeploymentStatus>,
}

/// Monitor tab state
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MonitorState {
    pub is_monitoring: bool,
    pub status: Option<crate::commands::monitor::ApplicationStatus>,
    pub logs: Vec<String>,
    pub metrics: Option<crate::commands::monitor::MonitoringMetrics>,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            current_tab: Tab::Build,
            should_quit: false,
            status_message: None,
            build_state: BuildState {
                is_building: false,
                progress: 0.0,
                logs: Vec::new(),
                build_info: None,
            },
            deploy_state: DeployState {
                is_deploying: false,
                progress: 0.0,
                current_step: "Ready".to_string(),
                logs: Vec::new(),
                deployment_status: None,
            },
            monitor_state: MonitorState {
                is_monitoring: false,
                status: None,
                logs: Vec::new(),
                metrics: None,
            },
        }
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn next_tab(&mut self) {
        if self.current_tab == Tab::Exit {
            self.quit();
        } else {
            self.current_tab = self.current_tab.next();
        }
    }

    pub fn prev_tab(&mut self) {
        self.current_tab = self.current_tab.prev();
    }

    pub fn set_status(&mut self, message: String) {
        self.status_message = Some(message);
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }
}

/// Run the TUI application
pub async fn run_tui(config: Config) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = Arc::new(Mutex::new(App::new(config)));
    let res = run_app(&mut terminal, app.clone()).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

/// Run the main application loop
async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: Arc<Mutex<App>>,
) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(100);

    let event_app = app.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    let action = match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
                        KeyCode::Right | KeyCode::Char('l') => Some(Action::NextTab),
                        KeyCode::Left | KeyCode::Char('h') => Some(Action::PrevTab),
                        KeyCode::Char('b') => Some(Action::StartBuild),
                        KeyCode::Char('d') => Some(Action::StartDeploy),
                        KeyCode::Char('m') => Some(Action::StartMonitor),
                        KeyCode::Char('c') => Some(Action::ClearStatus),
                        _ => None,
                    };

                    if let Some(action) = action {
                        let mut app = event_app.lock().unwrap();
                        match action {
                            Action::Quit => {
                                app.quit();
                                break;
                            }
                            Action::NextTab => app.next_tab(),
                            Action::PrevTab => app.prev_tab(),
                            Action::StartBuild => {
                                let config = app.config.clone();
                                let tx_clone = tx.clone();
                                tokio::spawn(async move {
                                                                            let tx_for_error = tx_clone.clone();
                                    if let Err(e) = start_build_operation_async(config, tx_clone).await {
                                        let _ = tx_for_error.send(BackgroundMessage::BuildComplete(Err(e))).await;
                                    }
                                });
                            }
                            Action::StartDeploy => {
                                let config = app.config.clone();
                                let tx_clone = tx.clone();
                                tokio::spawn(async move {
                                    let tx_for_error = tx_clone.clone();
                                    if let Err(e) = start_deploy_operation_async(config, tx_clone).await {
                                        let _ = tx_for_error.send(BackgroundMessage::DeployComplete(Err(e))).await;
                                    }
                                });
                            }
                            Action::StartMonitor => {
                                let config = app.config.clone();
                                let tx_clone = tx.clone();
                                tokio::spawn(async move {
                                    if let Err(_e) = start_monitor_operation_async(config, tx_clone).await {
                                    }
                                });
                            }
                            Action::ClearStatus => app.clear_status(),
                        }
                    }
                }
            }
        }
    });

    loop {
        {
            let app = app.lock().unwrap();
            if app.should_quit {
                break;
            }
        }

        terminal.draw(|f| {
            let app = app.lock().unwrap();
            ui(f, &app);
        })?;

        if let Ok(message) = rx.try_recv() {
            let mut app = app.lock().unwrap();
            handle_background_message(&mut app, message);
        }
    }

    Ok(())
}

/// Handle messages from background tasks
fn handle_background_message(app: &mut App, message: BackgroundMessage) {
    match message {
        BackgroundMessage::BuildProgress(progress, log_line) => {
            app.build_state.progress = progress;
            if let Some(log) = log_line {
                app.build_state.logs.push(log);
            }
        }
        BackgroundMessage::BuildComplete(result) => {
            app.build_state.is_building = false;
            app.build_state.progress = 100.0;
            match result {
                Ok(info) => {
                    app.build_state.build_info = Some(info);
                    app.set_status("Build completed successfully".to_string());
                }
                Err(e) => {
                    app.set_status(format!("Build failed: {}", e));
                }
            }
        }
        BackgroundMessage::DeployProgress(progress, step, log_line) => {
            app.deploy_state.progress = progress;
            app.deploy_state.current_step = step;
            if let Some(log) = log_line {
                app.deploy_state.logs.push(log);
            }
        }
        BackgroundMessage::DeployComplete(result) => {
            app.deploy_state.is_deploying = false;
            app.deploy_state.progress = 100.0;
            match result {
                Ok(_) => {
                    app.set_status("Deployment completed successfully".to_string());
                }
                Err(e) => {
                    app.set_status(format!("Deployment failed: {}", e));
                }
            }
        }
        BackgroundMessage::MonitorUpdate(status, metrics) => {
            app.monitor_state.status = Some(status);
            app.monitor_state.metrics = metrics;
        }
    }
}

/// Background task messages
#[derive(Debug)]
pub enum BackgroundMessage {
    BuildProgress(f64, Option<String>),
    BuildComplete(Result<crate::commands::build::BuildInfo>),
    DeployProgress(f64, String, Option<String>),
    DeployComplete(Result<String>),
    MonitorUpdate(crate::commands::monitor::ApplicationStatus, Option<crate::commands::monitor::MonitoringMetrics>),
}

/// Start build operation asynchronously
async fn start_build_operation_async(
    config: Config,
    tx: mpsc::Sender<BackgroundMessage>,
) -> Result<()> {
    for i in 0..=100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = tx.send(BackgroundMessage::BuildProgress(
            i as f64,
            Some(format!("Building... {}%", i))
        )).await;
    }

    let result = crate::commands::build::build_project(&config, None, false).await;
    let build_info = crate::commands::build::get_build_info(&config);

    let _ = tx.send(BackgroundMessage::BuildComplete(build_info)).await;
    result?;
    Ok(())
}

/// Start deploy operation asynchronously
async fn start_deploy_operation_async(
    config: Config,
    tx: mpsc::Sender<BackgroundMessage>,
) -> Result<()> {
          let steps = [
        "Connecting to server...",
        "Creating remote directory...",
        "Uploading binary...",
        "Setting permissions...",
        "Creating systemd service...",
        "Starting service...",
      ];

    for (i, step) in steps.iter().enumerate() {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let progress = ((i + 1) as f64 / steps.len() as f64) * 100.0;
        let _ = tx.send(BackgroundMessage::DeployProgress(
            progress,
            step.to_string(),
            Some(format!("Step {}: {}", i + 1, step))
        )).await;
    }

    let result = crate::commands::deploy::deploy_project(&config, false, false, false).await;
    let _ = tx.send(BackgroundMessage::DeployComplete(result)).await;
    Ok(())
}

/// Start monitor operation asynchronously
async fn start_monitor_operation_async(
    config: Config,
    tx: mpsc::Sender<BackgroundMessage>,
) -> Result<()> {
    loop {
        let status_result = crate::commands::monitor::ApplicationMonitor::new(config.clone())
            .check_status()
            .await;

        match status_result {
            Ok(status) => {
                let metrics = crate::commands::monitor::get_metrics(&config).await.ok();
                let _ = tx.send(BackgroundMessage::MonitorUpdate(status, metrics)).await;
            }
            Err(e) => {
                log::monitor_event(&format!("Monitor error: {}", e));
            }
        }

        tokio::time::sleep(Duration::from_secs(config.monitor.interval_secs)).await;
    }
}

/// Main UI rendering function
fn ui(f: &mut Frame, app: &App) {
    let size = f.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(size);

    let title = Paragraph::new("🚀 rzen - Rust Project Manager")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let tab_titles: Vec<&str> = vec![
        Tab::Build.title(),
        Tab::Deploy.title(),
        Tab::Monitor.title(),
        Tab::Config.title(),
        Tab::Exit.title(),
    ];
    let tabs = Tabs::new(tab_titles)
        .select(app.current_tab as usize)
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(tabs, chunks[1]);

    match app.current_tab {
        Tab::Build => draw_build_tab(f, app, chunks[2]),
        Tab::Deploy => draw_deploy_tab(f, app, chunks[2]),
        Tab::Monitor => draw_monitor_tab(f, app, chunks[2]),
        Tab::Config => draw_config_tab(f, app, chunks[2]),
        Tab::Exit => draw_exit_tab(f, app, chunks[2]),
    }

    let status = app.status_message.as_deref()
        .unwrap_or("Press 'q' to quit | 'h/l' or arrow keys to navigate | 'b' build | 'd' deploy | 'm' monitor");
    let status_bar = Paragraph::new(status)
        .style(Style::default().fg(Color::Green))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(status_bar, chunks[3]);
}

/// Draw build tab
fn draw_build_tab(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    let progress = if app.build_state.is_building {
        Gauge::default()
            .block(Block::default().title("Build Progress").borders(Borders::ALL))
            .gauge_style(Style::default().fg(Color::Green))
            .percent(app.build_state.progress as u16)
            .label(format!("{:.1}%", app.build_state.progress))
    } else {
        Gauge::default()
            .block(Block::default().title("Build Status").borders(Borders::ALL))
            .gauge_style(Style::default().fg(Color::Gray))
            .percent(0)
            .label("Ready")
    };
    f.render_widget(progress, chunks[0]);

    let logs: Vec<ListItem> = app.build_state.logs
        .iter()
        .rev()
        .take(20)
        .rev()
        .map(|log| ListItem::new(log.as_str()))
        .collect();

    let logs_list = List::new(logs)
        .block(Block::default().title("Build Logs").borders(Borders::ALL))
        .style(Style::default().fg(Color::White));
    f.render_widget(logs_list, chunks[1]);

    let info_text = if let Some(info) = &app.build_state.build_info {
        format!("Binary: {} | Size: {} | Mode: {}",
                info.project_name,
                info.format_size(),
                info.build_mode)
    } else {
        "No build information available".to_string()
    };

    let info = Paragraph::new(info_text)
        .block(Block::default().title("Build Info").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(info, chunks[2]);
}

/// Draw deploy tab
fn draw_deploy_tab(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    let progress = if app.deploy_state.is_deploying {
        Gauge::default()
            .block(Block::default().title("Deploy Progress").borders(Borders::ALL))
            .gauge_style(Style::default().fg(Color::Green))
            .percent(app.deploy_state.progress as u16)
            .label(&app.deploy_state.current_step)
    } else {
        Gauge::default()
            .block(Block::default().title("Deploy Status").borders(Borders::ALL))
            .gauge_style(Style::default().fg(Color::Gray))
            .percent(0)
            .label("Ready")
    };
    f.render_widget(progress, chunks[0]);

    let logs: Vec<ListItem> = app.deploy_state.logs
        .iter()
        .rev()
        .take(20)
        .rev()
        .map(|log| ListItem::new(log.as_str()))
        .collect();

    let logs_list = List::new(logs)
        .block(Block::default().title("Deploy Logs").borders(Borders::ALL))
        .style(Style::default().fg(Color::White));
    f.render_widget(logs_list, chunks[1]);

    let status_text = format!("Target: {} | User: {}",
                             app.config.deploy.vps_host,
                             app.config.deploy.vps_user);

    let status = Paragraph::new(status_text)
        .block(Block::default().title("Deploy Target").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(status, chunks[2]);
}

/// Draw monitor tab
fn draw_monitor_tab(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    let status_text = if let Some(status) = &app.monitor_state.status {
        let health_icon = if status.is_healthy() { "🟢" } else { "🔴" };
        let _response_time = status.response_time
            .map(|d| format!("{}ms", d.as_millis()))
            .unwrap_or_else(|| "N/A".to_string());

        format!("{} {}", health_icon, status.summary())
    } else {
        "No monitoring data available".to_string()
    };

    let status = Paragraph::new(status_text)
        .block(Block::default().title("Application Status").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(status, chunks[0]);

    let logs: Vec<ListItem> = app.monitor_state.logs
        .iter()
        .rev()
        .take(20)
        .rev()
        .map(|log| ListItem::new(log.as_str()))
        .collect();

    let logs_list = List::new(logs)
        .block(Block::default().title("Monitor Logs").borders(Borders::ALL))
        .style(Style::default().fg(Color::White));
    f.render_widget(logs_list, chunks[1]);

    let metrics_text = if let Some(metrics) = &app.monitor_state.metrics {
        format!("Uptime: {:.1}% | Errors: {} | Last Check: {}",
                metrics.uptime_percentage,
                metrics.error_count,
                metrics.last_check.format("%H:%M:%S"))
    } else {
        "No metrics available".to_string()
    };

    let metrics = Paragraph::new(metrics_text)
        .block(Block::default().title("Metrics").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(metrics, chunks[2]);
}

/// Draw config tab
fn draw_config_tab(f: &mut Frame, app: &App, area: Rect) {
    let config_text = format!(
        "Project: {}\n\
         Build Mode: {}\n\
         Target: {}\n\
         Host: {}\n\
         Health Endpoint: {}\n\
         Log Path: {}",
        app.config.project.name,
        app.config.project.build_mode,
        app.config.deploy.target,
        app.config.deploy.vps_host,
        app.config.monitor.health_endpoint.as_deref().unwrap_or("None"),
        app.config.monitor.log_path.as_deref().unwrap_or("None")
    );

    let config = Paragraph::new(config_text)
        .block(Block::default().title("Configuration").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(config, area);
}

/// Draw exit tab
fn draw_exit_tab(f: &mut Frame, _app: &App, area: Rect) {
    let exit_text = "Are you sure you want to exit?\n\n\
                     Press 'q' or 'Enter' to confirm exit\n\
                     Press 'h' or left arrow to go back";

    let exit = Paragraph::new(exit_text)
        .block(Block::default().title("Exit").borders(Borders::ALL))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(exit, area);
}
