//! Local node console: live host, cluster, and guest detail plus power control.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use pertisk_types::{
    ClusterStatus, DEFAULT_LISTEN, HostInfo, NodeId, NodeMetrics, VERSION, VmId, VmMetrics,
    VmRecord, VmState,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use reqwest::Client;
use serde::Deserialize;

const PASS_FILE: &str = "/etc/pertisk/admin";
const REFRESH_EVERY: Duration = Duration::from_secs(1);
const BRAND: Color = Color::Rgb(46, 170, 160);

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
    last_refresh: Instant,
    /// First `d` arms destroy; second `d` within a few seconds confirms.
    pending_delete: Option<(VmId, Instant)>,
    host: Option<HostInfo>,
    cluster: Option<ClusterStatus>,
    node: Option<NodeMetrics>,
    vm_live: HashMap<VmId, LiveStats>,
    refresh_job: Option<tokio::task::JoinHandle<RefreshOutcome>>,
}

#[derive(Clone, Default)]
struct LiveStats {
    cpu_pct: f32,
    mem_used: u64,
    mem_total: u64,
    disk_used: u64,
    disk_total: u64,
    net_rx_bps: u64,
    net_tx_bps: u64,
}

struct RefreshOutcome {
    token: Option<String>,
    login_error: Option<String>,
    vms: Result<Vec<VmRecord>, String>,
    host: Option<HostInfo>,
    cluster: Option<ClusterStatus>,
    node: Option<NodeMetrics>,
    vm_live: HashMap<VmId, LiveStats>,
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
    if std::env::args()
        .skip(1)
        .any(|a| a == "--version" || a == "-V")
    {
        println!("pertisk-tui {VERSION}");
        return Ok(());
    }

    // Soften kernel console spam without breaking the Linux VT redraw path.
    let _quiet = ConsoleQuiet::enter();

    let info = node_info();
    // Always hit the local daemon; PERTISK_LISTEN may be 0.0.0.0:7480.
    let api_port = info.listen.split(':').nth(1).unwrap_or("7480");
    let api_base = format!("http://127.0.0.1:{api_port}");

    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .context("http client")?;

    let mut app = App {
        info,
        client,
        api_base,
        token: None,
        vms: Vec::new(),
        selected: 0,
        status: "connecting".into(),
        error: String::new(),
        last_refresh: Instant::now()
            .checked_sub(REFRESH_EVERY)
            .unwrap_or_else(Instant::now),
        pending_delete: None,
        host: None,
        cluster: None,
        node: None,
        vm_live: HashMap::new(),
        refresh_job: None,
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
    if let Some(job) = app.refresh_job.take() {
        job.abort();
    }
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
        let _ = std::process::Command::new("dmesg")
            .args(["-n", "1"])
            .status();
        Self { printk }
    }
}

impl Drop for ConsoleQuiet {
    fn drop(&mut self) {
        if let Some(ref prev) = self.printk {
            let _ = std::fs::write("/proc/sys/kernel/printk", prev);
        }
        let _ = std::process::Command::new("dmesg")
            .args(["-n", "7"])
            .status();
    }
}

fn node_info() -> NodeInfo {
    let ips = local_ips();
    let password = admin_password();
    let listen = std::env::var("PERTISK_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.to_string());
    let ui_host = url_host(&ips);
    let http_port = listen.split(':').nth(1).unwrap_or("7480");
    NodeInfo {
        ui_url: format!("http://{ui_host}:{http_port}/"),
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

async fn fetch_host(client: &Client, base: &str, token: &str) -> Result<HostInfo> {
    let response = client
        .get(format!("{base}/v1/host"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .context("host request")?;
    let response = response.error_for_status()?;
    Ok(response.json().await.context("host json")?)
}

async fn fetch_cluster(client: &Client, base: &str, token: &str) -> Result<ClusterStatus> {
    let response = client
        .get(format!("{base}/v1/cluster"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .context("cluster request")?;
    let response = response.error_for_status()?;
    Ok(response.json().await.context("cluster json")?)
}

async fn fetch_node_metrics(client: &Client, base: &str, token: &str) -> Result<NodeMetrics> {
    let response = client
        .get(format!("{base}/v1/metrics/node"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .context("metrics request")?;
    let response = response.error_for_status()?;
    Ok(response.json().await.context("metrics json")?)
}

async fn fetch_vm_metrics(client: &Client, base: &str, token: &str, id: VmId) -> Result<LiveStats> {
    let response = client
        .get(format!("{base}/v1/vms/{id}/metrics"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .context("vm metrics request")?;
    let response = response.error_for_status()?;
    let body: VmMetrics = response.json().await.context("vm metrics json")?;
    Ok(live_from_metrics(body))
}

fn live_from_metrics(body: VmMetrics) -> LiveStats {
    if let Some(live) = body.live {
        LiveStats {
            cpu_pct: live.cpu_pct,
            mem_used: live.mem_used_bytes,
            mem_total: live.mem_total_bytes,
            disk_used: live.disk_used_bytes,
            disk_total: live.disk_total_bytes,
            net_rx_bps: live.net_rx_bps,
            net_tx_bps: live.net_tx_bps,
        }
    } else {
        LiveStats {
            mem_total: u64::from(body.memory_mib).saturating_mul(1024 * 1024),
            ..LiveStats::default()
        }
    }
}

async fn fetch_running_metrics(
    client: &Client,
    base: &str,
    token: &str,
    vms: &[VmRecord],
) -> HashMap<VmId, LiveStats> {
    let mut jobs = tokio::task::JoinSet::new();
    for vm in vms.iter().filter(|vm| vm.state == VmState::Running) {
        let client = client.clone();
        let base = base.to_string();
        let token = token.to_string();
        let id = vm.id;
        jobs.spawn(async move {
            let stats = fetch_vm_metrics(&client, &base, &token, id).await.ok();
            (id, stats)
        });
    }
    let mut out = HashMap::new();
    while let Some(joined) = jobs.join_next().await {
        if let Ok((id, Some(stats))) = joined {
            out.insert(id, stats);
        }
    }
    out
}

async fn load_snapshot(client: &Client, base: &str, token: &str) -> RefreshOutcome {
    let (vms, host, cluster, node) = tokio::join!(
        fetch_vms(client, base, token),
        fetch_host(client, base, token),
        fetch_cluster(client, base, token),
        fetch_node_metrics(client, base, token),
    );
    let vm_live = match &vms {
        Ok(list) => fetch_running_metrics(client, base, token, list).await,
        Err(_) => HashMap::new(),
    };
    RefreshOutcome {
        token: None,
        login_error: None,
        vms: vms.map_err(|err| err.to_string()),
        host: host.ok(),
        cluster: cluster.ok(),
        node: node.ok(),
        vm_live,
    }
}

fn format_bytes(n: u64) -> String {
    const KIB: f64 = 1024.0;
    let v = n as f64;
    if v < KIB {
        return format!("{n} B");
    }
    let units = ["KiB", "MiB", "GiB", "TiB"];
    let mut x = v;
    let mut i = 0usize;
    loop {
        x /= KIB;
        if x < KIB || i + 1 >= units.len() {
            break;
        }
        i += 1;
    }
    if x >= 10.0 {
        format!("{x:.0} {}", units[i])
    } else {
        format!("{x:.1} {}", units[i])
    }
}

fn format_used_total(used: u64, total: u64) -> String {
    if total == 0 {
        return "—".into();
    }
    format!("{}/{}", format_bytes(used), format_bytes(total))
}

fn format_rate(bps: u64) -> String {
    if bps == 0 {
        return "0".into();
    }
    format!("{}/s", format_bytes(bps))
}

fn format_duration(secs: u64) -> String {
    if secs == 0 {
        return "0s".into();
    }
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else if mins > 0 {
        format!("{mins}m")
    } else {
        format!("{secs}s")
    }
}

fn meter(pct: f32, width: usize) -> String {
    let width = width.max(1);
    let clamped = pct.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * width as f32).round() as usize;
    let filled = filled.min(width);
    format!("[{}{}]", "#".repeat(filled), "-".repeat(width - filled))
}

fn meter_color(pct: f32) -> Color {
    if pct >= 90.0 {
        Color::Red
    } else if pct >= 70.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn state_color(state: VmState) -> Color {
    match state {
        VmState::Running => Color::Green,
        VmState::Failed => Color::Red,
        VmState::Created => Color::Yellow,
        VmState::Stopped => Color::DarkGray,
    }
}

fn node_name(cluster: Option<&ClusterStatus>, id: Option<NodeId>) -> String {
    let Some(id) = id else {
        return "—".into();
    };
    cluster
        .and_then(|cluster| {
            cluster
                .members
                .iter()
                .find(|member| member.id == id)
                .map(|member| member.name.clone())
        })
        .unwrap_or_else(|| id.to_string())
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
            if app.error.is_empty() {
                app.status.clear();
            }
        }
        poll_refresh(app).await;
        if app.refresh_job.is_none() && app.last_refresh.elapsed() >= REFRESH_EVERY {
            kick_refresh(app);
        }
        terminal.draw(|f| draw(f, app))?;
        if event::poll(Duration::from_millis(80))? {
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
                        app.last_refresh = Instant::now()
                            .checked_sub(REFRESH_EVERY)
                            .unwrap_or_else(Instant::now);
                        kick_refresh(app);
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

fn kick_refresh(app: &mut App) {
    if app.refresh_job.is_some() {
        return;
    }
    app.info.ips = local_ips();
    app.info.password = admin_password();
    let ui_host = url_host(&app.info.ips);
    let http_port = app.info.listen.split(':').nth(1).unwrap_or("7480");
    app.info.ui_url = format!("http://{ui_host}:{http_port}/");

    let client = app.client.clone();
    let base = app.api_base.clone();
    let token = app.token.clone();
    let password = app.info.password.clone();
    app.last_refresh = Instant::now();
    app.refresh_job = Some(tokio::spawn(async move {
        let token = match token {
            Some(token) => token,
            None => match login(&client, &base, &password).await {
                Ok(token) => token,
                Err(err) => {
                    return RefreshOutcome {
                        token: None,
                        login_error: Some(err.to_string()),
                        vms: Err("not logged in".into()),
                        host: None,
                        cluster: None,
                        node: None,
                        vm_live: HashMap::new(),
                    };
                }
            },
        };
        let mut outcome = load_snapshot(&client, &base, &token).await;
        outcome.token = Some(token);
        outcome
    }));
}

async fn poll_refresh(app: &mut App) {
    let Some(job) = app.refresh_job.as_ref() else {
        return;
    };
    if !job.is_finished() {
        return;
    }
    let Some(job) = app.refresh_job.take() else {
        return;
    };
    let Ok(outcome) = job.await else {
        return;
    };
    if let Some(err) = outcome.login_error {
        app.token = None;
        app.error = format!("login: {err}");
        return;
    }
    if let Some(token) = outcome.token {
        app.token = Some(token);
    }
    match outcome.vms {
        Ok(vms) => {
            app.vms = vms;
            if app.selected >= app.vms.len() {
                app.selected = app.vms.len().saturating_sub(1);
            }
            app.vm_live = outcome.vm_live;
            app.error.clear();
            if app.pending_delete.is_none() && app.status == "connecting" {
                app.status.clear();
            }
        }
        Err(err) => {
            app.token = None;
            app.error = format!("list: {err}");
        }
    }
    if outcome.host.is_some() {
        app.host = outcome.host;
    }
    if outcome.cluster.is_some() {
        app.cluster = outcome.cluster;
    }
    if outcome.node.is_some() {
        app.node = outcome.node;
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
                app.vm_live.remove(&id);
                if app.selected >= app.vms.len() {
                    app.selected = app.vms.len().saturating_sub(1);
                }
                app.status = format!("deleted {id} ({name})");
            }
            Err(err) => app.error = format!("delete: {err}"),
        }
        return;
    }

    app.pending_delete = Some((id, Instant::now()));
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
            app.last_refresh = Instant::now()
                .checked_sub(REFRESH_EVERY)
                .unwrap_or_else(Instant::now);
        }
        Err(err) => app.error = err.to_string(),
    }
}

fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Min(5),
            Constraint::Length(6),
            Constraint::Length(3),
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], app);
    draw_vms(frame, chunks[1], app);
    draw_detail(frame, chunks[2], app);
    draw_help(frame, chunks[3], app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let live = app.node.as_ref().map(|node| &node.live);
    let cpu_pct = live.map(|sample| sample.cpu_pct).unwrap_or(0.0);
    let cpu = match live {
        Some(sample) => format!("{:.1}%", sample.cpu_pct),
        None => "—".into(),
    };
    let mem = live
        .map(|sample| format_used_total(sample.mem_used_bytes, sample.mem_total_bytes))
        .unwrap_or_else(|| "—".into());
    let disk = live
        .map(|sample| format_used_total(sample.disk_used_bytes, sample.disk_total_bytes))
        .unwrap_or_else(|| "—".into());
    let net = live
        .map(|sample| {
            format!(
                "rx {}  tx {}",
                format_rate(sample.net_rx_bps),
                format_rate(sample.net_tx_bps)
            )
        })
        .unwrap_or_else(|| "rx —  tx —".into());
    let running = app
        .vms
        .iter()
        .filter(|vm| vm.state == VmState::Running && !vm.template)
        .count();
    let guests = app.vms.iter().filter(|vm| !vm.template).count();
    let alloc = app
        .node
        .as_ref()
        .map(|node| {
            format!(
                "{} vCPU  {} MiB",
                node.allocated_vcpus, node.allocated_memory_mib
            )
        })
        .unwrap_or_else(|| "—".into());
    let hostname = app
        .host
        .as_ref()
        .map(|host| host.hostname.clone())
        .filter(|name| !name.is_empty())
        .or_else(|| app.node.as_ref().map(|node| node.name.clone()))
        .unwrap_or_else(|| "this node".into());
    let driver = app
        .host
        .as_ref()
        .map(|host| host.driver.to_string())
        .unwrap_or_else(|| "—".into());
    let storage = app
        .host
        .as_ref()
        .map(|host| host.storage_backend.to_string())
        .unwrap_or_else(|| "—".into());
    let kvm = match app.host.as_ref() {
        Some(host) if host.kvm => "kvm",
        Some(_) => "no kvm",
        None => "—",
    };
    let host_up = app
        .host
        .as_ref()
        .map(|host| format_duration(host.uptime_secs))
        .unwrap_or_else(|| "—".into());
    let daemon_up = app
        .host
        .as_ref()
        .map(|host| format_duration(host.daemon_uptime_secs))
        .unwrap_or_else(|| "—".into());
    let age = app.last_refresh.elapsed();
    let age_label = if app.refresh_job.is_some() {
        "syncing".to_string()
    } else if age.as_secs() == 0 {
        "live".to_string()
    } else {
        format!("{}s ago", age.as_secs())
    };
    let age_color = if age.as_secs() >= 5 && app.refresh_job.is_none() {
        Color::Yellow
    } else {
        Color::Green
    };
    let auth = if app.token.is_some() {
        Span::styled("api up", Style::default().fg(Color::Green))
    } else {
        Span::styled("api down", Style::default().fg(Color::Yellow))
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("CPU ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(meter(cpu_pct, 10), Style::default().fg(meter_color(cpu_pct))),
            Span::raw(format!(" {cpu}   ")),
            Span::raw(net),
        ]),
        Line::from(vec![
            Span::styled("Mem ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(mem),
            Span::raw("   "),
            Span::styled("Disk ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(disk),
            Span::raw("   "),
            Span::styled("Alloc ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(alloc),
        ]),
        Line::from(vec![
            Span::styled("Guests ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{running} running / {guests}")),
            Span::raw("   "),
            Span::styled("Node ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{driver}  {storage}  {kvm}")),
            Span::raw("   "),
            Span::styled("Up ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{host_up}  daemon {daemon_up}")),
        ]),
        Line::from(vec![
            Span::styled("UI ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&app.info.ui_url),
            Span::raw("  "),
            Span::styled("admin ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(&app.info.password, Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            auth,
            Span::raw("  "),
            Span::styled(age_label, Style::default().fg(age_color)),
        ]),
        Line::from(cluster_spans(app)),
    ];
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" [=] ", Style::default().fg(BRAND).add_modifier(Modifier::BOLD)),
            Span::raw(format!("Pertisk  {hostname}  v{VERSION} ")),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BRAND));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn cluster_spans(app: &App) -> Vec<Span<'static>> {
    let Some(cluster) = app.cluster.clone() else {
        return vec![Span::styled(
            "Cluster  waiting",
            Style::default().add_modifier(Modifier::BOLD),
        )];
    };
    let online = cluster.members.iter().filter(|member| member.online).count();
    let total = cluster.members.len();
    let quorum = if cluster.quorum { "quorum" } else { "no quorum" };
    let quorum_color = if cluster.quorum {
        Color::Green
    } else {
        Color::Red
    };
    let ha = if cluster.ha_armed { "HA armed" } else { "HA disarmed" };
    let fence = if cluster.fenced { "  fenced" } else { "" };
    let witness = if cluster.witness { "  witness" } else { "" };
    let mut spans = vec![
        Span::styled("Cluster ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(cluster.name),
        Span::raw("  "),
        Span::styled(quorum, Style::default().fg(quorum_color)),
        Span::raw(format!("  {ha}{fence}{witness}  {online}/{total} online")),
    ];
    let names: Vec<String> = cluster
        .members
        .iter()
        .map(|member| {
            let mark = if member.online { "+" } else { "-" };
            format!("{mark}{}", member.name)
        })
        .collect();
    if !names.is_empty() {
        spans.push(Span::raw(format!("  {}", names.join(" "))));
    }
    spans
}

fn draw_vms(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec!["ID", "NAME", "STATE", "CPU", "MEM", "vCPU", "HA", "GROUP"])
        .style(Style::default().fg(BRAND).add_modifier(Modifier::BOLD));
    let visible = area.height.saturating_sub(3) as usize;
    let start = table_window(app.vms.len(), app.selected, visible.max(1));
    let rows: Vec<Row> = app
        .vms
        .iter()
        .enumerate()
        .skip(start)
        .take(visible.max(1))
        .map(|(i, vm)| {
            let style = if i == app.selected {
                Style::default()
                    .bg(Color::Rgb(16, 42, 40))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let live = app.vm_live.get(&vm.id);
            let cpu = match live {
                Some(stats) if vm.state == VmState::Running => format!("{:.0}%", stats.cpu_pct),
                _ => "—".into(),
            };
            let mem = match live {
                Some(stats) if vm.state == VmState::Running && stats.mem_total > 0 => {
                    format_used_total(stats.mem_used, stats.mem_total)
                }
                _ => format!("{}M", vm.spec.memory_mib),
            };
            let state = if vm.template {
                "template".to_string()
            } else {
                vm.state.to_string()
            };
            let ha = if vm.spec.ha { "yes" } else { "—" };
            let group = vm
                .spec
                .group
                .as_deref()
                .filter(|group| !group.is_empty())
                .unwrap_or("—");
            Row::new(vec![
                Cell::from(vm.id.to_string()),
                Cell::from(vm.spec.name.clone()),
                Cell::from(Span::styled(state, Style::default().fg(state_color(vm.state)))),
                Cell::from(cpu),
                Cell::from(mem),
                Cell::from(vm.spec.vcpus.to_string()),
                Cell::from(ha),
                Cell::from(group.to_string()),
            ])
            .style(style)
        })
        .collect();
    let title = if app.vms.is_empty() {
        " Guests ".to_string()
    } else {
        format!(" Guests  {} ", app.vms.len())
    };
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Min(12),
            Constraint::Length(9),
            Constraint::Length(5),
            Constraint::Length(16),
            Constraint::Length(5),
            Constraint::Length(4),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(Block::default().title(title).borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn table_window(len: usize, selected: usize, visible: usize) -> usize {
    if len <= visible || visible == 0 {
        return 0;
    }
    let selected = selected.min(len - 1);
    if selected + 1 <= visible {
        0
    } else {
        (selected + 1 - visible).min(len - visible)
    }
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let lines = match app.vms.get(app.selected) {
        Some(vm) => guest_lines(app, vm),
        None => vec![Line::from("No guests on this node.")],
    };
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().title(" Selected ").borders(Borders::ALL)),
        area,
    );
}

fn guest_lines(app: &App, vm: &VmRecord) -> Vec<Line<'static>> {
    let node = node_name(app.cluster.as_ref(), vm.node_id);
    let group = vm
        .spec
        .group
        .as_deref()
        .filter(|group| !group.is_empty())
        .unwrap_or("none");
    let ha = if vm.spec.ha { "on" } else { "off" };
    let autostart = if vm.spec.autostart { "on" } else { "off" };
    let console = if vm.graphics_socket.is_some() {
        "display"
    } else if vm.console_socket.is_some() || vm.serial_log.is_some() {
        "serial"
    } else {
        "none"
    };
    let live = app.vm_live.get(&vm.id);
    let cpu = live
        .filter(|_| vm.state == VmState::Running)
        .map(|stats| format!("{:.1}%", stats.cpu_pct))
        .unwrap_or_else(|| "—".into());
    let net = live
        .filter(|_| vm.state == VmState::Running)
        .map(|stats| {
            format!(
                "rx {}  tx {}",
                format_rate(stats.net_rx_bps),
                format_rate(stats.net_tx_bps)
            )
        })
        .unwrap_or_else(|| "rx —  tx —".into());
    let disk = live
        .filter(|stats| vm.state == VmState::Running && stats.disk_total > 0)
        .map(|stats| format_used_total(stats.disk_used, stats.disk_total))
        .unwrap_or_else(|| "—".into());
    let disks = vm.spec.disks.len();
    let cdroms = vm.spec.disks.iter().filter(|disk| disk.cdrom).count();
    let ips: Vec<String> = vm
        .spec
        .nets
        .iter()
        .filter_map(|nic| nic.ip.clone().or_else(|| nic.ipv6.clone()))
        .collect();
    let ip_text = if ips.is_empty() {
        "no address".to_string()
    } else {
        ips.join(", ")
    };
    let backup = match &vm.spec.backup {
        Some(schedule) if schedule.enabled => format!(
            "every {}h  keep {}",
            schedule.interval_hours, schedule.keep
        ),
        Some(_) => "off".into(),
        None => "not set".into(),
    };
    let snaps = match &vm.spec.snapshot {
        Some(schedule) if schedule.enabled => format!(
            "every {}h keep {}",
            schedule.interval_hours, schedule.keep
        ),
        Some(_) => "off".into(),
        None => "not set".into(),
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{}  {}  ", vm.id, vm.spec.name),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                vm.state.to_string(),
                Style::default().fg(state_color(vm.state)),
            ),
            Span::raw(format!(
                "  {} vCPU  {} MiB  cpu {cpu}  {net}",
                vm.spec.vcpus, vm.spec.memory_mib
            )),
        ]),
        Line::from(format!(
            "node {node}   HA {ha}   group {group}   autostart {autostart}   console {console}"
        )),
        Line::from(format!(
            "disks {disks} ({cdroms} cd)  {disk}   nics {}  {ip_text}   backup {backup}   snapshots {snaps}",
            vm.spec.nets.len()
        )),
    ];
    if let Some(err) = vm.last_error.as_ref().filter(|err| !err.is_empty()) {
        let text = if err.chars().count() > 90 {
            let end = err
                .char_indices()
                .nth(90)
                .map(|(idx, _)| idx)
                .unwrap_or(err.len());
            format!("error  {}…", &err[..end])
        } else {
            format!("error  {err}")
        };
        lines.push(Line::from(Span::styled(
            text,
            Style::default().fg(Color::Red),
        )));
    }
    lines
}

fn draw_help(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = if app.pending_delete.is_some() {
        vec![Span::styled(
            "CONFIRM DELETE  d again    Esc cancel",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]
    } else {
        vec![
            Span::raw("j/k move   "),
            Span::styled("s", Style::default().fg(Color::Green)),
            Span::raw(" start   "),
            Span::styled("h", Style::default().fg(Color::Yellow)),
            Span::raw(" shutdown   "),
            Span::styled("b", Style::default().fg(Color::Cyan)),
            Span::raw(" restart   "),
            Span::styled("x", Style::default().fg(Color::Red)),
            Span::raw(" stop   "),
            Span::styled(
                "d",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" delete   r refresh   q quit"),
        ]
    };
    if !app.status.is_empty() {
        spans.push(Span::raw("   "));
        let color = if app.pending_delete.is_some() {
            Color::Yellow
        } else {
            Color::Green
        };
        spans.push(Span::styled(app.status.clone(), Style::default().fg(color)));
    }
    if !app.error.is_empty() {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            app.error.clone(),
            Style::default().fg(Color::Red),
        ));
    }
    frame.render_widget(
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
    fn format_bytes_gib() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(32554504192), "30 GiB");
    }

    #[test]
    fn format_used_total_pair() {
        assert_eq!(format_used_total(0, 0), "—");
        assert_eq!(
            format_used_total(644_245_094, 32_212_254_720),
            format!(
                "{}/{}",
                format_bytes(644_245_094),
                format_bytes(32_212_254_720)
            )
        );
    }

    #[test]
    fn meter_and_rates() {
        assert_eq!(meter(0.0, 4), "[----]");
        assert_eq!(meter(100.0, 4), "[####]");
        assert_eq!(meter(50.0, 4), "[##--]");
        assert_eq!(format_rate(0), "0");
        assert_eq!(format_rate(2048), format!("{}/s", format_bytes(2048)));
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(90), "1m");
        assert_eq!(format_duration(3_700), "1h 1m");
        assert_eq!(format_duration(90_000), "1d 1h");
    }

    #[test]
    fn table_window_keeps_selection_visible() {
        assert_eq!(table_window(3, 2, 10), 0);
        assert_eq!(table_window(20, 0, 5), 0);
        assert_eq!(table_window(20, 7, 5), 3);
        assert_eq!(table_window(20, 19, 5), 15);
    }
}
