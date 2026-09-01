//! TLS certificate bootstrap for the HTTPS listener.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use pertisk_types::DaemonConfig;
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, Ia5String, IsCa,
    KeyPair, KeyUsagePurpose, SanType,
};

pub struct TlsBind {
    pub listen: String,
    pub cert: PathBuf,
    pub key: PathBuf,
}

pub fn tls_bind(home: &Path, daemon: &DaemonConfig) -> Option<TlsBind> {
    let listen = daemon.effective_tls_listen()?;
    Some(TlsBind {
        listen,
        cert: daemon
            .tls_cert
            .clone()
            .unwrap_or_else(|| home.join("tls/cert.pem")),
        key: daemon
            .tls_key
            .clone()
            .unwrap_or_else(|| home.join("tls/key.pem")),
    })
}

pub fn ensure_self_signed(cert: &Path, key: &Path) -> Result<(), std::io::Error> {
    if cert.is_file() && key.is_file() {
        return Ok(());
    }
    if let Some(parent) = cert.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let (cert_pem, key_pem) = generate_self_signed()?;
    std::fs::write(cert, cert_pem)?;
    std::fs::write(key, key_pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(key)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(key, perms)?;
    }
    tracing::info!(cert = %cert.display(), key = %key.display(), "wrote self-signed TLS certificate");
    Ok(())
}

fn generate_self_signed() -> Result<(String, String), std::io::Error> {
    let dns = dns_names();
    let mut params = CertificateParams::new(dns).map_err(tls_err)?;
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(DnType::CommonName, "pertisk");
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.not_after = rcgen::date_time_ymd(2036, 9, 1);
    for ip in local_ips() {
        params.subject_alt_names.push(SanType::IpAddress(ip));
    }
    let key_pair = KeyPair::generate().map_err(tls_err)?;
    let cert = params.self_signed(&key_pair).map_err(tls_err)?;
    Ok((cert.pem(), key_pair.serialize_pem()))
}

fn tls_err(err: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(err.to_string())
}

fn dns_names() -> Vec<String> {
    let mut names = vec!["localhost".into(), "pertisk".into()];
    for flag in ["-s", "-f"] {
        if let Ok(out) = std::process::Command::new("hostname").arg(flag).output()
            && let Ok(text) = String::from_utf8(out.stdout)
        {
            let name = text.trim();
            if !name.is_empty() && !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
    }
    names.retain(|n| Ia5String::try_from(n.as_str()).is_ok());
    if names.is_empty() {
        names.push("localhost".into());
    }
    names
}

fn local_ips() -> Vec<IpAddr> {
    let mut ips = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
        let _ = sock.connect("1.1.1.1:443");
        if let Ok(addr) = sock.local_addr() {
            let ip = addr.ip();
            if !ip.is_unspecified() && !ips.contains(&ip) {
                ips.push(ip);
            }
        }
    }
    if let Ok(out) = std::process::Command::new("hostname").arg("-I").output()
        && let Ok(text) = String::from_utf8(out.stdout)
    {
        for tok in text.split_whitespace() {
            if let Ok(ip) = tok.parse::<IpAddr>()
                && !ip.is_unspecified()
                && !ips.contains(&ip)
            {
                ips.push(ip);
            }
        }
    }
    ips
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_pem_pair() {
        let dir = tempfile::tempdir().unwrap();
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");
        ensure_self_signed(&cert, &key).unwrap();
        let cert_pem = std::fs::read_to_string(&cert).unwrap();
        let key_pem = std::fs::read_to_string(&key).unwrap();
        assert!(cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(key_pem.contains("BEGIN PRIVATE KEY") || key_pem.contains("BEGIN RSA PRIVATE KEY"));
        ensure_self_signed(&cert, &key).unwrap();
        assert_eq!(std::fs::read_to_string(&cert).unwrap(), cert_pem);
    }

    #[test]
    fn includes_loopback_san() {
        let ips = local_ips();
        assert!(ips.contains(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }
}
