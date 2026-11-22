//! In-app streaming + subtitles commands (see `popcorntime-stream`).
//!
//! The engine starts lazily on the first `stream_play`; its cache lives in
//! `<app cache>/stream` and is wiped on app start, on stop and on exit.

use crate::error::Error;
use anyhow::Context;
use popcorntime_stream::{subtitles::SubtitleTrack, StreamEngine, StreamInfo, StreamStatus};
use std::{path::PathBuf, sync::Arc};
use tauri::{Manager, State};
use tokio::sync::OnceCell;
use tracing::instrument;

pub struct StreamState {
  dir: PathBuf,
  engine: OnceCell<Arc<StreamEngine>>,
}

impl StreamState {
  pub fn new(app_cache_dir: &std::path::Path) -> Self {
    let dir = app_cache_dir.join("stream");
    let _ = std::fs::remove_dir_all(&dir);
    Self {
      dir,
      engine: OnceCell::new(),
    }
  }

  pub fn dir(&self) -> &std::path::Path {
    &self.dir
  }

  async fn engine(&self) -> anyhow::Result<Arc<StreamEngine>> {
    self
      .engine
      .get_or_try_init(|| StreamEngine::start(self.dir.clone()))
      .await
      .cloned()
  }
}

/// Remove the stream cache (used on app exit).
pub fn wipe_on_exit(app: &tauri::AppHandle) {
  if let Some(state) = app.try_state::<StreamState>() {
    let _ = std::fs::remove_dir_all(state.dir());
  }
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(state), err(Debug))]
pub async fn stream_play(
  state: State<'_, StreamState>,
  magnet: String,
) -> Result<StreamInfo, Error> {
  let run = async {
    anyhow::ensure!(magnet.starts_with("magnet:?"), "not a magnet link");
    state.engine().await?.play(&magnet).await
  };
  run.await.map_err(Into::into)
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(state), err(Debug))]
pub async fn stream_stop(state: State<'_, StreamState>) -> Result<(), Error> {
  if let Some(engine) = state.engine.get() {
    engine.stop().await?;
  }
  Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub async fn stream_status(state: State<'_, StreamState>) -> Result<Option<StreamStatus>, Error> {
  match state.engine.get() {
    Some(engine) => Ok(engine.status().await?),
    None => Ok(None),
  }
}

/// Subtitles for an IMDb id. When something is streaming, the OpenSubtitles
/// hash of that file is sent so exact-release subtitles rank first.
#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(state), err(Debug))]
pub async fn subtitles_list(
  state: State<'_, StreamState>,
  imdb_code: String,
) -> Result<Vec<SubtitleTrack>, Error> {
  let video = match state.engine.get() {
    Some(engine) => engine.current_osdb_hash().await.ok(),
    None => None,
  };
  Ok(popcorntime_stream::subtitles::list(&imdb_code, video).await?)
}

/// One subtitle as WebVTT text (the player turns it into a <track>).
#[tauri::command(async)]
#[specta::specta]
#[instrument(err(Debug))]
pub async fn subtitle_vtt(url: String) -> Result<String, Error> {
  Ok(popcorntime_stream::subtitles::fetch_vtt(&url).await?)
}

/// Hand the current stream to VLC (for mkv/x265/DTS the WebView can't play),
/// with the chosen subtitle written next to the cache.
#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(state, vtt), err(Debug))]
pub async fn open_in_vlc(
  state: State<'_, StreamState>,
  url: String,
  vtt: Option<String>,
) -> Result<(), Error> {
  launch_vlc(state.dir(), &url, vtt).await.map_err(Into::into)
}

async fn launch_vlc(dir: &std::path::Path, url: &str, vtt: Option<String>) -> anyhow::Result<()> {
  anyhow::ensure!(url.starts_with("http://127.0.0.1:"), "only local streams");
  let vlc = PathBuf::from("/Applications/VLC.app");
  anyhow::ensure!(vlc.exists(), "VLC is not installed in /Applications");
  let mut args: Vec<String> = vec![
    "-n".into(),
    "-a".into(),
    vlc.display().to_string(),
    "--args".into(),
    url.to_string(),
  ];
  if let Some(vtt) = vtt.filter(|v| !v.is_empty()) {
    tokio::fs::create_dir_all(dir).await.ok();
    let sub = dir.join("subtitle.vtt");
    tokio::fs::write(&sub, vtt)
      .await
      .context("failed to write subtitle")?;
    args.push(format!("--sub-file={}", sub.display()));
  }
  let status = std::process::Command::new("/usr/bin/open")
    .args(&args)
    .status()
    .context("failed to launch VLC")?;
  anyhow::ensure!(status.success(), "VLC exited with {status}");
  Ok(())
}

/// Hand a magnet link to the system torrent app (e.g. Folx).
#[tauri::command(async)]
#[specta::specta]
#[instrument(err(Debug))]
pub async fn open_magnet(magnet: String) -> Result<(), Error> {
  let run = async {
    anyhow::ensure!(magnet.starts_with("magnet:?"), "not a magnet link");
    let status = std::process::Command::new("/usr/bin/open")
      .arg(&magnet)
      .status()
      .context("failed to run open")?;
    anyhow::ensure!(status.success(), "no app handles magnet links");
    Ok(())
  };
  run.await.map_err(Into::into)
}
