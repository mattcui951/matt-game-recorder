//! Serves media files to the UI over 127.0.0.1 with HTTP range support, so the <video> element
//! can seek instantly in multi-GB recordings. Every URL carries a random per-run token.

use axum::extract::{Path as AxPath, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use std::path::{Component, Path, PathBuf};
use tower::ServiceExt;
use tower_http::services::ServeFile;

fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(base.join(rel))
}

async fn serve(AxPath((token, path)): AxPath<(String, String)>, req: Request) -> Response {
    let Some(core) = crate::core::get() else { return StatusCode::SERVICE_UNAVAILABLE.into_response() };
    if !core.media_url.read().contains(&token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (area, rest) = path.split_once('/').unwrap_or((path.as_str(), ""));
    #[cfg(debug_assertions)]
    if area == "dev" {
        let query = req.uri().query().unwrap_or("").to_string();
        return dev_api(core, rest, &query);
    }
    let base = match area {
        "media" => core.library.root(),
        "art" => crate::lol::art::cache_dir(),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let Some(file) = safe_join(&base, rest) else { return StatusCode::BAD_REQUEST.into_response() };
    match ServeFile::new(file).oneshot(req).await {
        Ok(mut r) => {
            r.headers_mut().insert("cache-control", "no-cache".parse().unwrap());
            r.headers_mut().insert("access-control-allow-origin", "*".parse().unwrap());
            r.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Debug builds only: JSON so the UI can be previewed in a plain browser, plus test hooks to
/// start/cancel imports (`dev/import-outplayed?<folder>`, `dev/import-library?<folder>`).
#[cfg(debug_assertions)]
fn dev_api(core: &std::sync::Arc<crate::core::Core>, what: &str, query: &str) -> Response {
    let folder = || {
        let decoded = query.replace("%20", " ").replace("%5C", "\\").replace("%3A", ":");
        std::path::PathBuf::from(decoded)
    };
    let started = |id: String| serde_json::to_value(serde_json::json!({ "job": id }));
    let body = match what {
        "import-outplayed" => started(crate::import::start(core, "outplayed", folder(), false, Default::default())),
        "import-outplayed-all" => started(crate::import::start(core, "outplayed", folder(), false, crate::import::Options { include_deleted: true, delete_sources: false })),
        "import-library" => started(crate::import::start(core, "library", folder(), false, Default::default())),
        "pause" | "resume" => {
            let kind = core.jobs.set_paused(&core.app, query, what == "pause");
            if kind.as_deref() == Some("import") {
                crate::import::set_pending_paused(what == "pause");
            }
            serde_json::to_value(kind)
        }
        "jobs" => serde_json::to_value(core.jobs.list()),
        // Delete a recording the way a user would: Recently deleted, then removed for good.
        "purge" => {
            let ok = core.library.soft_delete(query).and_then(|_| core.library.purge(query)).is_ok();
            core.library_changed();
            serde_json::to_value(ok)
        }
        "cancel" => {
            core.jobs.cancel(query);
            serde_json::to_value("ok")
        }
        "library" => serde_json::to_value(core.library.snapshot()),
        "settings" => serde_json::to_value(core.settings()),
        "status" => serde_json::to_value(core.status.read().clone()),
        "projects" => serde_json::to_value(crate::editor::list(&core.library.root())),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    match body {
        Ok(v) => ([("content-type", "application/json"), ("access-control-allow-origin", "*")], v.to_string()).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Bind and start the server; returns the base URL (".../<token>/").
pub fn start() -> anyhow::Result<String> {
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    std_listener.set_nonblocking(true)?;
    let port = std_listener.local_addr()?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let app = Router::new().route("/{token}/{*path}", get(serve));
    tauri::async_runtime::spawn(async move {
        match tokio::net::TcpListener::from_std(std_listener) {
            Ok(listener) => {
                if let Err(e) = axum::serve(listener, app).await {
                    log::error!("media server stopped: {e}");
                }
            }
            Err(e) => log::error!("media server failed: {e}"),
        }
    });
    Ok(format!("http://127.0.0.1:{port}/{token}/"))
}
