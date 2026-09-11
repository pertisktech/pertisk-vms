//! Host apt updates and repositories (Proxmox-style, in-place — not a reflash).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pertisk_types::{
    AddRepositoryRequest, AptActionResult, AptRepository, SetRepositoryRequest, UpdatePackage,
    UpdatesStatus,
};

pub fn list_updates() -> Result<UpdatesStatus, String> {
    if find_apt().is_none() {
        return Ok(UpdatesStatus {
            apt: false,
            reason: Some("apt-get not found on this node".into()),
            packages: Vec::new(),
            reboot_required: reboot_required(),
        });
    }
    let output = match run_apt(&[
        "-s",
        "-o",
        "Debug::NoLocking=true",
        "-o",
        "APT::Get::Show-User-Simulation-Note=false",
        "dist-upgrade",
    ]) {
        Ok(log) => log,
        Err(err) => {
            return Ok(UpdatesStatus {
                apt: true,
                reason: Some(err),
                packages: Vec::new(),
                reboot_required: reboot_required(),
            });
        }
    };
    Ok(UpdatesStatus {
        apt: true,
        reason: None,
        packages: parse_inst_lines(&output),
        reboot_required: reboot_required(),
    })
}

pub fn refresh() -> Result<AptActionResult, String> {
    let log = run_apt(&["update"])?;
    Ok(AptActionResult {
        ok: true,
        log,
        reboot_required: reboot_required(),
    })
}

pub fn upgrade() -> Result<AptActionResult, String> {
    let log = run_apt(&[
        "-y",
        "-o",
        "Dpkg::Options::=--force-confold",
        "-o",
        "Dpkg::Options::=--force-confdef",
        "dist-upgrade",
    ])?;
    Ok(AptActionResult {
        ok: true,
        log,
        reboot_required: reboot_required(),
    })
}

pub fn list_repos() -> Result<Vec<AptRepository>, String> {
    list_repos_at(&apt_root())
}

pub fn add_repo(req: AddRepositoryRequest) -> Result<AptRepository, String> {
    add_repo_at(&apt_root(), req)
}

pub fn set_repo(req: SetRepositoryRequest) -> Result<AptRepository, String> {
    set_repo_at(&apt_root(), req)
}

fn list_repos_at(root: &Path) -> Result<Vec<AptRepository>, String> {
    let mut repos = Vec::new();
    let list = root.join("etc/apt/sources.list");
    if list.is_file() {
        parse_list_file(root, &list, &mut repos)?;
    }
    let dir = root.join("etc/apt/sources.list.d");
    if dir.is_dir() {
        let mut files: Vec<_> = fs::read_dir(&dir)
            .map_err(|err| format!("read {}: {err}", dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e == "list" || e == "sources")
            })
            .collect();
        files.sort();
        for path in files {
            if path.extension().and_then(|e| e.to_str()) == Some("sources") {
                parse_deb822_file(root, &path, &mut repos)?;
            } else {
                parse_list_file(root, &path, &mut repos)?;
            }
        }
    }
    Ok(repos)
}

fn add_repo_at(root: &Path, req: AddRepositoryRequest) -> Result<AptRepository, String> {
    let name = sanitize_repo_name(&req.name)?;
    let uri = req.uri.trim();
    let suite = req.suite.trim();
    let components = req.components.trim();
    if uri.is_empty() || suite.is_empty() {
        return Err("uri and suite are required".into());
    }
    let dir = root.join("etc/apt/sources.list.d");
    fs::create_dir_all(&dir).map_err(|err| format!("create {}: {err}", dir.display()))?;
    let path = dir.join(format!("{name}.list"));
    if path.exists() {
        return Err(format!(
            "repository file already exists: {}",
            path.display()
        ));
    }
    let components = if components.is_empty() {
        "main"
    } else {
        components
    };
    let body = format!("deb {uri} {suite} {components}\n");
    fs::write(&path, body).map_err(|err| format!("write {}: {err}", path.display()))?;
    list_repos_at(root)?
        .into_iter()
        .find(|r| r.file.ends_with(&format!("{name}.list")))
        .ok_or_else(|| "added repository but failed to read it back".into())
}

fn set_repo_at(root: &Path, req: SetRepositoryRequest) -> Result<AptRepository, String> {
    let mut repos = list_repos_at(root)?;
    let Some(idx) = repos.iter().position(|r| r.id == req.id) else {
        return Err(format!("repository not found: {}", req.id));
    };
    if repos[idx].enabled == req.enabled {
        return Ok(repos.remove(idx));
    }
    let file = PathBuf::from(&repos[idx].file);
    let abs = if file.is_absolute() {
        file
    } else {
        root.join(file)
    };
    let text = fs::read_to_string(&abs).map_err(|err| format!("read {}: {err}", abs.display()))?;
    let updated = if abs.extension().and_then(|e| e.to_str()) == Some("sources") {
        set_deb822_enabled(&text, &repos[idx], req.enabled)?
    } else {
        set_list_enabled(&text, &repos[idx], req.enabled)?
    };
    fs::write(&abs, updated).map_err(|err| format!("write {}: {err}", abs.display()))?;
    list_repos_at(root)?
        .into_iter()
        .find(|r| r.id == req.id)
        .ok_or_else(|| "updated repository but failed to read it back".into())
}

fn apt_root() -> PathBuf {
    std::env::var("PERTISK_APT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

fn find_apt() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &["/usr/bin/apt-get", "/bin/apt-get", "/usr/local/bin/apt-get"];
    for path in CANDIDATES {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    pertisk_types::find_in_path("apt-get")
}

fn reboot_required() -> bool {
    apt_root().join("var/run/reboot-required").is_file()
}

fn run_apt(args: &[&str]) -> Result<String, String> {
    ensure_apt_dns();
    let bin = find_apt().ok_or_else(|| "apt-get not found on this node".to_string())?;
    let mut cmd = if Path::new("/usr/bin/timeout").is_file() {
        let mut wrapped = Command::new("/usr/bin/timeout");
        wrapped.args(["-k", "10", "120"]);
        wrapped.arg(&bin);
        wrapped
    } else {
        Command::new(&bin)
    };
    cmd.args([
        "-o",
        "Acquire::ForceIPv4=true",
        "-o",
        "Acquire::Retries=1",
        "-o",
        "Acquire::http::Timeout=20",
        "-o",
        "Acquire::https::Timeout=20",
        "-o",
        "Acquire::Languages=none",
        "-o",
        "Acquire::PDiffs=false",
        "-o",
        "APT::Color=0",
    ]);
    cmd.args(args);
    cmd.env("DEBIAN_FRONTEND", "noninteractive");
    cmd.env("LC_ALL", "C");
    cmd.env("TERM", "dumb");
    cmd.env(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    let output = cmd.output().map_err(|err| format!("run apt-get: {err}"))?;
    let mut log = tidy_apt_log(&output.stdout, &output.stderr);
    if log.len() > 64 * 1024 {
        let keep = 64 * 1024;
        log = format!("…\n{}", &log[log.len() - keep..]);
    }
    if output.status.code() == Some(124) {
        return Err("apt-get timed out after 120s (mirror or DNS too slow)".into());
    }
    if apt_dns_failed(&log) {
        return Err(
            "No DNS: cannot reach deb.debian.org. Plug the LAN in, wait for DHCP, then Refresh again."
                .into(),
        );
    }
    if !output.status.success() {
        return Err(if log.trim().is_empty() {
            format!("apt-get failed ({})", output.status)
        } else {
            log
        });
    }
    if args.iter().any(|a| *a == "update") && !log.contains("Finished.") {
        if !log.is_empty() && !log.ends_with('\n') {
            log.push('\n');
        }
        log.push_str("Finished.\n");
    }
    Ok(log)
}

fn tidy_apt_log(stdout: &[u8], stderr: &[u8]) -> String {
    let mut raw = String::from_utf8_lossy(stdout).into_owned();
    if !stderr.is_empty() {
        if !raw.is_empty() && !raw.ends_with('\n') {
            raw.push('\n');
        }
        raw.push_str(&String::from_utf8_lossy(stderr));
    }
    raw.replace('\r', "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn apt_dns_failed(log: &str) -> bool {
    log.contains("Temporary failure resolving")
        || log.contains("Could not resolve '")
        || log.contains("Failed to resolve")
}

fn host_resolves(name: &str) -> bool {
    Command::new("getent")
        .args(["hosts", name])
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn ensure_apt_dns() {
    if host_resolves("deb.debian.org") {
        return;
    }
    let _ = Command::new("/usr/sbin/pertisk-fix-dns").status();
    if host_resolves("deb.debian.org") {
        return;
    }
    let path = Path::new("/etc/resolv.conf");
    let text = fs::read_to_string(path).unwrap_or_default();
    let stub = text.contains("127.0.0.53") || path.is_symlink() || !text.contains("nameserver");
    if stub {
        if path.is_symlink() {
            let _ = fs::remove_file(path);
        }
        let _ = fs::write(path, "nameserver 1.1.1.1\nnameserver 8.8.8.8\n");
    }
}

pub(crate) fn parse_inst_lines(text: &str) -> Vec<UpdatePackage> {
    let mut packages = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Inst ") else {
            continue;
        };
        let Some(pkg) = rest.split_whitespace().next() else {
            continue;
        };
        let mut version = String::new();
        if let Some(start) = rest.find('[')
            && let Some(end) = rest[start + 1..].find(']')
        {
            version = rest[start + 1..start + 1 + end].trim().to_string();
        }
        let mut available = String::new();
        let mut origin = String::new();
        let mut arch = String::new();
        if let Some(start) = rest.find('(')
            && let Some(end) = rest.rfind(')')
            && end > start
        {
            let inside = rest[start + 1..end].trim();
            let mut parts = inside.split_whitespace();
            available = parts.next().unwrap_or("").to_string();
            let rest_origin: Vec<&str> = parts.collect();
            if let Some(last) = rest_origin.last()
                && last.starts_with('[')
                && last.ends_with(']')
            {
                arch = last.trim_matches(['[', ']']).to_string();
                origin = rest_origin[..rest_origin.len() - 1].join(" ");
            } else {
                origin = rest_origin.join(" ");
            }
        }
        packages.push(UpdatePackage {
            name: pkg.to_string(),
            version,
            available,
            arch,
            origin,
        });
    }
    packages
}

fn rel_file(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn parse_list_file(root: &Path, path: &Path, out: &mut Vec<AptRepository>) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let file = rel_file(root, path);
    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (enabled, body) = if let Some(rest) = trimmed.strip_prefix('#') {
            let rest = rest.trim();
            if rest.starts_with("deb") {
                (false, rest)
            } else {
                continue;
            }
        } else {
            (true, trimmed)
        };
        let Some(repo) = parse_deb_line(body) else {
            continue;
        };
        out.push(AptRepository {
            id: format!("{file}:{i}"),
            enabled,
            file: file.clone(),
            types: repo.0,
            uri: repo.1,
            suite: repo.2,
            components: repo.3,
        });
    }
    Ok(())
}

/// `deb [options] URI SUITE COMPONENTS…`
fn parse_deb_line(line: &str) -> Option<(String, String, String, String)> {
    let mut rest = line.trim();
    let types = if rest.starts_with("deb-src") {
        rest = rest["deb-src".len()..].trim();
        "deb-src"
    } else if rest.starts_with("deb") {
        rest = rest[3..].trim();
        "deb"
    } else {
        return None;
    };
    if rest.starts_with('[') {
        let end = rest.find(']')?;
        rest = rest[end + 1..].trim();
    }
    let mut parts = rest.split_whitespace();
    let uri = parts.next()?.to_string();
    let suite = parts.next()?.to_string();
    let components = parts.collect::<Vec<_>>().join(" ");
    Some((types.to_string(), uri, suite, components))
}

fn parse_deb822_file(root: &Path, path: &Path, out: &mut Vec<AptRepository>) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let file = rel_file(root, path);
    for (i, stanza) in split_stanzas(&text).into_iter().enumerate() {
        let fields = parse_deb822_fields(&stanza);
        let types = fields.get("Types").cloned().unwrap_or_default();
        if !types
            .split_whitespace()
            .any(|t| t == "deb" || t == "deb-src")
        {
            continue;
        }
        let enabled = match fields.get("Enabled").map(|s| s.to_ascii_lowercase()) {
            Some(v) if v == "no" || v == "false" => false,
            _ => true,
        };
        let uri = fields.get("URIs").cloned().unwrap_or_default();
        let suite = fields.get("Suites").cloned().unwrap_or_default();
        let components = fields.get("Components").cloned().unwrap_or_default();
        let kind = if types.split_whitespace().any(|t| t == "deb") {
            "deb"
        } else {
            "deb-src"
        };
        out.push(AptRepository {
            id: format!("{file}:{i}"),
            enabled,
            file: file.clone(),
            types: kind.into(),
            uri,
            suite,
            components,
        });
    }
    Ok(())
}

fn split_stanzas(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !cur.trim().is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

fn parse_deb822_fields(stanza: &str) -> std::collections::BTreeMap<String, String> {
    let mut fields = std::collections::BTreeMap::new();
    let mut key = String::new();
    let mut val = String::new();
    for line in stanza.lines() {
        if let Some(rest) = line.strip_prefix(' ')
            && !key.is_empty()
        {
            if !val.is_empty() {
                val.push(' ');
            }
            val.push_str(rest.trim());
            continue;
        }
        if !key.is_empty() {
            fields.insert(std::mem::take(&mut key), std::mem::take(&mut val));
        }
        if let Some((k, v)) = line.split_once(':') {
            key = k.trim().to_string();
            val = v.trim().to_string();
        }
    }
    if !key.is_empty() {
        fields.insert(key, val);
    }
    fields
}

fn set_list_enabled(text: &str, repo: &AptRepository, enabled: bool) -> Result<String, String> {
    let mut found = false;
    let mut out = String::new();
    for (i, raw) in text.lines().enumerate() {
        let id = format!("{}:{i}", repo.file);
        if id != repo.id {
            out.push_str(raw);
            out.push('\n');
            continue;
        }
        found = true;
        let trimmed = raw.trim();
        let body = trimmed.strip_prefix('#').map(str::trim).unwrap_or(trimmed);
        if enabled {
            out.push_str(body);
        } else if trimmed.starts_with('#') {
            out.push_str(raw);
        } else {
            out.push_str("# ");
            out.push_str(body);
        }
        out.push('\n');
    }
    if !found {
        return Err("repository line not found in file".into());
    }
    Ok(out)
}

fn set_deb822_enabled(text: &str, repo: &AptRepository, enabled: bool) -> Result<String, String> {
    let want = repo
        .id
        .rsplit_once(':')
        .and_then(|(_, n)| n.parse::<usize>().ok());
    let Some(want) = want else {
        return Err("invalid repository id".into());
    };
    let stanzas = split_stanzas(text);
    if want >= stanzas.len() {
        return Err("repository stanza not found".into());
    }
    let mut rebuilt = Vec::new();
    for (i, stanza) in stanzas.into_iter().enumerate() {
        if i != want {
            rebuilt.push(stanza);
            continue;
        }
        rebuilt.push(rewrite_enabled_field(&stanza, enabled));
    }
    let mut out = rebuilt.join("\n\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn rewrite_enabled_field(stanza: &str, enabled: bool) -> String {
    let value = if enabled { "yes" } else { "no" };
    let mut seen = false;
    let mut lines: Vec<String> = Vec::new();
    for line in stanza.lines() {
        if line.to_ascii_lowercase().starts_with("enabled:") {
            lines.push(format!("Enabled: {value}"));
            seen = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !seen {
        lines.push(format!("Enabled: {value}"));
    }
    lines.join("\n")
}

fn sanitize_repo_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is required".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        || name.starts_with('.')
    {
        return Err("name may only contain letters, digits, '.', '-' and '_'".into());
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apt_inst_lines() {
        let text = "\
Reading package lists...
Inst linux-image-amd64 [6.12.12-1] (6.12.22-1 Debian:13/stable [amd64])
Inst qemu-system-x86 [1:9.2.0] (1:10.0.0 Debian:13/stable [amd64])
Conf linux-image-amd64 (6.12.22-1 Debian:13/stable [amd64])
";
        let pkgs = parse_inst_lines(text);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "linux-image-amd64");
        assert_eq!(pkgs[0].version, "6.12.12-1");
        assert_eq!(pkgs[0].available, "6.12.22-1");
        assert_eq!(pkgs[0].arch, "amd64");
        assert_eq!(pkgs[0].origin, "Debian:13/stable");
        assert_eq!(pkgs[1].name, "qemu-system-x86");
    }

    #[test]
    fn detects_apt_dns_failure() {
        let log = "\
Ign:1 http://deb.debian.org/debian trixie InRelease
Err:1 http://deb.debian.org/debian trixie InRelease
  Temporary failure resolving 'deb.debian.org'
W: Failed to fetch http://deb.debian.org/debian/dists/trixie/InRelease  Temporary failure resolving 'deb.debian.org'
";
        assert!(apt_dns_failed(log));
        assert!(!apt_dns_failed(
            "Hit:1 http://deb.debian.org/debian trixie InRelease\n"
        ));
    }

    #[test]
    fn tidy_apt_progress_overwrites() {
        let log = tidy_apt_log(
            b"Hit:1 http://deb.debian.org/debian trixie InRelease\n",
            b"Reading package lists...\rReading package lists... Done\n",
        );
        assert!(log.contains("Hit:1"));
        assert!(log.contains("Reading package lists... Done"));
        assert!(!log.contains('\r'));
    }

    #[test]
    fn list_and_toggle_deb822_repo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let apt = root.join("etc/apt/sources.list.d");
        fs::create_dir_all(&apt).unwrap();
        fs::write(
            apt.join("debian.sources"),
            "Types: deb\n\
             URIs: http://deb.debian.org/debian\n\
             Suites: trixie trixie-updates\n\
             Components: main contrib\n\
             Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg\n",
        )
        .unwrap();
        let repos = list_repos_at(root).unwrap();
        assert_eq!(repos.len(), 1);
        assert!(repos[0].enabled);
        assert_eq!(repos[0].uri, "http://deb.debian.org/debian");
        assert_eq!(repos[0].suite, "trixie trixie-updates");
        let id = repos[0].id.clone();
        let disabled = set_repo_at(
            root,
            SetRepositoryRequest {
                id: id.clone(),
                enabled: false,
            },
        )
        .unwrap();
        assert!(!disabled.enabled);
        let text = fs::read_to_string(apt.join("debian.sources")).unwrap();
        assert!(text.contains("Enabled: no"));
    }

    #[test]
    fn add_and_disable_list_repo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let added = add_repo_at(
            root,
            AddRepositoryRequest {
                name: "debian-security".into(),
                uri: "http://security.debian.org/debian-security".into(),
                suite: "trixie-security".into(),
                components: "main contrib".into(),
            },
        )
        .unwrap();
        assert!(added.enabled);
        assert_eq!(added.suite, "trixie-security");
        let disabled = set_repo_at(
            root,
            SetRepositoryRequest {
                id: added.id.clone(),
                enabled: false,
            },
        )
        .unwrap();
        assert!(!disabled.enabled);
        let text =
            fs::read_to_string(root.join("etc/apt/sources.list.d/debian-security.list")).unwrap();
        assert!(text.trim_start().starts_with("# deb "));
    }
}
