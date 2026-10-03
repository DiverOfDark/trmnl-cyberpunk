//! `GET /api/stackchan/usage`: Claude rate limits for the Femto desk robot
//! (M5Stack StackChan). Same numbers as the desk screen, plus the reset
//! times the device counts down locally between polls.

use std::hash::{Hash, Hasher};

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

use crate::data::{AgentUsage, SectionStatus};
use crate::AppState;

#[derive(Serialize, ToSchema, PartialEq, Debug)]
pub struct StackchanUsage {
    /// A Claude account is signed in on `/agents`.
    pub signed_in: bool,
    /// The most recent upstream pull succeeded.
    pub ok: bool,
    /// When usage was last pulled successfully.
    pub fetched_at: Option<DateTime<Utc>>,
    /// `null` until the first successful pull.
    pub session: Option<Window>,
    pub week: Option<Window>,
    /// Requests are refused right now, or a window is spent.
    pub limited: bool,
    /// When a limited account comes back.
    pub back_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, ToSchema, PartialEq, Debug)]
pub struct Window {
    /// Percent used, 0..100.
    pub pct: u8,
    /// `null` while the window hasn't started.
    pub resets_at: Option<DateTime<Utc>>,
    pub window_secs: i64,
    /// Session only: where the window ends up at the current rate, percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_pct: Option<u8>,
    /// Week only: usage against an even spread, ±5 points (desk screen rule).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>, example = "over")]
    pub pace: Option<Pace>,
}

#[derive(Serialize, ToSchema, PartialEq, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Pace {
    Over,
    On,
    Under,
}

pub fn usage(signed_in: bool, usage: Option<&AgentUsage>, status: &SectionStatus, now: DateTime<Utc>) -> StackchanUsage {
    let usage = usage.filter(|_| signed_in);
    StackchanUsage {
        signed_in,
        ok: signed_in && status.ok,
        fetched_at: status.last_ok,
        session: usage.map(|u| Window {
            pct: u.session_pct,
            resets_at: u.session_resets,
            window_secs: u.session_window_secs,
            projection_pct: u.session_projection(now),
            pace: None,
        }),
        week: usage.map(|u| Window {
            pct: u.week_pct,
            resets_at: u.week_resets,
            window_secs: u.week_window_secs,
            projection_pct: None,
            pace: u.week_pace(now).map(|p| match u.week_pct as i32 - p as i32 {
                d if d > 5 => Pace::Over,
                d if d < -5 => Pace::Under,
                _ => Pace::On,
            }),
        }),
        limited: usage.is_some_and(AgentUsage::is_limited),
        back_at: usage.filter(|u| u.is_limited()).and_then(AgentUsage::back_at),
    }
}

/// `STACKCHAN_TOKEN` set → requests must carry `Authorization: Bearer <it>`.
fn authorized(headers: &HeaderMap) -> bool {
    let Ok(token) = std::env::var("STACKCHAN_TOKEN") else { return true };
    if token.is_empty() {
        return true;
    }
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| t == token)
}

#[utoipa::path(
    get,
    path = "/api/stackchan/usage",
    responses(
        (status = 200, description = "Claude usage for the StackChan", body = StackchanUsage),
        (status = 304, description = "Unchanged since the `If-None-Match` ETag"),
        (status = 401, description = "`STACKCHAN_TOKEN` is set and the bearer token doesn't match"),
        (status = 503, description = "No Claude account signed in (body still describes the state)", body = StackchanUsage),
    ),
    tag = "stackchan",
)]
pub async fn get_usage(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&headers) {
        return (StatusCode::UNAUTHORIZED, "bad or missing bearer token").into_response();
    }
    // Local mode serves mock usage; present it as signed in so the device
    // can be developed against it.
    let signed_in = state.local_mode || state.agents.status().await.iter().any(|l| l.provider == "claude" && l.signed_in);
    let body = {
        let data = state.data.read().await;
        usage(signed_in, data.claude.as_ref(), &data.status.claude, Utc::now())
    };
    let json = serde_json::to_string(&body).expect("usage serializes");
    let etag = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        json.hash(&mut h);
        format!("W/\"{:016x}\"", h.finish())
    };
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
    }
    let status = if signed_in { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (
        status,
        [(header::CONTENT_TYPE, "application/json".to_string()), (header::ETAG, etag), (header::CACHE_CONTROL, "no-cache".to_string())],
        json,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 3, h, m, 0).unwrap()
    }

    fn claude() -> AgentUsage {
        AgentUsage {
            name: "CLAUDE".into(),
            session_pct: 38,
            session_resets: Some(at(14, 0)),
            session_window_secs: 5 * 3600,
            week_pct: 61,
            week_resets: Some(Utc.with_ymd_and_hms(2026, 10, 8, 7, 0, 0).unwrap()),
            week_window_secs: 7 * 86_400,
            limited: false,
        }
    }

    #[test]
    fn signed_in_shape() {
        let now = at(11, 0);
        let u = usage(true, Some(&claude()), &SectionStatus::fresh(now), now);
        assert!(u.signed_in && u.ok && !u.limited);
        let s = u.session.as_ref().unwrap();
        assert_eq!((s.pct, s.resets_at, s.window_secs), (38, Some(at(14, 0)), 18_000));
        // 2h of 5h elapsed: 38 % → 95 % projected.
        assert_eq!(s.projection_pct, Some(95));
        let w = u.week.as_ref().unwrap();
        assert_eq!(w.pace, Some(Pace::Over));
        let json = serde_json::to_value(&u).unwrap();
        assert_eq!(json["week"]["pace"], "over");
        assert!(json["session"].get("pace").is_none());
    }

    #[test]
    fn signed_out_hides_numbers() {
        let now = at(11, 0);
        let u = usage(false, Some(&claude()), &SectionStatus::fresh(now), now);
        assert!(!u.signed_in && !u.ok);
        assert!(u.session.is_none() && u.week.is_none());
    }

    #[test]
    fn limited_reports_back_at() {
        let now = at(11, 0);
        let mut c = claude();
        c.session_pct = 100;
        let u = usage(true, Some(&c), &SectionStatus::fresh(now), now);
        assert!(u.limited);
        assert_eq!(u.back_at, Some(at(14, 0)));
    }
}
