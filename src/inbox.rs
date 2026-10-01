//! Unread mail for the desk screen, over IMAP (implicit TLS, port 993).
//!
//! The mailbox is opened read-only (`EXAMINE`) and headers are fetched with
//! `BODY.PEEK`, so looking never marks anything as read.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use mail_parser::{HeaderValue, MessageParser};
use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

use crate::data::{InboxData, MailItem};
use crate::fetch::NotConfigured;

/// How many of the newest unread messages are read for the people count and
/// the list. An inbox with hundreds unread is a backlog, not news; the count
/// still covers all of them.
const LOOK_AT: usize = 50;
const LIST: usize = 3;

pub struct Imap {
    host: String,
    port: u16,
    user: String,
    password: String,
    mailbox: String,
}

impl Imap {
    /// `None` when `IMAP_HOST` is unset — the inbox panel is opted out.
    pub fn from_env() -> Option<Self> {
        let var = |k: &str| std::env::var(k).unwrap_or_default().trim().to_string();
        let host = var("IMAP_HOST");
        if host.is_empty() {
            return None;
        }
        Some(Self {
            host,
            port: var("IMAP_PORT").parse().unwrap_or(993),
            user: var("IMAP_USER"),
            password: std::env::var("IMAP_PASSWORD").unwrap_or_default(),
            mailbox: Some(var("IMAP_MAILBOX")).filter(|m| !m.is_empty()).unwrap_or_else(|| "INBOX".into()),
        })
    }

    pub async fn fetch(imap: Option<&Self>) -> Result<InboxData> {
        imap.ok_or(NotConfigured("IMAP_HOST"))?.unread().await
    }

    async fn unread(&self) -> Result<InboxData> {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let tcp = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .with_context(|| format!("connecting to {}:{}", self.host, self.port))?;
        let name = tokio_rustls::rustls::pki_types::ServerName::try_from(self.host.clone())?;
        let tls = TlsConnector::from(Arc::new(config)).connect(name, tcp).await?;

        let mut client = async_imap::Client::new(tls);
        client
            .read_response()
            .await?
            .ok_or_else(|| anyhow!("server closed before greeting"))?;
        let mut session = client
            .login(&self.user, &self.password)
            .await
            .map_err(|(e, _)| anyhow!("IMAP login as {}: {e}", self.user))?;
        session.examine(&self.mailbox).await?;

        let mut uids: Vec<u32> = session.uid_search("UNSEEN").await?.into_iter().collect();
        uids.sort_unstable_by(|a, b| b.cmp(a));
        let unread = uids.len() as u32;
        uids.truncate(LOOK_AT);

        let mut mails = Vec::new();
        if !uids.is_empty() {
            let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            let fetches: Vec<_> = session
                .uid_fetch(&set, "(INTERNALDATE BODY.PEEK[HEADER])")
                .await?
                .try_collect()
                .await?;
            for f in &fetches {
                let Some(header) = f.header() else { continue };
                let received = f.internal_date().map(|t| t.with_timezone(&Utc));
                if let Some(mail) = parse_header(header, received) {
                    mails.push(mail);
                }
            }
        }
        let _ = session.logout().await;

        Ok(summarize(unread, mails))
    }
}

fn summarize(unread: u32, mut mails: Vec<MailItem>) -> InboxData {
    let people = mails.iter().filter(|m| m.person).count() as u32;
    mails.sort_by(|a, b| b.person.cmp(&a.person).then(b.received.cmp(&a.received)));
    mails.truncate(LIST);
    InboxData { unread, people, recent: mails }
}

/// Sender, subject and date out of a raw header block, deciding whether a
/// person wrote it.
fn parse_header(raw: &[u8], received: Option<DateTime<Utc>>) -> Option<MailItem> {
    let msg = MessageParser::default().parse_headers(raw)?;
    let from = msg.from().and_then(|a| a.first());
    let address = from.and_then(|a| a.address()).unwrap_or_default().to_string();
    let name = from
        .and_then(|a| a.name())
        .map(|n| n.trim().trim_matches('"').trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| address.split('@').next().unwrap_or_default().to_string());
    let received = received
        .or_else(|| msg.date().and_then(|d| DateTime::from_timestamp(d.to_timestamp(), 0)))
        .unwrap_or_else(Utc::now);
    let has = |h: &'static str| msg.header(h).is_some_and(|v| !matches!(v, HeaderValue::Empty));
    let text = |h: &'static str| {
        msg.header(h).and_then(|v| v.as_text()).unwrap_or_default().trim().to_ascii_lowercase()
    };
    let bulk = has("List-Id")
        || has("List-Unsubscribe")
        || matches!(text("Precedence").as_str(), "bulk" | "list" | "junk")
        || !matches!(text("Auto-Submitted").as_str(), "" | "no");
    Some(MailItem {
        from: if name.is_empty() { "?".into() } else { name },
        subject: msg.subject().unwrap_or("(no subject)").trim().to_string(),
        received,
        person: !bulk && !robot_address(&address),
    })
}

/// Mailboxes that are never a person, whatever the headers claim: robot
/// prefixes (`notifications@`, `noreply-123@`) and role accounts.
fn robot_address(address: &str) -> bool {
    let local = address.split('@').next().unwrap_or_default().to_ascii_lowercase();
    const PREFIXES: &[&str] = &[
        "noreply", "no-reply", "no_reply", "donotreply", "do-not-reply", "notification",
        "notify", "mailer-daemon", "postmaster", "bounce", "newsletter",
    ];
    const ROLES: &[&str] = &[
        "news", "info", "support", "billing", "invoice", "invoices", "alerts", "updates",
        "team", "hello",
    ];
    PREFIXES.iter().any(|p| local.starts_with(p))
        || ROLES.contains(&local.as_str())
        || local.contains("noreply")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(raw: &str) -> MailItem {
        parse_header(raw.replace('\n', "\r\n").as_bytes(), None).unwrap()
    }

    #[test]
    fn a_person_writing_directly_is_a_person() {
        let m = mail("From: \"Anna K.\" <anna@example.org>\nSubject: Re: Saturday plans\nDate: Wed, 30 Sep 2026 11:52:00 +0200\n\n");
        assert_eq!(m.from, "Anna K.");
        assert_eq!(m.subject, "Re: Saturday plans");
        assert!(m.person);
        assert_eq!(m.received.to_rfc3339(), "2026-09-30T09:52:00+00:00");
    }

    #[test]
    fn lists_and_robots_are_not_people() {
        assert!(!mail("From: Hetzner <billing@hetzner.com>\nSubject: Invoice\n\n").person);
        assert!(!mail("From: Shop <shop@example.com>\nList-Unsubscribe: <mailto:u@example.com>\nSubject: Sale\n\n").person);
        assert!(!mail("From: GitHub <notifications@github.com>\nSubject: PR\n\n").person);
        assert!(!mail("From: cron@host\nAuto-Submitted: auto-generated\nSubject: job\n\n").person);
    }

    #[test]
    fn encoded_subjects_and_bare_addresses() {
        let m = mail("From: landlord@example.org\nSubject: =?UTF-8?B?0J/RgNC40LLQtdGC?=\n\n");
        assert_eq!(m.from, "landlord");
        assert_eq!(m.subject, "Привет");
    }

    #[test]
    fn people_are_listed_first() {
        let at = |m: i64| Utc::now() - chrono::Duration::minutes(m);
        let item = |from: &str, person, ago| MailItem { from: from.into(), subject: String::new(), received: at(ago), person };
        let s = summarize(9, vec![item("shop", false, 1), item("old friend", true, 300), item("friend", true, 30), item("bank", false, 2)]);
        assert_eq!(s.unread, 9);
        assert_eq!(s.people, 2);
        let names: Vec<_> = s.recent.iter().map(|m| m.from.as_str()).collect();
        assert_eq!(names, ["friend", "old friend", "shop"]);
    }
}
