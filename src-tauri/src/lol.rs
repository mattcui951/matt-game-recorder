//! League of Legends Live Client Data API (https://127.0.0.1:2999) and the rules that turn its
//! raw events into the player's highlights.
//!
//! Highlight rules (from the active player's point of view):
//!   kill       ChampionKill where I am the killer (FirstBlood noted in the label)
//!   multikill  Multikill where I am the killer (2 double ... 5 penta)
//!   death      ChampionKill where I am the victim (turret/minion executions included)
//!   assist     ChampionKill where I am among the assisters
//!   turret     TurretKilled of an enemy turret (my team destroyed it)
//!   inhibitor  InhibKilled of an enemy inhibitor
//!   dragon / herald / baron   objective taken by my team (Stolen noted in the label)
//!   victory    GameEnd with Result "Win"
//!   ultimate   my ultimate key pressed in game (see `input`), rate-limited

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

const BASE: &str = "https://127.0.0.1:2999/liveclientdata";

pub struct LiveClient {
    http: reqwest::blocking::Client,
}

impl Default for LiveClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveClient {
    pub fn new() -> Self {
        let http = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_millis(900))
            .build()
            .expect("http client");
        Self { http }
    }

    fn get(&self, path: &str) -> Option<Value> {
        let resp = self.http.get(format!("{BASE}/{path}")).send().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json().ok()
    }

    pub fn game_stats(&self) -> Option<Value> {
        self.get("gamestats")
    }

    pub fn events(&self) -> Option<Vec<Value>> {
        self.get("eventdata")?.get("Events")?.as_array().cloned()
    }

    pub fn active_player(&self) -> Option<Value> {
        self.get("activeplayer")
    }

    pub fn player_list(&self) -> Option<Vec<Value>> {
        self.get("playerlist")?.as_array().cloned()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameEvent {
    pub kind: String,
    /// Seconds of game time.
    pub game_time: f64,
    pub label: String,
    /// Kill count for multikills (2..=5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi: Option<u8>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kda {
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
}

fn norm(name: &str) -> String {
    name.split('#').next().unwrap_or("").trim().to_lowercase()
}

fn names_of(v: &Value) -> Vec<String> {
    ["riotId", "riotIdGameName", "summonerName"]
        .iter()
        .filter_map(|k| v.get(*k).and_then(Value::as_str))
        .map(norm)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Turns raw Live Client events into this player's highlights.
#[derive(Default)]
pub struct MatchTracker {
    me: HashSet<String>,
    /// normalized name -> team ("ORDER" | "CHAOS")
    teams: HashMap<String, String>,
    pub my_team: Option<String>,
    pub champion: Option<String>,
    /// Data Dragon key, e.g. "MonkeyKing".
    pub champion_key: Option<String>,
    pub kda: Kda,
    pub game_mode: Option<String>,
    pub result: Option<String>,
    pub ult_level: u32,
    seen: HashSet<i64>,
}

impl MatchTracker {
    pub fn update_active_player(&mut self, v: &Value) {
        for n in names_of(v) {
            self.me.insert(n);
        }
        self.ult_level = v.pointer("/abilities/R/abilityLevel").and_then(Value::as_u64).unwrap_or(self.ult_level as u64) as u32;
    }

    pub fn update_players(&mut self, players: &[Value]) {
        for p in players {
            let team = p.get("team").and_then(Value::as_str).unwrap_or("").to_string();
            let names = names_of(p);
            let is_me = names.iter().any(|n| self.me.contains(n));
            for n in &names {
                self.teams.insert(n.clone(), team.clone());
            }
            if is_me {
                self.my_team = Some(team.clone());
                self.champion = p.get("championName").and_then(Value::as_str).map(str::to_string);
                self.champion_key = p
                    .get("rawChampionName")
                    .and_then(Value::as_str)
                    .and_then(|r| r.rsplit('_').next())
                    .map(str::to_string)
                    .or_else(|| self.champion.clone().map(|c| c.replace([' ', '\'', '.'], "")));
                if let Some(s) = p.get("scores") {
                    let g = |k: &str| s.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
                    self.kda = Kda { kills: g("kills"), deaths: g("deaths"), assists: g("assists") };
                }
            }
        }
    }

    fn is_me(&self, name: &str) -> bool {
        !name.is_empty() && self.me.contains(&norm(name))
    }

    fn is_ally(&self, name: &str) -> bool {
        match (&self.my_team, self.teams.get(&norm(name))) {
            (Some(mine), Some(t)) => mine == t,
            _ => false,
        }
    }

    /// Structures are named e.g. "Turret_T2_C_05_A" / "Barracks_T1_L1": T1 belongs to ORDER, T2 to CHAOS.
    fn is_enemy_structure(&self, name: &str) -> bool {
        let owner = if name.contains("_T1_") {
            "ORDER"
        } else if name.contains("_T2_") {
            "CHAOS"
        } else {
            return false;
        };
        self.my_team.as_deref().is_some_and(|t| t != owner)
    }

    pub fn ingest(&mut self, raw: &[Value]) -> Vec<GameEvent> {
        let mut out = Vec::new();
        let mut first_blood_time: Option<f64> = None;
        for e in raw {
            let id = e.get("EventID").and_then(Value::as_i64).unwrap_or(-1);
            if !self.seen.insert(id) {
                continue;
            }
            let name = e.get("EventName").and_then(Value::as_str).unwrap_or("");
            let t = e.get("EventTime").and_then(Value::as_f64).unwrap_or(0.0);
            let s = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let mut push = |kind: &str, label: String, multi: Option<u8>| {
                out.push(GameEvent { kind: kind.into(), game_time: t, label, multi });
            };
            match name {
                "FirstBlood" if self.is_me(&s("Recipient")) => first_blood_time = Some(t),
                "ChampionKill" => {
                    let killer = s("KillerName");
                    let victim = s("VictimName");
                    let assisters: Vec<String> = e
                        .get("Assisters")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                        .unwrap_or_default();
                    if self.is_me(&killer) {
                        push("kill", format!("Killed {victim}"), None);
                    } else if self.is_me(&victim) {
                        push("death", format!("Killed by {}", pretty_killer(&killer)), None);
                    } else if assisters.iter().any(|a| self.is_me(a)) {
                        push("assist", format!("Assist on {victim}"), None);
                    }
                }
                "Multikill" if self.is_me(&s("KillerName")) => {
                    let n = e.get("KillStreak").and_then(Value::as_u64).unwrap_or(2).clamp(2, 5) as u8;
                    let label = ["", "", "Double Kill", "Triple Kill", "Quadra Kill", "Penta Kill"][n as usize];
                    push("multikill", label.to_string(), Some(n));
                }
                "TurretKilled" if self.is_enemy_structure(&s("TurretKilled")) => push("turret", "Turret destroyed".into(), None),
                "InhibKilled" if self.is_enemy_structure(&s("InhibKilled")) => push("inhibitor", "Inhibitor destroyed".into(), None),
                "DragonKill" if self.is_ally(&s("KillerName")) => {
                    let ty = s("DragonType");
                    let what = if ty.is_empty() { "Dragon".to_string() } else { format!("{ty} Drake") };
                    push("dragon", stolen(&s("Stolen"), what), None);
                }
                "HeraldKill" if self.is_ally(&s("KillerName")) => push("herald", stolen(&s("Stolen"), "Rift Herald".into()), None),
                "BaronKill" if self.is_ally(&s("KillerName")) => push("baron", stolen(&s("Stolen"), "Baron Nashor".into()), None),
                "GameEnd" => {
                    let r = s("Result");
                    self.result = Some(if r.eq_ignore_ascii_case("win") { "win".into() } else { "loss".into() });
                    if r.eq_ignore_ascii_case("win") {
                        push("victory", "Victory".into(), None);
                    }
                }
                _ => {}
            }
        }
        if let Some(t) = first_blood_time {
            if let Some(k) = out.iter_mut().find(|e| e.kind == "kill" && (e.game_time - t).abs() < 1.0) {
                k.label = format!("First Blood - {}", k.label.trim_start_matches("Killed "));
            }
        }
        out
    }
}

fn stolen(flag: &str, what: String) -> String {
    if flag.eq_ignore_ascii_case("true") { format!("{what} (stolen)") } else { what }
}

fn pretty_killer(name: &str) -> String {
    if name.starts_with("Turret_") {
        "a turret".into()
    } else if name.contains("Minion") {
        "minions".into()
    } else if name.is_empty() {
        "the enemy".into()
    } else {
        name.to_string()
    }
}

/// Data Dragon helpers (champion portraits and splash art), cached on disk.
pub mod art {
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::time::Duration;

    pub fn cache_dir() -> PathBuf {
        let d = crate::paths::config_dir().join("art");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    fn client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder().timeout(Duration::from_secs(15)).build().expect("http")
    }

    fn version() -> Option<String> {
        static V: OnceLock<Option<String>> = OnceLock::new();
        V.get_or_init(|| {
            let v: serde_json::Value = client().get("https://ddragon.leagueoflegends.com/api/versions.json").send().ok()?.json().ok()?;
            v.get(0)?.as_str().map(str::to_string)
        })
        .clone()
    }

    fn fetch(url: &str, path: &PathBuf) -> bool {
        if path.exists() {
            return true;
        }
        let Ok(resp) = client().get(url).send() else { return false };
        if !resp.status().is_success() {
            return false;
        }
        let Ok(bytes) = resp.bytes() else { return false };
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, path).is_ok()
    }

    /// Champion key -> name in Simplified Chinese (the champion's own name, e.g. "泰达米尔", which is
    /// Data Dragon's `title` in zh_CN). Cached per game version; the newest cache serves offline.
    pub fn names_zh() -> Option<std::collections::HashMap<String, String>> {
        let dir = cache_dir();
        let path = version().map(|v| dir.join(format!("champions-{v}.zh_CN.json")));
        if let Some(p) = &path {
            fetch(&format!("https://ddragon.leagueoflegends.com/cdn/{}/data/zh_CN/champion.json", version()?), p);
        }
        let file = path.filter(|p| p.exists()).or_else(|| {
            let mut cached: Vec<PathBuf> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".zh_CN.json")).collect();
            cached.sort();
            cached.pop()
        })?;
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
        Some(
            v.get("data")?
                .as_object()?
                .iter()
                .filter_map(|(key, c)| Some((key.clone(), c.get("title")?.as_str()?.to_string())))
                .collect(),
        )
    }

    /// Download the portrait and splash for a champion key; returns true when both exist.
    pub fn ensure(key: &str) -> bool {
        let dir = cache_dir();
        let icon = dir.join(format!("{key}.png"));
        let splash = dir.join(format!("{key}_splash.jpg"));
        let icon_ok = icon.exists() || version().is_some_and(|v| fetch(&format!("https://ddragon.leagueoflegends.com/cdn/{v}/img/champion/{key}.png"), &icon));
        let splash_ok = fetch(&format!("https://ddragon.leagueoflegends.com/cdn/img/champion/splash/{key}_0.jpg"), &splash);
        icon_ok && splash_ok
    }
}
