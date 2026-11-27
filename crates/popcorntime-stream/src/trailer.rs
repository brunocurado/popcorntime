//! YouTube trailers inside the app.
//!
//! YouTube refuses embeds without an http(s) Referer ("Error 153"), and the
//! WebView's own origin is `tauri://localhost`. So a one-line page with the
//! embed is served from 127.0.0.1 and the dialog iframes that page.

use anyhow::{Context, Result};

pub fn valid_youtube_id(id: &str) -> bool {
  (6..=20).contains(&id.len())
    && id
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn trailer_page(id: &str) -> String {
  format!(
    r#"<!doctype html><html><head><meta charset="utf-8"><meta name="referrer" content="strict-origin-when-cross-origin">
<style>html,body{{margin:0;height:100%;background:#000}}iframe{{border:0;width:100%;height:100%}}</style></head>
<body><iframe allow="autoplay; encrypted-media; fullscreen; picture-in-picture" allowfullscreen
referrerpolicy="strict-origin-when-cross-origin"
src="https://www.youtube-nocookie.com/embed/{id}?autoplay=1&rel=0&playsinline=1&modestbranding=1"></iframe></body></html>"#
  )
}

/// Start the trailer page server on 127.0.0.1 (random port).
pub async fn serve() -> Result<u16> {
  use axum::{extract::Path, http::StatusCode, response::Html};
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .context("failed to bind trailer server")?;
  let port = listener.local_addr()?.port();
  let app = axum::Router::new().route(
    "/trailer/{id}",
    axum::routing::get(|Path(id): Path<String>| async move {
      if valid_youtube_id(&id) {
        Ok(Html(trailer_page(&id)))
      } else {
        Err(StatusCode::NOT_FOUND)
      }
    }),
  );
  tokio::spawn(async move {
    if let Err(e) = axum::serve(listener, app).await {
      tracing::error!("trailer server stopped: {e:#}");
    }
  });
  Ok(port)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn validates_ids() {
    assert!(valid_youtube_id("FVI84Dfx2-I"));
    assert!(!valid_youtube_id("x\"><script>"));
    assert!(!valid_youtube_id("abc"));
  }

  #[tokio::test]
  async fn serves_embed_page() {
    let port = serve().await.unwrap();
    let ok = reqwest::get(format!("http://127.0.0.1:{port}/trailer/FVI84Dfx2-I"))
      .await
      .unwrap();
    assert_eq!(ok.status(), 200);
    assert!(
      ok.text()
        .await
        .unwrap()
        .contains("youtube-nocookie.com/embed/FVI84Dfx2-I")
    );
    let bad = reqwest::get(format!("http://127.0.0.1:{port}/trailer/%3Cscript%3E"))
      .await
      .unwrap();
    assert_eq!(bad.status(), 404);
  }
}
