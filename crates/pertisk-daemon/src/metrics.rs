//! Live host / guest resource sampling (CPU, memory, disk, network).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use pertisk_types::{ResourceSample, VmRecord, VmState, VolumeRecord};

#[derive(Debug, Default)]
struct Counters {
    /// CPU: busy ticks (or process utime+stime), and total ticks (host only).
    cpu_busy: u64,
    cpu_total: u64,
    net_rx: u64,
    net_tx: u64,
    at_ms: u64,
}

#[derive(Debug, Default)]
pub struct MetricsCache {
    prev: Mutex<HashMap<String, Counters>>,
}

impl MetricsCache {
    pub fn new() -> Self {
        Self::default()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn host_cores() -> u64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(1)
        .max(1)
}

/// Sample the hypervisor host.
pub fn sample_host(cache: &MetricsCache, storage_root: &Path) -> ResourceSample {
    let at = now_ms();
    let (cpu_busy, cpu_total) = read_host_cpu().unwrap_or((0, 1));
    let (mem_used, mem_total) = read_meminfo().unwrap_or((0, 0));
    let (disk_used, disk_total) = read_disk(storage_root).unwrap_or((0, 0));
    let (net_rx, net_tx) = read_host_net().unwrap_or((0, 0));

    let mut prev = cache.prev.lock().unwrap_or_else(|e| e.into_inner());
    let key = "host".to_string();
    let (cpu_pct, rx_bps, tx_bps) = rate_from_prev(
        prev.get(&key),
        Counters {
            cpu_busy,
            cpu_total,
            net_rx,
            net_tx,
            at_ms: at,
        },
        true,
    );
    prev.insert(
        key,
        Counters {
            cpu_busy,
            cpu_total,
            net_rx,
            net_tx,
            at_ms: at,
        },
    );

    ResourceSample {
        cpu_pct,
        mem_used_bytes: mem_used,
        mem_total_bytes: mem_total,
        disk_used_bytes: disk_used,
        disk_total_bytes: disk_total,
        net_rx_bps: rx_bps,
        net_tx_bps: tx_bps,
        collected_at_ms: at,
    }
}

/// Sample a running guest via QEMU pid + TAP counters + disk file sizes.
pub fn sample_vm(
    cache: &MetricsCache,
    vm: &VmRecord,
    volumes: &[VolumeRecord],
) -> Option<ResourceSample> {
    if vm.state != VmState::Running {
        return None;
    }
    let pid = vm.pid?;
    let at = now_ms();
    let cpu_busy = read_proc_cpu(pid).unwrap_or(0);
    let mem_used = read_proc_rss(pid).unwrap_or(0);
    let mem_total = u64::from(vm.spec.memory_mib).saturating_mul(1024 * 1024);
    let (disk_used, disk_total) = vm_disk_bytes(vm, volumes);
    let taps: Vec<&str> = vm
        .spec
        .nets
        .iter()
        .filter_map(|n| n.tap.as_deref())
        .collect();
    let (net_rx, net_tx) = read_taps_net(&taps).unwrap_or((0, 0));

    let mut prev = cache.prev.lock().unwrap_or_else(|e| e.into_inner());
    let key = format!("vm:{}", vm.id);
    let cores = host_cores();
    let (cpu_pct, rx_bps, tx_bps) = {
        let cur = Counters {
            cpu_busy,
            cpu_total: cores, // marker; process CPU uses busy delta / elapsed / cores
            net_rx,
            net_tx,
            at_ms: at,
        };
        rate_from_prev_process(prev.get(&key), &cur, cores)
    };
    prev.insert(
        key,
        Counters {
            cpu_busy,
            cpu_total: cores,
            net_rx,
            net_tx,
            at_ms: at,
        },
    );

    Some(ResourceSample {
        cpu_pct,
        mem_used_bytes: mem_used.min(mem_total.max(mem_used)),
        mem_total_bytes: mem_total.max(1),
        disk_used_bytes: disk_used,
        disk_total_bytes: disk_total.max(1),
        net_rx_bps: rx_bps,
        net_tx_bps: tx_bps,
        collected_at_ms: at,
    })
}

fn rate_from_prev(prev: Option<&Counters>, cur: Counters, host_cpu: bool) -> (f32, u64, u64) {
    let Some(p) = prev else {
        return (0.0, 0, 0);
    };
    if cur.at_ms <= p.at_ms {
        return (0.0, 0, 0);
    }
    let dt_ms = cur.at_ms - p.at_ms;
    let dt_s = (dt_ms as f64 / 1000.0).max(0.001);

    let cpu_pct = if host_cpu {
        let dbusy = cur.cpu_busy.saturating_sub(p.cpu_busy) as f64;
        let dtotal = cur.cpu_total.saturating_sub(p.cpu_total) as f64;
        if dtotal > 0.0 {
            ((dbusy / dtotal) * 100.0).clamp(0.0, 100.0) as f32
        } else {
            0.0
        }
    } else {
        0.0
    };

    let rx = ((cur.net_rx.saturating_sub(p.net_rx) as f64) / dt_s).max(0.0) as u64;
    let tx = ((cur.net_tx.saturating_sub(p.net_tx) as f64) / dt_s).max(0.0) as u64;
    (cpu_pct, rx, tx)
}

fn rate_from_prev_process(prev: Option<&Counters>, cur: &Counters, cores: u64) -> (f32, u64, u64) {
    let Some(p) = prev else {
        return (0.0, 0, 0);
    };
    if cur.at_ms <= p.at_ms {
        return (0.0, 0, 0);
    }
    let dt_ms = cur.at_ms - p.at_ms;
    let dt_s = (dt_ms as f64 / 1000.0).max(0.001);
    // Process jiffies (typically 100 Hz). Approximate: busy_delta / (HZ * elapsed * cores) * 100
    let hz = 100.0_f64;
    let dbusy = cur.cpu_busy.saturating_sub(p.cpu_busy) as f64;
    let cpu_pct = ((dbusy / (hz * dt_s * cores as f64)) * 100.0).clamp(0.0, 100.0) as f32;
    let rx = ((cur.net_rx.saturating_sub(p.net_rx) as f64) / dt_s).max(0.0) as u64;
    let tx = ((cur.net_tx.saturating_sub(p.net_tx) as f64) / dt_s).max(0.0) as u64;
    (cpu_pct, rx, tx)
}

fn read_host_cpu() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    let line = text.lines().next()?;
    if !line.starts_with("cpu ") {
        return None;
    }
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|p| p.parse().ok())
        .collect();
    if parts.len() < 4 {
        return None;
    }
    // user nice system idle iowait irq softirq steal...
    let idle = parts[3] + parts.get(4).copied().unwrap_or(0);
    let total: u64 = parts.iter().sum();
    let busy = total.saturating_sub(idle);
    Some((busy, total.max(1)))
}

fn read_meminfo() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let key = parts.next()?;
        let val: u64 = parts.next()?.parse().ok()?;
        match key {
            "MemTotal:" => total_kb = val,
            "MemAvailable:" => avail_kb = val,
            _ => {}
        }
    }
    if total_kb == 0 {
        return None;
    }
    let used = total_kb.saturating_sub(avail_kb).saturating_mul(1024);
    Some((used, total_kb.saturating_mul(1024)))
}

fn read_disk(root: &Path) -> Option<(u64, u64)> {
    let probe = if root.exists() {
        root.to_path_buf()
    } else {
        root.parent().unwrap_or(Path::new("/")).to_path_buf()
    };
    let out = std::process::Command::new("df")
        .args(["-Pk", probe.to_str()?])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let mut cols = line.split_whitespace();
    let _fs = cols.next()?;
    let total_kb: u64 = cols.next()?.parse().ok()?;
    let used_kb: u64 = cols.next()?.parse().ok()?;
    Some((
        used_kb.saturating_mul(1024),
        total_kb.saturating_mul(1024).max(1),
    ))
}

fn read_host_net() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/net/dev").ok()?;
    let mut prefer_br0 = None;
    let mut sum_rx = 0u64;
    let mut sum_tx = 0u64;
    for line in text.lines().skip(2) {
        let line = line.trim();
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name == "lo" {
            continue;
        }
        let fields: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect();
        if fields.len() < 9 {
            continue;
        }
        let rx = fields[0];
        let tx = fields[8];
        if name == "br0" {
            prefer_br0 = Some((rx, tx));
        }
        // Skip virtual taps belonging to guests (pNNNN)
        if name.starts_with('p') && name[1..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if name.starts_with("tap") || name.starts_with("veth") {
            continue;
        }
        sum_rx = sum_rx.saturating_add(rx);
        sum_tx = sum_tx.saturating_add(tx);
    }
    Some(prefer_br0.unwrap_or((sum_rx, sum_tx)))
}

fn read_taps_net(taps: &[&str]) -> Option<(u64, u64)> {
    if taps.is_empty() {
        return Some((0, 0));
    }
    let text = std::fs::read_to_string("/proc/net/dev").ok()?;
    let mut rx = 0u64;
    let mut tx = 0u64;
    for line in text.lines().skip(2) {
        let line = line.trim();
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if !taps.iter().any(|t| *t == name) {
            continue;
        }
        let fields: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect();
        if fields.len() < 9 {
            continue;
        }
        rx = rx.saturating_add(fields[0]);
        tx = tx.saturating_add(fields[8]);
    }
    Some((rx, tx))
}

fn read_proc_cpu(pid: u32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // After comm (in parens), fields: state ppid ... utime(14) stime(15) — 1-indexed from start of file is messy.
    let after = text.rfind(')').map(|i| &text[i + 2..])?;
    let parts: Vec<&str> = after.split_whitespace().collect();
    // utime is field 14 of full stat = index 11 after comm (fields 3..)
    let utime: u64 = parts.get(11)?.parse().ok()?;
    let stime: u64 = parts.get(12)?.parse().ok()?;
    Some(utime.saturating_add(stime))
}

fn read_proc_rss(pid: u32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

fn vm_disk_bytes(vm: &VmRecord, volumes: &[VolumeRecord]) -> (u64, u64) {
    let mut used = 0u64;
    let mut total = 0u64;
    for disk in &vm.spec.disks {
        if disk.cdrom {
            continue;
        }
        if let Some(id) = disk.volume_id {
            if let Some(vol) = volumes.iter().find(|v| v.id == id) {
                total = total.saturating_add(vol.size_bytes);
                let on_disk = std::fs::metadata(&vol.path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                used = used.saturating_add(on_disk);
                continue;
            }
        }
        let on_disk = std::fs::metadata(&disk.path).map(|m| m.len()).unwrap_or(0);
        used = used.saturating_add(on_disk);
        total = total.saturating_add(on_disk);
    }
    (used, total.max(used).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_first_sample_zero() {
        let (cpu, rx, tx) = rate_from_prev(
            None,
            Counters {
                cpu_busy: 10,
                cpu_total: 100,
                net_rx: 1000,
                net_tx: 2000,
                at_ms: 1000,
            },
            true,
        );
        assert_eq!(cpu, 0.0);
        assert_eq!(rx, 0);
        assert_eq!(tx, 0);
    }
}
