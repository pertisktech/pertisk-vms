//! SMTP event notifications (shared recipient list).

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use pertisk_types::{NotifyConfig, SmtpTls};
use tracing::{info, warn};

pub async fn send_event(cfg: &NotifyConfig, node_name: &str, kind: &str, subject: &str, body: &str) {
    if !cfg.wants_event(kind) {
        return;
    }
    if !cfg.smtp_ready() {
        warn!(event = kind, "notify skipped: SMTP not configured");
        return;
    }
    let subject = format!("[{node_name}] {subject}");
    let body = format!("{body}\n\n— pertisk on {node_name}\n");
    if let Err(err) = send_mail(cfg, &subject, &body).await {
        warn!(event = kind, error = %err, "notify mail failed");
    } else {
        info!(event = kind, "notify mail sent");
    }
}

pub async fn send_test(cfg: &NotifyConfig, node_name: &str) -> Result<(), String> {
    if !cfg.smtp_ready() {
        return Err("SMTP host, from address, and at least one recipient are required".into());
    }
    let subject = format!("[{node_name}] Pertisk test email");
    let body = format!(
        "This is a test message from pertisk on {node_name}.\n\nIf you received this, SMTP settings work.\n"
    );
    send_mail(cfg, &subject, &body).await
}

async fn send_mail(cfg: &NotifyConfig, subject: &str, body: &str) -> Result<(), String> {
    let from: Mailbox = cfg
        .from
        .trim()
        .parse()
        .map_err(|err| format!("invalid from address: {err}"))?;
    let mut builder = Message::builder().from(from).subject(subject);
    let mut any = false;
    for raw in &cfg.recipients {
        let addr = raw.trim();
        if addr.is_empty() {
            continue;
        }
        let mailbox: Mailbox = addr
            .parse()
            .map_err(|err| format!("invalid recipient {addr}: {err}"))?;
        builder = builder.to(mailbox);
        any = true;
    }
    if !any {
        return Err("no recipients".into());
    }
    let message = builder
        .body(body.to_string())
        .map_err(|err| format!("build message: {err}"))?;

    let mailer = build_transport(cfg)?;
    mailer
        .send(message)
        .await
        .map_err(|err| format!("smtp send: {err}"))?;
    Ok(())
}

fn build_transport(cfg: &NotifyConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let host = cfg.smtp_host.trim();
    let creds = if cfg.smtp_user.trim().is_empty() {
        None
    } else {
        Some(Credentials::new(
            cfg.smtp_user.trim().to_string(),
            cfg.smtp_password.clone(),
        ))
    };

    match cfg.smtp_tls {
        SmtpTls::Off => {
            let mut b = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
                .port(cfg.smtp_port);
            if let Some(c) = creds {
                b = b.credentials(c);
            }
            Ok(b.build())
        }
        SmtpTls::StartTls => {
            let tls = TlsParameters::new(host.to_string())
                .map_err(|err| format!("tls params: {err}"))?;
            let mut b = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                .map_err(|err| format!("smtp relay: {err}"))?
                .port(cfg.smtp_port)
                .tls(Tls::Required(tls));
            if let Some(c) = creds {
                b = b.credentials(c);
            }
            Ok(b.build())
        }
        SmtpTls::Tls => {
            let tls = TlsParameters::new(host.to_string())
                .map_err(|err| format!("tls params: {err}"))?;
            let mut b = AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                .map_err(|err| format!("smtp relay: {err}"))?
                .port(cfg.smtp_port)
                .tls(Tls::Wrapper(tls));
            if let Some(c) = creds {
                b = b.credentials(c);
            }
            Ok(b.build())
        }
    }
}
