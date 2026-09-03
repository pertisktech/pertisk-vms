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
}

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
    let info = node_info();
    let listen_host = info.listen.split(':').next().unwrap_or("127.0.0.1");
    let api_port = info.listen.split(':').nth(1).unwrap_or("7480");
    let api_base = format!("http://{listen_host}:{api_port}");

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
    };

    io::stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;
    let result = loop_ui(&mut app).await;
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    result
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
    loop {
        terminal.draw(|f| draw(f, app))?;
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('r') => refresh(app).await,
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.selected = app.selected.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !app.vms.is_empty() {
                            app.selected = (app.selected + 1).min(app.vms.len() - 1);
                        }
                    }
                    KeyCode::Char('s') => vm_action(app, "start").await,
                    KeyCode::Char('x') => vm_action(app, "stop").await,
                    KeyCode::Char('h') => vm_action(app, "shutdown").await,
                    KeyCode::Char('b') => vm_action(app, "restart").await,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

async fn refresh(app: &mut App) {
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
            app.status = "refreshed".into();
        }
        Err(err) => app.error = format!("list: {err}"),
    }
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
    let mut spans = vec![
        Span::raw("j/k select  "),
        Span::styled("s", Style::default().fg(Color::Green)),
        Span::raw(" start  "),
        Span::styled("h", Style::default().fg(Color::Yellow)),
        Span::raw(" shutdown  "),
        Span::styled("b", Style::default().fg(Color::Cyan)),
        Span::raw(" restart  "),
        Span::styled("x", Style::default().fg(Color::Red)),
        Span::raw(" stop  "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" refresh  q quit"),
    ];
    if !app.status.is_empty() {
        spans.push(Span::raw("  |  "));
        spans.push(Span::styled(&app.status, Style::default().fg(Color::Green)));
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
