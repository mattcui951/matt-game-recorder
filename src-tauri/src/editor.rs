//! Video editor projects, stored as JSON in <media>/Projects.

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectClip {
    pub id: String,
    pub recording_id: String,
    pub start: f64,
    pub end: f64,
    /// fit | fill | stretch
    pub crop: String,
    /// percent
    pub volume: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    /// 16:9 | 9:16 | 1:1 | 4:5 | 21:9 | original
    pub aspect: String,
    pub volume: f64,
    pub protect: bool,
    pub watermark: bool,
    /// Recording ids imported into the project bin.
    pub media: Vec<String>,
    pub clips: Vec<ProjectClip>,
}

fn dir(root: &Path) -> PathBuf {
    let d = root.join("Projects");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn list(root: &Path) -> Vec<Project> {
    let mut v: Vec<Project> = std::fs::read_dir(dir(root))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|p: &Project| std::cmp::Reverse(p.updated_at));
    v
}

pub fn get(root: &Path, id: &str) -> Result<Project> {
    let bytes = std::fs::read(dir(root).join(format!("{id}.json"))).context(tr!("Project not found", "找不到该项目"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save(root: &Path, p: &mut Project) -> Result<()> {
    p.updated_at = Local::now();
    let path = dir(root).join(format!("{}.json", p.id));
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(p)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

pub fn create(root: &Path, media: Vec<String>, clips: Vec<ProjectClip>) -> Result<Project> {
    let n = list(root).len() + 1;
    let now = Local::now();
    let mut p = Project {
        id: uuid::Uuid::new_v4().simple().to_string(),
        name: format!("Project {n:02}"),
        created_at: now,
        updated_at: now,
        aspect: "16:9".into(),
        volume: 100.0,
        protect: true,
        watermark: false,
        media,
        clips,
    };
    save(root, &mut p)?;
    Ok(p)
}

pub fn delete(root: &Path, id: &str) -> Result<()> {
    std::fs::remove_file(dir(root).join(format!("{id}.json")))?;
    Ok(())
}

/// Recording ids that protected projects depend on.
pub fn protected_ids(root: &Path) -> Vec<String> {
    list(root).into_iter().filter(|p| p.protect).flat_map(|p| p.media.into_iter().chain(p.clips.into_iter().map(|c| c.recording_id))).collect()
}

/// Output frame for an aspect ratio, 1080 on the short side (or source size for "original").
pub fn frame_size(aspect: &str, source: Option<(u32, u32)>) -> (u32, u32) {
    match aspect {
        "9:16" => (1080, 1920),
        "1:1" => (1080, 1080),
        "4:5" => (1080, 1350),
        "21:9" => (2520, 1080),
        "original" => match source {
            Some((w, h)) if w > 0 && h > 0 => {
                let h2 = h.min(1440);
                ((w as u64 * h2 as u64 / h as u64) as u32 / 2 * 2, h2 / 2 * 2)
            }
            _ => (1920, 1080),
        },
        _ => (1920, 1080),
    }
}
