//! Match results from the League client (LCU) for League matches whose result never arrived,
//! and the champions behind the player names in older recordings' events.
//!
//! A recording gets its result from the Live Client's GameEnd event, but League can close before
//! that event is read (a lost ARAM on 2026-10-31 ended with none). Outplayed imports made of
//! highlights only have no result either. The signed-in client's match history has the result of
//! its latest 40 games (it ignores paging), so a background thread looks up every finished match
//! without a result there, matched by champion and start time: at startup, and every 20 seconds
//! while a match is still waiting for the history to catch up.

use crate::core::Core;
use crate::library::Session;
use chrono::{DateTime, Local, TimeZone};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CLIENT_EXES: [&str; 2] = ["LeagueClientUx.exe", "LeagueClient.exe"];
/// How long a match newer than the whole history keeps being looked up.
const WAIT_FOR_HISTORY: Duration = Duration::from_secs(20 * 60);
/// A recording starts when the game process does, a little after the game is created; one
/// started late (app restarted mid-game) still falls inside the game.
const EARLY_START: f64 = 180.0;

/// One game of the client's match history, from the signed-in player's side.
#[derive(Clone, Debug)]
pub struct Game {
    pub id: i64,
    pub created: DateTime<Local>,
    /// Seconds.
    pub duration: f64,
    /// Data Dragon key, e.g. "MonkeyKing".
    pub champion: Option<String>,
    pub win: bool,
}

struct Lcu {
    http: reqwest::blocking::Client,
    base: String,
    password: String,
}

impl Lcu {
    /// The port and password come from the `lockfile` the running client writes next to itself
    /// (`LeagueClient:<pid>:<port>:<password>:https`).
    fn connect() -> Option<Self> {
        let procs = crate::win::processes();
        let lockfile = CLIENT_EXES
            .iter()
            .filter_map(|n| crate::win::find_process(&procs, n))
            .filter_map(crate::win::process_path)
            .filter_map(|exe| exe.parent().map(|d| d.join("lockfile")))
            .find(|p| p.exists())?;
        let text = std::fs::read_to_string(lockfile).ok()?;
        let parts: Vec<&str> = text.trim().split(':').collect();
        let port: u16 = parts.get(2)?.parse().ok()?;
        let http = reqwest::blocking::Client::builder()
            // The client serves a self-signed Riot certificate, only ever on 127.0.0.1.
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self { http, base: format!("https://127.0.0.1:{port}"), password: parts.get(3)?.to_string() })
    }

    fn get(&self, path: &str) -> Option<Value> {
        let resp = self.http.get(format!("{}{path}", self.base)).basic_auth("riot", Some(&self.password)).send().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json().ok()
    }

    /// Champion id -> Data Dragon key.
    fn champions(&self) -> Option<HashMap<i64, String>> {
        let list = self.get("/lol-game-data/assets/v1/champion-summary.json")?;
        Some(
            list.as_array()?
                .iter()
                .filter_map(|c| Some((c.get("id")?.as_i64()?, c.get("alias")?.as_str()?.to_string())))
                .filter(|(id, _)| *id > 0)
                .collect(),
        )
    }

    fn history(&self, champions: &HashMap<i64, String>) -> Option<Vec<Game>> {
        let v = self.get("/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex=40")?;
        let games = v.pointer("/games/games")?.as_array()?;
        Some(
            games
                .iter()
                .filter_map(|g| {
                    // Only the signed-in player is listed.
                    let me = g.pointer("/participants/0")?;
                    Some(Game {
                        id: g.get("gameId")?.as_i64()?,
                        created: Local.timestamp_millis_opt(g.get("gameCreation")?.as_i64()?).single()?,
                        duration: g.get("gameDuration")?.as_f64()?,
                        champion: me.get("championId").and_then(Value::as_i64).and_then(|id| champions.get(&id).cloned()),
                        win: me.pointer("/stats/win")?.as_bool()?,
                    })
                })
                .collect(),
        )
    }
}

impl Lcu {
    /// Every player of a game, normalized in-game name -> champion key (from the full game).
    fn players(&self, game_id: i64, champions: &HashMap<i64, String>) -> Option<HashMap<String, String>> {
        let g = self.get(&format!("/lol-match-history/v1/games/{game_id}"))?;
        let champion_of: HashMap<i64, i64> = g
            .get("participants")?
            .as_array()?
            .iter()
            .filter_map(|p| Some((p.get("participantId")?.as_i64()?, p.get("championId")?.as_i64()?)))
            .collect();
        let mut out = HashMap::new();
        // A player who hides their name shows up in events under their champion's name.
        for c in champion_of.values().filter_map(|c| champions.get(c)) {
            out.insert(c.to_lowercase(), c.clone());
        }
        for p in g.get("participantIdentities")?.as_array()? {
            let Some(champion) = p.get("participantId").and_then(Value::as_i64).and_then(|id| champion_of.get(&id)).and_then(|c| champions.get(c)) else { continue };
            for key in ["gameName", "summonerName"] {
                if let Some(name) = p.pointer(&format!("/player/{key}")).and_then(Value::as_str).filter(|n| !n.is_empty()) {
                    out.insert(name.trim().to_lowercase(), champion.clone());
                }
            }
        }
        Some(out)
    }
}

/// The player named in an event label as the game wrote it ("Killed by Garen Main"), unless it
/// is not a player ("a turret", "minions").
pub fn player_in_label(label: &str) -> Option<&str> {
    let name = ["Killed by ", "First Blood - ", "Killed ", "Assist on "].iter().find_map(|p| label.strip_prefix(p))?;
    (!matches!(name, "a turret" | "minions" | "the enemy") && !name.is_empty()).then_some(name)
}

/// Fill in `champion` on kills, deaths and assists from a game's players; returns whether any changed.
pub fn name_champions(events: &mut [crate::library::RecEvent], players: &HashMap<String, String>) -> bool {
    let mut changed = false;
    for e in events.iter_mut().filter(|e| e.champion.is_none() && matches!(e.kind.as_str(), "kill" | "death" | "assist")) {
        let found = player_in_label(&e.label).and_then(|n| {
            let n = n.trim().to_lowercase();
            // "Kai'Sa" in a label, "kaisa" as a champion key.
            players.get(&n).or_else(|| players.get(&n.replace([' ', '\'', '.', '&'], "")))
        });
        if let Some(c) = found {
            e.champion = Some(c.clone());
            changed = true;
        }
    }
    changed
}

/// The history game a session recorded: same champion, and the recording started inside the
/// game (or shortly before it was created). The closest start wins.
pub fn find_game<'a>(s: &Session, games: &'a [Game]) -> Option<&'a Game> {
    let key = s.champion_key.as_deref()?;
    games
        .iter()
        .filter(|g| g.champion.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(key)))
        .filter(|g| {
            let d = (s.started_at - g.created).num_milliseconds() as f64 / 1000.0;
            d > -EARLY_START && d < g.duration
        })
        .min_by_key(|g| (s.started_at - g.created).num_seconds().abs())
}

pub fn spawn(core: Arc<Core>) {
    std::thread::Builder::new()
        .name("match-results".into())
        .spawn(move || {
            let mut checker = Checker::default();
            // Nothing to look up and nothing changed since: skip, so an idle app stays idle.
            let mut settled_at = None;
            loop {
                let generation = core.library.generation();
                if settled_at != Some(generation) {
                    settled_at = checker.run(&core).then_some(generation);
                }
                std::thread::sleep(Duration::from_secs(20));
            }
        })
        .expect("match results thread");
}

#[derive(Default)]
struct Checker {
    champions: Option<HashMap<i64, String>>,
    /// Sessions the history cannot have (older than it, or another account): not looked up
    /// again until the next start.
    given_up: HashSet<String>,
    /// When a session newer than the whole history was first looked up.
    waiting: HashMap<String, Instant>,
    /// Sessions whose event names were looked up (or cannot be): not again until the next start.
    named: HashSet<String>,
}

impl Checker {
    fn missing(&self, core: &Core) -> Vec<Session> {
        let live = core.status.read().session_id.clone();
        core.library.sessions_where(|s, _| {
            s.result.is_none()
                && s.game == crate::game::LOL_GAME
                // A full session can span several games.
                && s.capture_mode != "session"
                && s.champion_key.is_some()
                && live.as_deref() != Some(s.id.as_str())
                && !self.given_up.contains(&s.id)
        })
    }

    /// Sessions with kills, deaths or assists that name a player but not their champion (recorded
    /// before champions were stored).
    fn unnamed(&self, core: &Core) -> Vec<Session> {
        core.library.sessions_where(|s, mut recordings| {
            s.game == crate::game::LOL_GAME
                && s.champion_key.is_some()
                && s.capture_mode != "session"
                && !self.named.contains(&s.id)
                && Iterator::any(&mut recordings, |r| r.events.iter().any(|e| e.champion.is_none() && matches!(e.kind.as_str(), "kill" | "death" | "assist") && player_in_label(&e.label).is_some()))
        })
    }

    /// Look up what is missing. True when nothing is left to look up until the library changes.
    fn run(&mut self, core: &Core) -> bool {
        let missing = self.missing(core);
        let unnamed = self.unnamed(core);
        if missing.is_empty() && unnamed.is_empty() {
            return true;
        }
        let Some(lcu) = Lcu::connect() else { return false };
        if self.champions.is_none() {
            self.champions = lcu.champions();
        }
        let Some(champions) = &self.champions else { return false };
        let Some(games) = lcu.history(champions) else { return false };
        let Some(oldest) = games.iter().map(|g| g.created).min() else { return false };
        let newest_end = games.iter().map(|g| g.created + chrono::Duration::seconds(g.duration as i64)).max().unwrap_or(oldest);

        let mut found = 0;
        for mut s in missing {
            if let Some(g) = find_game(&s, &games) {
                s.result = Some(if g.win { "win" } else { "loss" }.into());
                s.result_source = Some("client".into());
                match core.library.save_session(&s) {
                    Ok(()) => {
                        log::info!("{} ({}) {} per the League client", s.title, s.started_at.format("%Y-%m-%d %H:%M"), if g.win { "won" } else { "lost" });
                        found += 1;
                    }
                    Err(e) => log::warn!("could not save the result of {}: {e:#}", s.title),
                }
                self.waiting.remove(&s.id);
            } else if s.started_at < oldest - chrono::Duration::seconds(EARLY_START as i64) || s.started_at < newest_end {
                // Inside or before the history's span and not in it: never will be.
                self.given_up.insert(s.id);
            } else if self.waiting.entry(s.id.clone()).or_insert_with(Instant::now).elapsed() > WAIT_FOR_HISTORY {
                self.given_up.insert(s.id);
            }
        }
        // Champions for the player names in older recordings, from each game's full player list.
        for s in unnamed {
            self.named.insert(s.id.clone());
            let Some(g) = find_game(&s, &games) else { continue };
            let Some(players) = lcu.players(g.id, champions) else { continue };
            let mut named = 0;
            for r in core.library.recordings_of(&s.id) {
                let mut events = r.events.clone();
                if name_champions(&mut events, &players) && core.library.update(&r.id, |r| r.events = events).is_ok() {
                    named += 1;
                }
            }
            if named > 0 {
                log::info!("{} ({}): champions named in {named} recordings", s.title, s.started_at.format("%Y-%m-%d %H:%M"));
                found += 1;
            }
        }
        if found > 0 {
            core.library_changed();
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Local> {
        Local.from_local_datetime(&chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()).single().unwrap()
    }

    fn session(started: &str, key: &str) -> Session {
        serde_json::from_value(serde_json::json!({
            "id": "s", "game": "League of Legends", "title": "Match 03", "startedAt": at(started),
            "championKey": key, "captureMode": "match",
        }))
        .unwrap()
    }

    fn game(created: &str, duration: f64, key: &str, win: bool) -> Game {
        Game { id: 1, created: at(created), duration, champion: Some(key.into()), win }
    }

    #[test]
    fn names_become_champions() {
        let players: HashMap<String, String> = [("call anh phoong", "Pyke"), ("nautilus", "Nautilus"), ("melk and cookies", "Lulu"), ("kaisa", "Kaisa")]
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        let ev = |kind: &str, label: &str| crate::library::RecEvent { kind: kind.into(), label: label.into(), ..Default::default() };
        let mut events = vec![
            ev("death", "Killed by Call Anh Phoong"),
            ev("kill", "Killed Nautilus"),
            ev("assist", "Assist on Melk and Cookies"),
            ev("death", "Killed by a turret"),
            ev("kill", "First Blood - Nautilus"),
            ev("kill", "Killed Someone Else"),
            ev("death", "Killed by Kai'Sa"),
        ];
        assert!(name_champions(&mut events, &players));
        let got: Vec<Option<&str>> = events.iter().map(|e| e.champion.as_deref()).collect();
        assert_eq!(got, [Some("Pyke"), Some("Nautilus"), Some("Lulu"), None, Some("Nautilus"), None, Some("Kaisa")]);
        assert!(!name_champions(&mut events, &players));
    }

    #[test]
    fn matches_by_champion_and_start_time() {
        // The 2026-10-31 evening from the real client history.
        let games = [
            game("2026-10-31 22:32:27", 1194.0, "Tryndamere", false),
            game("2026-10-31 22:13:48", 947.0, "Ekko", true),
            game("2026-10-31 22:03:03", 542.0, "Belveth", true),
        ];
        assert!(!find_game(&session("2026-10-31 22:32:29", "Tryndamere"), &games).unwrap().win);
        // Started five minutes into the game after an app restart.
        assert!(find_game(&session("2026-10-31 22:19:23", "Ekko"), &games).unwrap().win);
        // Same time, other champion: not this game.
        assert!(find_game(&session("2026-10-31 22:32:29", "Ekko"), &games).is_none());
        // After the game ended.
        assert!(find_game(&session("2026-10-31 22:55:00", "Tryndamere"), &games).is_none());
        // Same champion twice in a row picks the game the recording started in.
        let twice = [game("2026-10-31 22:00:00", 900.0, "Ekko", true), game("2026-10-31 22:16:00", 900.0, "Ekko", false)];
        assert!(!find_game(&session("2026-10-31 22:16:05", "Ekko"), &twice).unwrap().win);
    }
}
