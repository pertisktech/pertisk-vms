//! Write hostname / SSH login onto a cloned guest disk so AlmaLinux still works
//! when cloud-init finishes as DataSourceNone.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{Result, StorageError};

pub struct GuestIdentity<'a> {
    pub hostname: &'a str,
    pub user: &'a str,
    pub password: Option<&'a str>,
    pub ssh_authorized_keys: &'a [String],
}

/// No-op for tiny test images. On a real cloud disk, mounts the root FS and
/// writes hostname, sshd password auth, optional password/keys.
pub fn inject_guest_identity(disk: &Path, id: &GuestIdentity<'_>) -> Result<()> {
    if disk.metadata()?.len() < 64 * 1024 * 1024 {
        return Ok(());
    }
    let extra = operator_ssh_keys();
    let mut keys = Vec::new();
    for key in id.ssh_authorized_keys.iter().chain(extra.iter()) {
        let key = key.trim();
        if looks_like_ssh_key(key) && !keys.iter().any(|k: &String| k == key) {
            keys.push(key.to_string());
        }
    }
    let id = GuestIdentity {
        hostname: id.hostname,
        user: id.user,
        password: id.password,
        ssh_authorized_keys: &keys,
    };
    let loopdev = losetup(disk)?;
    let result = inject_on_loop(&loopdev, &id);
    let _ = Command::new("losetup").args(["-d", &loopdev]).status();
    result
}

/// Operator keys cloned into every cloud guest (same as a standard cloud VM).
pub fn operator_ssh_keys() -> Vec<String> {
    parse_ssh_key_file(Path::new("/etc/pertisk/ssh/authorized_keys"))
}

pub fn parse_ssh_key_file(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_ssh_keys(&text)
}

pub fn parse_ssh_keys(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if looks_like_ssh_key(line) && !out.iter().any(|k: &String| k == line) {
            out.push(line.to_string());
        }
    }
    out
}

pub fn looks_like_ssh_key(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty()
        && !line.starts_with('#')
        && (line.starts_with("ssh-")
            || line.starts_with("ecdsa-")
            || line.starts_with("sk-ssh-")
            || line.starts_with("sk-ecdsa-"))
}

/// Days since 1970-01-01. Shadow `lastchg=0` means "must change password now".
fn shadow_lastchg() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(1)
        .max(1)
        .to_string()
}

fn losetup(disk: &Path) -> Result<String> {
    let output = Command::new("losetup")
        .args(["--find", "--partscan", "--show"])
        .arg(disk)
        .output()?;
    if !output.status.success() {
        return Err(StorageError::Message(format!(
            "losetup failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let dev = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if dev.is_empty() {
        return Err(StorageError::Message("losetup returned no device".into()));
    }
    let _ = Command::new("udevadm")
        .args(["settle", "--timeout=5"])
        .status();
    std::thread::sleep(Duration::from_millis(400));
    Ok(dev)
}

fn inject_on_loop(loopdev: &str, id: &GuestIdentity<'_>) -> Result<()> {
    let parts = partitions(loopdev);
    if parts.is_empty() {
        return Err(StorageError::Message(format!("no partitions on {loopdev}")));
    }
    let mnt = tempfile_mnt()?;
    let mut mounted = None;
    for part in &parts {
        if try_mount(part, &mnt) {
            if mnt.join("etc/os-release").is_file() {
                mounted = Some(part.clone());
                break;
            }
            let _ = Command::new("umount").arg(&mnt).status();
        }
    }
    if mounted.is_none() {
        let _ = fs::remove_dir_all(&mnt);
        return Err(StorageError::Message(
            "could not mount a guest root filesystem to inject login".into(),
        ));
    }
    let written = apply_identity(&mnt, id);
    let _ = File::create(mnt.join(".autorelabel"));
    let _ = Command::new("umount").arg(&mnt).status();
    let _ = fs::remove_dir_all(&mnt);
    written
}

fn partitions(loopdev: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let name = Path::new(loopdev)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let sys = PathBuf::from(format!("/sys/block/{name}"));
    if let Ok(entries) = fs::read_dir(&sys) {
        for ent in entries.flatten() {
            let n = ent.file_name();
            let n = n.to_string_lossy();
            if n.starts_with(name) && n != name {
                out.push(PathBuf::from(format!("/dev/{n}")));
            }
        }
    }
    if out.is_empty() {
        for suffix in ["p1", "p2", "p3", "p4", "p5", "1", "2", "3", "4"] {
            let p = PathBuf::from(format!("{loopdev}{suffix}"));
            if p.exists() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn try_mount(dev: &Path, mnt: &Path) -> bool {
    for extra in [Some("nouuid"), None] {
        let mut cmd = Command::new("mount");
        if let Some(opt) = extra {
            cmd.args(["-o", opt]);
        }
        if cmd
            .arg(dev)
            .arg(mnt)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

fn tempfile_mnt() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("pertisk-inject-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn apply_identity(root: &Path, id: &GuestIdentity<'_>) -> Result<()> {
    let hostname = sanitize_host(id.hostname);
    let user = sanitize_user(id.user);
    fs::write(root.join("etc/hostname"), format!("{hostname}\n"))?;
    patch_hosts(root, &hostname)?;
    if id.password.filter(|p| !p.is_empty()).is_some() {
        patch_sshd(root);
    }
    ensure_user(root, &user)?;
    write_nocloud_seed(
        root,
        &GuestIdentity {
            hostname: id.hostname,
            user: &user,
            password: id.password,
            ssh_authorized_keys: id.ssh_authorized_keys,
        },
        &hostname,
    )?;
    if let Some(password) = id.password.filter(|p| !p.is_empty()) {
        let hash = hash_password(password)?;
        set_shadow_hash(root, &user, &hash)?;
        unlock_passwd(root, &user);
    }
    write_authorized_keys(root, &user, id.ssh_authorized_keys)?;
    let _ = File::create(root.join(".autorelabel"));
    Ok(())
}

fn sanitize_user(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().to_ascii_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
            out.push(c);
        }
    }
    if out.is_empty() || out == "root" {
        "cloud-user".into()
    } else {
        out.chars().take(32).collect()
    }
}

/// Cloud images often have only `root` until cloud-init creates the distro user.
fn ensure_user(root: &Path, user: &str) -> Result<()> {
    if passwd_home(root, user).is_some() && shadow_has(root, user) {
        add_user_to_groups(root, user, &["wheel", "adm", "sudo"]);
        return Ok(());
    }
    let uid = next_uid(root);
    let gid = ensure_group(root, user, uid)?;
    let home = format!("/home/{user}");
    if passwd_home(root, user).is_none() {
        append_line(
            &root.join("etc/passwd"),
            &format!("{user}:x:{uid}:{gid}:Cloud User:{home}:/bin/bash"),
        )?;
    }
    if !shadow_has(root, user) {
        append_line(
            &root.join("etc/shadow"),
            &format!("{user}:*:{}:0:99999:7:::", shadow_lastchg()),
        )?;
    }
    add_user_to_groups(root, user, &["wheel", "adm", "sudo", user]);
    let home_path = root.join(home.trim_start_matches('/'));
    fs::create_dir_all(&home_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&home_path, fs::Permissions::from_mode(0o755));
        let _ = std::os::unix::fs::chown(&home_path, Some(uid), Some(gid));
    }
    let sudoers = root.join("etc/sudoers.d");
    let _ = fs::create_dir_all(&sudoers);
    let sudo_path = sudoers.join("90-pertisk-cloud");
    let _ = fs::write(&sudo_path, format!("{user} ALL=(ALL) NOPASSWD:ALL\n"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&sudo_path, fs::Permissions::from_mode(0o440));
    }
    Ok(())
}

fn shadow_has(root: &Path, user: &str) -> bool {
    let Ok(text) = fs::read_to_string(root.join("etc/shadow")) else {
        return false;
    };
    let prefix = format!("{user}:");
    text.lines().any(|l| l.starts_with(&prefix))
}

fn next_uid(root: &Path) -> u32 {
    let mut used = std::collections::BTreeSet::new();
    if let Ok(text) = fs::read_to_string(root.join("etc/passwd")) {
        for line in text.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                if let Ok(uid) = parts[2].parse::<u32>() {
                    used.insert(uid);
                }
            }
        }
    }
    let mut uid = 1000u32;
    while used.contains(&uid) {
        uid += 1;
    }
    uid
}

fn ensure_group(root: &Path, name: &str, gid: u32) -> Result<u32> {
    let path = root.join("etc/group");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let prefix = format!("{name}:");
    for line in text.lines() {
        if line.starts_with(&prefix) {
            let gid = line
                .split(':')
                .nth(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(gid);
            return Ok(gid);
        }
    }
    append_line(&path, &format!("{name}:x:{gid}:"))?;
    let gshadow = root.join("etc/gshadow");
    if gshadow.is_file()
        && !fs::read_to_string(&gshadow)
            .unwrap_or_default()
            .lines()
            .any(|l| l.starts_with(&prefix))
    {
        let _ = append_line(&gshadow, &format!("{name}:*::{name}"));
    }
    Ok(gid)
}

fn add_user_to_groups(root: &Path, user: &str, groups: &[&str]) {
    let path = root.join("etc/group");
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let mut lines = Vec::new();
    for line in text.lines() {
        let mut parts: Vec<String> = line.split(':').map(|s| s.to_string()).collect();
        if parts.len() >= 4 && groups.contains(&parts[0].as_str()) {
            let mut members: Vec<String> = if parts[3].is_empty() {
                Vec::new()
            } else {
                parts[3].split(',').map(|s| s.to_string()).collect()
            };
            if !members.iter().any(|m| m == user) {
                members.push(user.to_string());
                parts[3] = members.join(",");
            }
            lines.push(parts.join(":"));
        } else {
            lines.push(line.to_string());
        }
    }
    lines.push(String::new());
    let _ = fs::write(path, lines.join("\n"));
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    let mut text = fs::read_to_string(path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(line);
    text.push('\n');
    fs::write(path, text)?;
    Ok(())
}

fn sanitize_host(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').chars().take(63).collect::<String>();
    if out.is_empty() {
        "pertisk".into()
    } else {
        out
    }
}

fn patch_hosts(root: &Path, hostname: &str) -> Result<()> {
    let path = root.join("etc/hosts");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| !l.contains(hostname) && !l.starts_with("127.0.1.1"))
        .map(|l| l.to_string())
        .collect();
    if !lines.iter().any(|l| l.starts_with("127.0.0.1")) {
        lines.insert(0, "127.0.0.1 localhost".into());
    }
    lines.push(format!("127.0.1.1 {hostname}"));
    lines.push(String::new());
    fs::write(path, lines.join("\n"))?;
    Ok(())
}

fn patch_sshd(root: &Path) {
    let dir = root.join("etc/ssh/sshd_config.d");
    let _ = fs::create_dir_all(&dir);
    for path in sshd_files(root) {
        if let Ok(text) = fs::read_to_string(&path) {
            let updated = text
                .lines()
                .map(|line| {
                    let trim = line.trim_start();
                    if trim.starts_with("PasswordAuthentication")
                        || trim.starts_with("#PasswordAuthentication")
                    {
                        "PasswordAuthentication yes".to_string()
                    } else if trim.starts_with("KbdInteractiveAuthentication")
                        || trim.starts_with("#KbdInteractiveAuthentication")
                    {
                        "KbdInteractiveAuthentication yes".to_string()
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let _ = fs::write(&path, format!("{updated}\n"));
        }
    }
    let _ = fs::write(
        dir.join("99-pertisk.conf"),
        "PasswordAuthentication yes\nKbdInteractiveAuthentication yes\nPubkeyAuthentication yes\n",
    );
}

fn sshd_files(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join("etc/ssh/sshd_config")];
    if let Ok(entries) = fs::read_dir(root.join("etc/ssh/sshd_config.d")) {
        for ent in entries.flatten() {
            files.push(ent.path());
        }
    }
    files
}

fn write_nocloud_seed(root: &Path, id: &GuestIdentity<'_>, hostname: &str) -> Result<()> {
    let dir = root.join("var/lib/cloud/seed/nocloud");
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("meta-data"),
        format!("instance-id: iid-{hostname}\nlocal-hostname: {hostname}\nhostname: {hostname}\n"),
    )?;
    let mut user_data = format!(
        "#cloud-config\nhostname: {hostname}\nfqdn: {hostname}\npreserve_hostname: false\nssh_pwauth: true\n"
    );
    if let Some(password) = id.password.filter(|p| !p.is_empty()) {
        user_data.push_str("chpasswd:\n  expire: false\n  list: |\n    ");
        user_data.push_str(id.user);
        user_data.push(':');
        user_data.push_str(password);
        user_data.push('\n');
    }
    if !id.ssh_authorized_keys.is_empty() {
        user_data.push_str("ssh_authorized_keys:\n");
        for key in id.ssh_authorized_keys {
            let key = key.trim();
            if !key.is_empty() {
                user_data.push_str("  - ");
                user_data.push_str(key);
                user_data.push('\n');
            }
        }
    }
    fs::write(dir.join("user-data"), user_data)?;
    let cfg = root.join("etc/cloud/cloud.cfg.d");
    let _ = fs::create_dir_all(&cfg);
    let _ = fs::write(
        cfg.join("99-pertisk.cfg"),
        "datasource_list: [ NoCloud, ConfigDrive, None ]\nssh_pwauth: true\n",
    );
    Ok(())
}

fn hash_password(password: &str) -> Result<String> {
    let mut child = Command::new("openssl")
        .args(["passwd", "-6", "-stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(password.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(StorageError::Message(format!(
            "openssl passwd failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.is_empty() {
        return Err(StorageError::Message(
            "openssl passwd produced no hash".into(),
        ));
    }
    Ok(hash)
}

fn set_shadow_hash(root: &Path, user: &str, hash: &str) -> Result<()> {
    let path = root.join("etc/shadow");
    let text = fs::read_to_string(&path)
        .map_err(|_| StorageError::Message("guest /etc/shadow missing".into()))?;
    let prefix = format!("{user}:");
    let mut found = false;
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with(&prefix) {
            found = true;
            let mut parts: Vec<String> = line.split(':').map(|s| s.to_string()).collect();
            if parts.len() >= 2 {
                parts[1] = hash.to_string();
            }
            if parts.len() >= 3 {
                parts[2] = shadow_lastchg();
            }
            lines.push(parts.join(":"));
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push(format!("{user}:{hash}:{}:0:99999:7:::", shadow_lastchg()));
    }
    lines.push(String::new());
    fs::write(path, lines.join("\n"))?;
    Ok(())
}

fn unlock_passwd(root: &Path, user: &str) {
    let path = root.join("etc/passwd");
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let prefix = format!("{user}:");
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with(&prefix) {
            let mut parts: Vec<String> = line.split(':').map(|s| s.to_string()).collect();
            if parts.len() >= 2 && (parts[1] == "!" || parts[1] == "*" || parts[1] == "!!") {
                parts[1] = "x".into();
            }
            lines.push(parts.join(":"));
        } else {
            lines.push(line.to_string());
        }
    }
    lines.push(String::new());
    let _ = fs::write(path, lines.join("\n"));
}

fn write_authorized_keys(root: &Path, user: &str, keys: &[String]) -> Result<()> {
    let keys: Vec<&str> = keys
        .iter()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .collect();
    if keys.is_empty() {
        return Ok(());
    }
    let Some((uid, gid, home)) = passwd_home(root, user) else {
        return Err(StorageError::Message(format!(
            "guest has no user '{user}' in /etc/passwd"
        )));
    };
    let home = root.join(home.trim_start_matches('/'));
    let ssh = home.join(".ssh");
    fs::create_dir_all(&ssh)?;
    let auth = ssh.join("authorized_keys");
    fs::write(&auth, keys.join("\n") + "\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(&auth, fs::Permissions::from_mode(0o600));
        let _ = std::os::unix::fs::chown(&home, Some(uid), Some(gid));
        let _ = std::os::unix::fs::chown(&ssh, Some(uid), Some(gid));
        let _ = std::os::unix::fs::chown(&auth, Some(uid), Some(gid));
        for path in [&ssh, &auth] {
            let _ = Command::new("setfattr")
                .args([
                    "-n",
                    "security.selinux",
                    "-v",
                    "unconfined_u:object_r:ssh_home_t:s0",
                ])
                .arg(path)
                .status();
        }
    }
    Ok(())
}

fn passwd_home(root: &Path, user: &str) -> Option<(u32, u32, String)> {
    let text = fs::read_to_string(root.join("etc/passwd")).ok()?;
    let prefix = format!("{user}:");
    for line in text.lines() {
        if !line.starts_with(&prefix) {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 6 {
            continue;
        }
        let uid = parts[2].parse().ok()?;
        let gid = parts[3].parse().ok()?;
        return Some((uid, gid, parts[5].to_string()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pertisk-inject-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("etc")).unwrap();
        fs::write(dir.join("etc/passwd"), "root:x:0:0:root:/root:/bin/bash\n").unwrap();
        fs::write(dir.join("etc/shadow"), "root:*:0:0:99999:7:::\n").unwrap();
        fs::write(dir.join("etc/group"), "root:x:0:\nwheel:x:10:\nadm:x:4:\n").unwrap();
        dir
    }

    #[test]
    fn creates_missing_cloud_user_in_passwd_and_shadow() {
        let root = fixture_root();
        apply_identity(
            &root,
            &GuestIdentity {
                hostname: "AlmaLinux-10-1",
                user: "almalinux",
                password: None,
                ssh_authorized_keys: &[],
            },
        )
        .unwrap();
        let passwd = fs::read_to_string(root.join("etc/passwd")).unwrap();
        assert!(
            passwd.contains("almalinux:x:1000:1000:Cloud User:/home/almalinux:/bin/bash"),
            "{passwd}"
        );
        let shadow = fs::read_to_string(root.join("etc/shadow")).unwrap();
        assert!(shadow.contains("almalinux:"), "{shadow}");
        let lastchg = shadow
            .lines()
            .find(|l| l.starts_with("almalinux:"))
            .unwrap()
            .split(':')
            .nth(2)
            .unwrap();
        assert_ne!(lastchg, "0", "{shadow}");
        assert_eq!(
            fs::read_to_string(root.join("etc/hostname"))
                .unwrap()
                .trim(),
            "AlmaLinux-10-1"
        );
        let group = fs::read_to_string(root.join("etc/group")).unwrap();
        assert!(group.contains("wheel:x:10:almalinux"), "{group}");
        assert!(root.join("home/almalinux").is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn set_shadow_hash_appends_when_user_missing() {
        let root = fixture_root();
        set_shadow_hash(&root, "almalinux", "$6$abc$hash").unwrap();
        let shadow = fs::read_to_string(root.join("etc/shadow")).unwrap();
        assert!(shadow.contains("almalinux:$6$abc$hash:"), "{shadow}");
        let lastchg = shadow
            .lines()
            .find(|l| l.starts_with("almalinux:"))
            .unwrap()
            .split(':')
            .nth(2)
            .unwrap();
        assert_ne!(
            lastchg, "0",
            "lastchg=0 forces an immediate password change"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_ssh_keys_skips_comments() {
        let keys = parse_ssh_keys(
            "# comment\nssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeKey user@host\n\nnot-a-key\n",
        );
        assert_eq!(
            keys,
            vec!["ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeKey user@host"]
        );
    }
}
