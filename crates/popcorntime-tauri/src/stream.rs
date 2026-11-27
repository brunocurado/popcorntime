//! In-app streaming + subtitles commands (see `popcorntime-stream`).
//!
//! The engine starts lazily on the first `stream_play`; its cache lives in
//! `<app cache>/stream` and is wiped on app start, on stop and on exit.

use crate::error::Error;
use anyhow::Context;
use popcorntime_stream::{
  opensubtitles::{
    Credentials, KeychainStore, MemoryStore, OpenSubtitles, OpenSubtitlesStatus, Query,
    SecretStore, TRACK_SCHEME,
  },
  subtitles::{SubtitleTrack, PREFERRED_LANGS},
  FileHint, StreamEngine, StreamInfo, StreamStatus,
};
use std::{path::PathBuf, sync::Arc};
use tauri::{Manager, State};
use tokio::sync::OnceCell;
use tracing::instrument;

pub struct StreamState {
  dir: PathBuf,
  engine: OnceCell<Arc<StreamEngine>>,
  trailer_port: OnceCell<u16>,
  opensubs: OpenSubtitles,
}

impl StreamState {
  pub fn new(app_cache_dir: &std::path::Path) -> Self {
    let dir = app_cache_dir.join("stream");
    let _ = std::fs::remove_dir_all(&dir);
    let store: Box<dyn SecretStore> = match KeychainStore::new() {
      Ok(k) => Box::new(k),
      Err(e) => {
        tracing::warn!("keychain unavailable, OpenSubtitles credentials kept in memory: {e:#}");
        Box::<MemoryStore>::default()
      }
    };
    Self {
      dir,
      engine: OnceCell::new(),
      trailer_port: OnceCell::new(),
      opensubs: OpenSubtitles::new(store),
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
  hint: Option<FileHint>,
  start_at: Option<f64>,
) -> Result<StreamInfo, Error> {
  let run = async {
    anyhow::ensure!(magnet.starts_with("magnet:?"), "not a magnet link");
    state
      .engine()
      .await?
      .play(&magnet, &hint.unwrap_or_default(), start_at)
      .await
  };
  run.await.map_err(Into::into)
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(state), err(Debug))]
pub async fn stream_stop(
  state: State<'_, StreamState>,
  torrent_id: Option<u32>,
) -> Result<(), Error> {
  if let Some(engine) = state.engine.get() {
    engine.stop_if(torrent_id).await?;
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
  season: Option<u32>,
  episode: Option<u32>,
) -> Result<Vec<SubtitleTrack>, Error> {
  let video = match state.engine.get() {
    Some(engine) => engine.current_osdb_hash().await.ok(),
    None => None,
  };
  let ep = season.zip(episode);
  // Free sources together: the Stremio addon, plus yts-subs.com for movies.
  let addon_fut = popcorntime_stream::subtitles::list_for(&imdb_code, ep, video.clone());
  let ytssubs_fut = async {
    if ep.is_some() {
      return Ok(Vec::new()); // yts-subs has no series
    }
    popcorntime_stream::ytssubs::list(&imdb_code, PREFERRED_LANGS).await
  };
  let (addon, ytssubs) = tokio::join!(addon_fut, ytssubs_fut);
  let addon = addon.unwrap_or_else(|e| {
    tracing::warn!("subtitle addon failed: {e:#}");
    Vec::new()
  });
  let ytssubs = ytssubs.unwrap_or_else(|e| {
    tracing::warn!("yts-subs failed: {e:#}");
    Vec::new()
  });
  let addon = merge_by_language(addon, ytssubs);
  let has_pt = addon.iter().any(|t| t.lang == "por" || t.lang == "pob");
  if has_pt {
    return Ok(addon);
  }
  // No Portuguese from the free addon: ask OpenSubtitles.com, if configured.
  // English too when the free sources have none: it's the next best choice.
  let mut languages = vec!["pt-pt".to_string(), "pt-br".to_string()];
  if !addon.iter().any(|t| t.lang == "eng") {
    languages.push("en".to_string());
  }
  let query = Query {
    imdb: imdb_code.clone(),
    episode: ep,
    moviehash: video.map(|(h, _)| h),
    languages,
  };
  match state.opensubs.search(&query, PREFERRED_LANGS).await {
    Ok(Some(official)) => Ok(merge_by_language(official, addon)),
    Ok(None) => Ok(addon),
    Err(e) => {
      tracing::warn!("opensubtitles.com fallback failed: {e:#}");
      Ok(addon)
    }
  }
}

/// One subtitle as WebVTT text (the player turns it into a <track>).
#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(state), err(Debug))]
pub async fn subtitle_vtt(state: State<'_, StreamState>, url: String) -> Result<String, Error> {
  if url.starts_with(TRACK_SCHEME) {
    return Ok(state.opensubs.fetch_vtt(&url).await?);
  }
  if url.starts_with(popcorntime_stream::ytssubs::TRACK_SCHEME) {
    return Ok(popcorntime_stream::ytssubs::fetch_vtt(&url).await?);
  }
  Ok(popcorntime_stream::subtitles::fetch_vtt(&url).await?)
}

/// OpenSubtitles.com fallback: is it configured, as whom, downloads left.
#[tauri::command(async)]
#[specta::specta]
pub async fn opensubtitles_status(
  state: State<'_, StreamState>,
) -> Result<OpenSubtitlesStatus, Error> {
  Ok(state.opensubs.status().await)
}

/// Check the credentials against OpenSubtitles.com and keep them in the
/// Keychain. Nothing is stored if the check fails. Never logged.
#[tauri::command(async)]
#[specta::specta]
#[instrument(skip_all, err(Debug))]
pub async fn opensubtitles_save(
  state: State<'_, StreamState>,
  api_key: String,
  username: Option<String>,
  password: Option<String>,
) -> Result<OpenSubtitlesStatus, Error> {
  Ok(
    state
      .opensubs
      .save(Credentials {
        api_key,
        username,
        password,
      })
      .await?,
  )
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(state), err(Debug))]
pub async fn opensubtitles_clear(state: State<'_, StreamState>) -> Result<(), Error> {
  Ok(state.opensubs.clear().await?)
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

/// Local page that embeds the YouTube trailer (see `popcorntime_stream::trailer`).
#[tauri::command(async)]
#[specta::specta]
#[instrument(skip(state), err(Debug))]
pub async fn trailer_url(
  state: State<'_, StreamState>,
  youtube_id: String,
) -> Result<String, Error> {
  let run = async {
    anyhow::ensure!(
      popcorntime_stream::trailer::valid_youtube_id(&youtube_id),
      "invalid YouTube id"
    );
    let port = state
      .trailer_port
      .get_or_try_init(popcorntime_stream::trailer::serve)
      .await?;
    Ok(format!("http://127.0.0.1:{port}/trailer/{youtube_id}"))
  };
  run.await.map_err(Into::into)
}

/// Group tracks by preferred language (PT-PT, PT-BR, EN…), keeping each
/// list's own order inside a language: `first` before `second`.
pub fn merge_by_language(
  first: Vec<SubtitleTrack>,
  second: Vec<SubtitleTrack>,
) -> Vec<SubtitleTrack> {
  let rank = |l: &str| PREFERRED_LANGS.iter().position(|p| *p == l).unwrap_or(99);
  let mut all: Vec<(usize, usize, SubtitleTrack)> = first
    .into_iter()
    .chain(second)
    .enumerate()
    .map(|(i, t)| (rank(&t.lang), i, t))
    .collect();
  all.sort_by_key(|(r, i, _)| (*r, *i));
  all.into_iter().map(|(_, _, t)| t).collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn t(id: &str, lang: &str) -> SubtitleTrack {
    SubtitleTrack {
      id: id.into(),
      lang: lang.into(),
      label: id.into(),
      url: format!("https://x/{id}"),
    }
  }

  #[test]
  fn merges_sources_by_language_keeping_order() {
    let addon = vec![t("a-pob", "pob"), t("a-eng", "eng")];
    let ys = vec![t("y-por", "por"), t("y-pob", "pob"), t("y-eng", "eng")];
    let ids: Vec<_> = merge_by_language(addon, ys)
      .into_iter()
      .map(|t| t.id)
      .collect();
    assert_eq!(ids, ["y-por", "a-pob", "y-pob", "a-eng", "y-eng"]);
  }
}
