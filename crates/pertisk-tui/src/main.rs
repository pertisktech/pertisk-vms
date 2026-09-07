//! Local node console: show IP, admin password, and control guests.

use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use pertisk_types::{DEFAULT_LISTEN, VmId, VmRecord};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use reqwest::Client;
use serde::Deserialize;

const PASS_FILE: &str = "/etc/pertisk/admin";

#[derive(Clone)]
struct NodeInfo {
    ips: Vec<String>,
    password: String,
    listen: String,
    ui_url: String,
}

struct App {
    info: NodeInfo,
    client: Client,
    api_base: String,
    token: Option<String>,
    vms: Vec<VmRecord>,
    selected: usize,
    status: String,
    error: String,
    last_refresh: std::time::Instant,
    /// First `d` arms destroy; second `d` within a few seconds confirms.
    pending_delete: Option<(VmId, std::time::Instant)>,
}

const REFRESH_EVERY: Duration = Duration::from_secs(3);

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
}

fn main() -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;
    rt.block_on(run())
}

async fn run() -> Result<()> {
    // Soften kernel console spam without breaking the Linux VT redraw path.
    let _quiet = ConsoleQuiet::enter();

    let info = node_info();
    // Always hit the local daemon; PERTISK_LISTEN may be 0.0.0.0:7480.
    let api_port = info.listen.split(':').nth(1).unwrap_or("7480");
    let api_base = format!("http://127.0.0.1:{api_port}");

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("http client")?;

    let token = login(&client, &api_base, &info.password).await.ok();
    let vms = if let Some(ref token) = token {
        fetch_vms(&client, &api_base, token)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut app = App {
        info,
        client,
        api_base,
        token,
        vms,
        selected: 0,
        status: String::new(),
        error: String::new(),
        last_refresh: std::time::Instant::now(),
        pending_delete: None,
    };

    // Linux VGA/serial consoles (TERM=linux) often go blank with the alternate
    // screen buffer. Draw on the primary screen there instead.
    let use_alt_screen = supports_alternate_screen();
    if use_alt_screen {
        io::stdout().execute(EnterAlternateScreen)?;
    } else {
        io::stdout().execute(crossterm::terminal::Clear(
            crossterm::terminal::ClearType::All,
        ))?;
        io::stdout().execute(crossterm::cursor::MoveTo(0, 0))?;
    }
    enable_raw_mode()?;
    let result = loop_ui(&mut app).await;
    disable_raw_mode()?;
    if use_alt_screen {
        io::stdout().execute(LeaveAlternateScreen)?;
    } else {
        io::stdout().execute(crossterm::terminal::Clear(
            crossterm::terminal::ClearType::All,
        ))?;
        io::stdout().execute(crossterm::cursor::Show)?;
    }
    result
}

fn supports_alternate_screen() -> bool {
    match std::env::var("TERM") {
        Ok(term) => {
            let t = term.to_ascii_lowercase();
            !(t.is_empty()
                || t == "linux"
                || t == "dumb"
                || t == "vt100"
                || t == "vt102"
                || t == "ansi"
                || t.starts_with("cons"))
        }
        Err(_) => false,
    }
}

/// Mute console printk while the TUI owns the terminal.
struct ConsoleQuiet {
    printk: Option<String>,
}

impl ConsoleQuiet {
    fn enter() -> Self {
        let printk = std::fs::read_to_string("/proc/sys/kernel/printk").ok();
        // console_loglevel=1 → emergencies only on the console
        let _ = std::fs::write("/proc/sys/kernel/printk", "1 4 1 7\n");
        let _ = std::process::Command::new("dmesg").args(["-n", "1"]).status();
        Self { printk }
    }
}

impl Drop for ConsoleQuiet {
    fn drop(&mut self) {
        if let Some(ref prev) = self.printk {
            let _ = std::fs::write("/proc/sys/kernel/printk", prev);
        }
        let _ = std::process::Command::new("dmesg").args(["-n", "7"]).status();
    }
}

fn node_info() -> NodeInfo {
    let ips = local_ips();
    let password = admin_password();
    let listen = std::env::var("PERTISK_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.to_string());
    let ui_host = url_host(&ips);
    let tls_listen = std::env::var("PERTISK_TLS_LISTEN").unwrap_or_else(|_| "0.0.0.0:7443".into());
    let tls_port = tls_listen
        .rsplit(':')
        .next()
        .filter(|p| *p != "off" && !p.is_empty())
        .unwrap_or("7443");
    NodeInfo {
        ui_url: format!("https://{ui_host}:{tls_port}/"),
        ips,
        password,
        listen,
    }
}

fn url_host(ips: &[String]) -> String {
    let raw = ips
        .iter()
        .find(|ip| ip.contains('.'))
        .or_else(|| ips.first())
        .map(|s| s.as_str())
        .unwrap_or("127.0.0.1");
    if raw.contains(':') {
        format!("[{raw}]")
    } else {
        raw.to_string()
    }
}

fn local_ips() -> Vec<String> {
    let addrs = pertisk_types::probe_host_addrs();
    let mut ips = addrs.ipv4;
    ips.extend(addrs.ipv6);
    if ips.is_empty() {
        vec!["127.0.0.1".into()]
    } else {
        ips
    }
}

fn admin_password() -> String {
    if let Ok(text) = std::fs::read_to_string(PASS_FILE) {
        let pw = text.trim();
        if !pw.is_empty() {
            return pw.into();
        }
    }
    if let Ok(pw) = std::env::var("PERTISK_ADMIN_PASSWORD") {
        if !pw.is_empty() {
            return pw;
        }
    }
    "admin".into()
}

async fn login(client: &Client, base: &str, password: &str) -> Result<String> {
    let response = client
        .post(format!("{base}/v1/login"))
        .json(&serde_json::json!({ "username": "admin", "password": password }))
        .send()
        .await
        .context("login request")?;
    if !response.status().is_success() {
        anyhow::bail!("login failed: {}", response.status());
    }
    let body: TokenResponse = response.json().await.context("login json")?;
    Ok(body.token)
}

async fn fetch_vms(client: &Client, base: &str, token: &str) -> Result<Vec<VmRecord>> {
    let response = client
        .get(format!("{base}/v1/vms"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await?;
    let response = response.error_for_status()?;
    Ok(response.json().await?)
}

async fn power(
    client: &Client,
    base: &str,
    token: &str,
    id: VmId,
    action: &str,
) -> Result<VmRecord> {
    let response = client
        .post(format!("{base}/v1/vms/{id}/{action}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await?;
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("{action} failed: {text}");
    }
    Ok(response.json().await?)
}

async fn loop_ui(app: &mut App) -> Result<()> {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    loop {
        if app
            .pending_delete
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(5))
        {
            app.pending_delete = None;
            app.status.clear();
        }
        if app.pending_delete.is_none() && app.last_refresh.elapsed() >= REFRESH_EVERY {
            refresh(app).await;
        }
        terminal.draw(|f| draw(f, app))?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Esc => {
                        app.pending_delete = None;
                        app.status.clear();
                    }
                    KeyCode::Char('r') => {
                        app.pending_delete = None;
                        refresh(app).await;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.pending_delete = None;
                        app.selected = app.selected.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        app.pending_delete = None;
                        if !app.vms.is_empty() {
                            app.selected = (app.selected + 1).min(app.vms.len() - 1);
                        }
                    }
                    KeyCode::Char('s') => {
                        app.pending_delete = None;
                        vm_action(app, "start").await;
                    }
                    KeyCode::Char('x') => {
                        app.pending_delete = None;
                        vm_action(app, "stop").await;
                    }
                    KeyCode::Char('h') => {
                        app.pending_delete = None;
                        vm_action(app, "shutdown").await;
                    }
                    KeyCode::Char('b') => {
                        app.pending_delete = None;
                        vm_action(app, "restart").await;
                    }
                    KeyCode::Char('d') => destroy_selected(app).await,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

async fn refresh(app: &mut App) {
    app.last_refresh = std::time::Instant::now();
    app.info.ips = local_ips();
    app.info.password = admin_password();
    let ui_host = url_host(&app.info.ips);
    let tls_port = app
        .info
        .ui_url
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .unwrap_or("7443");
    app.info.ui_url = format!("https://{ui_host}:{tls_port}/");
    app.error.clear();
    if app.token.is_none() {
        match login(&app.client, &app.api_base, &app.info.password).await {
            Ok(token) => app.token = Some(token),
            Err(err) => {
                app.error = format!("login: {err}");
                return;
            }
        }
    }
    let Some(token) = app.token.clone() else {
        return;
    };
    match fetch_vms(&app.client, &app.api_base, &token).await {
        Ok(vms) => {
            app.vms = vms;
            if app.selected >= app.vms.len() {
                app.selected = app.vms.len().saturating_sub(1);
            }
            app.status = "auto".into();
        }
        Err(err) => {
            app.token = None;
            app.error = format!("list: {err}");
        }
    }
}

async fn destroy_selected(app: &mut App) {
    app.error.clear();
    let Some(vm) = app.vms.get(app.selected).cloned() else {
        app.error = "no guest selected".into();
        return;
    };
    let id = vm.id;
    let name = vm.spec.name.clone();

    if let Some((pending_id, _)) = app.pending_delete
        && pending_id == id
    {
        app.pending_delete = None;
        let Some(token) = app.token.clone() else {
            app.error = "not logged in".into();
            return;
        };
        match delete_vm(&app.client, &app.api_base, &token, id).await {
            Ok(()) => {
                app.vms.retain(|v| v.id != id);
                if app.selected >= app.vms.len() {
                    app.selected = app.vms.len().saturating_sub(1);
                }
                app.status = format!("deleted {id} ({name})");
                app.last_refresh = std::time::Instant::now();
            }
            Err(err) => app.error = format!("delete: {err}"),
        }
        return;
    }

    app.pending_delete = Some((id, std::time::Instant::now()));
    app.status = format!("DELETE {id} ({name})? press d again, Esc cancel");
}

async fn delete_vm(client: &Client, base: &str, token: &str, id: VmId) -> Result<()> {
    let response = client
        .delete(format!("{base}/v1/vms/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .context("delete request")?;
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("{text}");
    }
    Ok(())
}

async fn vm_action(app: &mut App, action: &str) {
    app.error.clear();
    app.status.clear();
    let Some(vm) = app.vms.get(app.selected) else {
        app.error = "no guest selected".into();
        return;
    };
    let Some(token) = app.token.clone() else {
        app.error = "not logged in".into();
        return;
    };
    let id = vm.id;
    match power(&app.client, &app.api_base, &token, id, action).await {
        Ok(updated) => {
            if let Some(row) = app.vms.iter_mut().find(|v| v.id == id) {
                *row = updated;
            }
            app.status = format!("{action} {id} ok");
        }
        Err(err) => app.error = err.to_string(),
    }
}

fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(f.area());

    draw_info(f, chunks[0], app);
    draw_vms(f, chunks[1], app);
    draw_help(f, chunks[2], app);
}

fn draw_info(f: &mut Frame, area: Rect, app: &App) {
    let ips = if app.info.ips.is_empty() {
        "—".into()
    } else {
        app.info.ips.join(", ")
    };
    let auth = if app.token.is_some() {
        Span::styled("connected", Style::default().fg(Color::Green))
    } else {
        Span::styled("offline", Style::default().fg(Color::Yellow))
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("Web UI ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&app.info.ui_url),
        ]),
        Line::from(vec![
            Span::styled("IP(s)  ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(ips),
        ]),
        Line::from(vec![
            Span::styled("User   ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("admin"),
        ]),
        Line::from(vec![
            Span::styled("Password ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(&app.info.password, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("API    ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&app.api_base),
            Span::raw("  "),
            auth,
        ]),
    ];
    let block = Block::default().title(" pertisk-vm ").borders(Borders::ALL);
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_vms(f: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec!["ID", "NAME", "STATE", "CPU", "MEM"])
        .style(Style::default().add_modifier(Modifier::BOLD));
    let rows: Vec<Row> = app
        .vms
        .iter()
        .enumerate()
        .map(|(i, vm)| {
            let style = if i == app.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(vm.id.to_string()),
                Cell::from(vm.spec.name.clone()),
                Cell::from(format!("{}", vm.state)),
                Cell::from(vm.spec.vcpus.to_string()),
                Cell::from(format!("{} MiB", vm.spec.memory_mib)),
            ])
            .style(style)
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Min(12),
            Constraint::Length(10),
            Constraint::Length(4),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(Block::default().title(" Guests ").borders(Borders::ALL));
    f.render_widget(table, area);
}

fn draw_help(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = if app.pending_delete.is_some() {
        vec![
            Span::styled(
                "CONFIRM DELETE: press d again  |  Esc cancel",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]
    } else {
        vec![
            Span::raw("j/k select  "),
            Span::styled("s", Style::default().fg(Color::Green)),
            Span::raw(" start  "),
            Span::styled("h", Style::default().fg(Color::Yellow)),
            Span::raw(" shutdown  "),
            Span::styled("b", Style::default().fg(Color::Cyan)),
            Span::raw(" restart  "),
            Span::styled("x", Style::default().fg(Color::Red)),
            Span::raw(" stop  "),
            Span::styled("d", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" delete  "),
            Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" refresh  q quit"),
        ]
    };
    if !app.status.is_empty() {
        spans.push(Span::raw("  |  "));
        let color = if app.pending_delete.is_some() {
            Color::Yellow
        } else {
            Color::Green
        };
        spans.push(Span::styled(&app.status, Style::default().fg(color)));
    }
    if !app.error.is_empty() {
        spans.push(Span::raw("  |  "));
        spans.push(Span::styled(&app.error, Style::default().fg(Color::Red)));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_ips_non_empty_fallback() {
        let ips = local_ips();
        assert!(!ips.is_empty());
    }

    #[test]
    fn url_host_brackets_ipv6() {
        assert_eq!(url_host(&["2001:db8::1".into()]), "[2001:db8::1]");
        assert_eq!(
            url_host(&["2001:db8::1".into(), "10.0.0.5".into()]),
            "10.0.0.5"
        );
    }
}
