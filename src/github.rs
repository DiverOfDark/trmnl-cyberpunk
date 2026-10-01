//! The GitHub contribution calendar for the desk screen, read from the same
//! public page the profile embeds (`/users/{user}/contributions`). No token:
//! it shows what the profile shows, private contributions included when the
//! profile is set to count them.

use anyhow::{bail, Result};
use chrono::NaiveDate;
use reqwest::Client;

use crate::data::{ContributionDay, GithubData};
use crate::fetch::NotConfigured;

pub async fn fetch(client: &Client, user: &str) -> Result<GithubData> {
    if user.is_empty() {
        return Err(NotConfigured("GITHUB_USER").into());
    }
    let url = format!("https://github.com/users/{user}/contributions");
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("{url}: HTTP {}", resp.status());
    }
    let days = parse(&resp.text().await?);
    if days.is_empty() {
        bail!("{url}: no contribution cells — has the page changed?");
    }
    Ok(GithubData { user: user.to_string(), days })
}

/// Days out of the calendar markup, oldest first. Each day is a
/// `<td data-date=… data-level=… id=…>`, and its count sits in the
/// `<tool-tip for=id>` that labels it ("7 contributions on …",
/// "No contributions on …").
fn parse(html: &str) -> Vec<ContributionDay> {
    let mut counts = std::collections::HashMap::new();
    for (tag, rest) in tags(html, "<tool-tip") {
        if let Some(id) = attr(tag, "for") {
            let text = rest.split('<').next().unwrap_or_default();
            let n = text
                .split_whitespace()
                .next()
                .and_then(|w| w.replace(',', "").parse::<u32>().ok())
                .unwrap_or(0);
            counts.insert(id.to_string(), n);
        }
    }
    let mut days: Vec<ContributionDay> = tags(html, "<td")
        .filter_map(|(tag, _)| {
            let date = NaiveDate::parse_from_str(attr(tag, "data-date")?, "%Y-%m-%d").ok()?;
            let level = attr(tag, "data-level")?.parse::<u8>().ok()?.min(4);
            let count = attr(tag, "id").and_then(|id| counts.get(id).copied()).unwrap_or(0);
            Some(ContributionDay { date, level, count })
        })
        .collect();
    days.sort_by_key(|d| d.date);
    days
}

/// Every opening tag named `open`, with the text that follows it.
fn tags<'a>(html: &'a str, open: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> {
    html.match_indices(open).filter_map(move |(i, _)| {
        let after = &html[i + open.len()..];
        // `<td` must not match `<tdx`, nor `<tool-tip` a longer name.
        if !after.starts_with([' ', '>', '\n', '\t']) {
            return None;
        }
        let end = after.find('>')?;
        Some((&after[..end], &after[end + 1..]))
    })
}

/// Value of `name="…"` inside a tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_days_levels_and_counts() {
        let html = r#"
<td tabindex="0" data-ix="0" style="width: 11px" data-date="2025-09-28" id="contribution-day-component-0-0" data-level="1" role="gridcell" class="ContributionCalendar-day"></td>
<td tabindex="0" data-ix="1" style="width: 11px" data-date="2025-09-29" id="contribution-day-component-1-0" data-level="0" role="gridcell" class="ContributionCalendar-day"></td>
<td tabindex="0" data-ix="2" data-date="2025-09-30" id="contribution-day-component-2-0" data-level="4" class="ContributionCalendar-day"></td>
<td class="ContributionCalendar-label">Mon</td>
<tool-tip id="tooltip-a" for="contribution-day-component-0-0" popover="manual" class="sr-only">7 contributions on September 28th.</tool-tip>
<tool-tip id="tooltip-b" for="contribution-day-component-1-0" popover="manual" class="sr-only">No contributions on September 29th.</tool-tip>
<tool-tip id="tooltip-c" for="contribution-day-component-2-0" popover="manual" class="sr-only">1,204 contributions on September 30th.</tool-tip>
"#;
        let days = parse(html);
        let got: Vec<_> = days.iter().map(|d| (d.date.to_string(), d.level, d.count)).collect();
        assert_eq!(
            got,
            [
                ("2025-09-28".to_string(), 1, 7),
                ("2025-09-29".to_string(), 0, 0),
                ("2025-09-30".to_string(), 4, 1204),
            ]
        );
    }
}
