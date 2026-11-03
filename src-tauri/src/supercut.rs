//! Supercuts: moments picked by kind out of one recording or out of every match in a date range,
//! cut and joined into one video (see `ffmpeg::render_parts`).
//!
//! Kinds are event kinds, except that kills come in tiers: "kill" is a single kill and
//! "double" ... "penta" are multikill streaks, so a supercut can be "triples and up" only.

use crate::library::{Library, RecEvent, Recording};
use chrono::NaiveDate;

/// The supercut kind of an event: its kind, or the tier of a multikill.
pub fn key(e: &RecEvent) -> &str {
    match (e.kind.as_str(), e.multi) {
        ("multikill", Some(5)) => "penta",
        ("multikill", Some(4)) => "quadra",
        ("multikill", Some(3)) => "triple",
        ("multikill", _) => "double",
        (k, _) => k,
    }
}

/// A stretch of one recording to keep, with the moments in it.
pub struct Window {
    pub start: f64,
    pub end: f64,
    pub events: Vec<RecEvent>,
}

/// The stretches of `rec` around the picked moments: `before` seconds ahead of each and `after`
/// past it (past the last kill of a streak). Overlapping stretches are joined so nothing plays twice.
pub fn windows(rec: &Recording, kinds: &[String], before: f64, after: f64) -> Vec<Window> {
    let mut picked: Vec<&RecEvent> = rec.events.iter().filter(|e| kinds.iter().any(|k| k == key(e))).collect();
    picked.sort_by(|a, b| a.t.total_cmp(&b.t));
    let limit = if rec.duration > 0.0 { rec.duration } else { f64::MAX };
    let mut out: Vec<Window> = Vec::new();
    for e in picked {
        let start = (e.t - before).max(0.0);
        let end = (e.t + e.span.unwrap_or(0.0) + after).min(limit).max(start + 0.5);
        match out.last_mut() {
            Some(w) if start <= w.end => {
                w.end = w.end.max(end);
                w.events.push(e.clone());
            }
            _ => out.push(Window { start, end, events: vec![e.clone()] }),
        }
    }
    out
}

/// The recordings a date range's supercut draws from, oldest match first: each match's full
/// recording, or its highlight and manual clips when it has none. Clips, supercuts and exports
/// made from them are left out so no moment comes twice.
pub fn sources(library: &Library, from: NaiveDate, to: NaiveDate) -> Vec<Recording> {
    let snap = library.snapshot();
    let mut sessions: Vec<_> = snap.sessions.into_iter().filter(|s| (from..=to).contains(&s.started_at.date_naive())).collect();
    sessions.sort_by_key(|s| s.started_at);
    let mut out = Vec::new();
    for s in sessions {
        let mut recs: Vec<Recording> = snap.recordings.iter().filter(|r| r.session_id == s.id && r.deleted_at.is_none()).cloned().collect();
        recs.sort_by_key(|r| r.created_at);
        let full: Vec<Recording> = recs.iter().filter(|r| r.kind == "match" || r.kind == "session").cloned().collect();
        if full.is_empty() {
            out.extend(recs.into_iter().filter(|r| r.kind == "highlight" || r.kind == "manual"));
        } else {
            out.extend(full);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(t: f64, kind: &str, multi: Option<u8>, span: Option<f64>) -> RecEvent {
        RecEvent { t, kind: kind.into(), multi, span, before: 10.0, after: 5.0, ..Default::default() }
    }

    fn rec(events: Vec<RecEvent>) -> Recording {
        let mut r: Recording = serde_json::from_value(serde_json::json!({
            "id": "r", "sessionId": "s", "kind": "match", "title": "Match 01", "file": "Match 01.mp4", "createdAt": "2026-11-02T17:42:00-05:00", "duration": 2000.0,
        }))
        .unwrap();
        r.events = events;
        r
    }

    #[test]
    fn tiers_pick_only_those_streaks() {
        let r = rec(vec![ev(100.0, "kill", None, None), ev(318.4, "multikill", Some(2), Some(6.2)), ev(1695.2, "multikill", Some(5), Some(24.3)), ev(1800.0, "death", None, None)]);
        let only_pentas = windows(&r, &["penta".into()], 5.0, 2.0);
        assert_eq!(only_pentas.len(), 1);
        let w = &only_pentas[0];
        assert!((w.start - 1690.2).abs() < 1e-9 && (w.end - (1695.2 + 24.3 + 2.0)).abs() < 1e-9, "covers the whole streak");
        let triples_up = windows(&r, &["triple".into(), "quadra".into(), "penta".into()], 5.0, 2.0);
        assert_eq!(triples_up.len(), 1, "the double is not a triple");
        let kills = windows(&r, &["kill".into()], 5.0, 2.0);
        assert_eq!(kills.len(), 1, "single kills only, not streaks");
    }

    #[test]
    fn close_moments_share_a_window() {
        let r = rec(vec![ev(100.0, "kill", None, None), ev(104.0, "kill", None, None), ev(300.0, "kill", None, None)]);
        let w = windows(&r, &["kill".into()], 5.0, 2.0);
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].start, w[0].end, w[0].events.len()), (95.0, 106.0, 2));
    }
}
