//! In-app torrent streaming (librqbit) plus subtitles (OpenSubtitles).
//!
//! One movie at a time: `play` drops whatever was streaming before (files
//! included), so the cache never holds more than the current film. The cache
//! dir is wiped on start and on `stop`.
//!
//! Containers the WebView can't open (mkv, avi…) are remuxed on the fly by
//! ffmpeg into HLS (fMP4 segments) under `<cache>/hls`, served on a second
//! local port; video is copied when it is H.264/HEVC, audio becomes AAC.

pub mod opensubtitles;
pub mod remux;
pub mod subtitles;
pub mod trailer;
pub mod ytssubs;

use anyhow::{Context, Result};
use librqbit::{
  AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, DhtSessionConfig, Session,
  SessionOptions,
  api::TorrentIdOrHash,
  http_api::{HttpApi, HttpApiOptions},
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};
use tokio::{
  io::{AsyncReadExt, AsyncSeekExt},
  sync::Mutex,
};

/// Hard ceiling for one streamed file (agreed cache budget).
pub const CACHE_CAP_BYTES: u64 = 8 * 1024 * 1024 * 1024;

const VIDEO_EXT: &[&str] = &["mp4", "m4v", "mkv", "webm", "mov", "avi"];
/// Containers WKWebView's <video> can open; the rest go to VLC.
const IN_APP_EXT: &[&str] = &["mp4", "m4v", "mov", "webm"];

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct StreamInfo {
  pub torrent_id: u32,
  pub file_index: u32,
  pub file_name: String,
  /// Bytes, as f64 so it survives the JS number boundary for big files.
  pub file_size: f64,
  /// Raw file over HTTP (Range-capable) — what VLC and subtitles hashing use.
  pub url: String,
  /// What the in-app <video> should load: the raw file for mp4/mov/webm,
  /// the HLS playlist when the file was remuxed, `None` when neither works.
  pub play_url: Option<String>,
  /// The in-app player can show it (natively or through the remux).
  pub in_app: bool,
  /// True when `play_url` is the ffmpeg HLS remux.
  pub remuxed: bool,
  /// Seconds skipped at the start of the remux (resume); the player adds it
  /// to `currentTime` to get the film position.
  pub time_offset: f64,
  /// Film length in seconds when known (from ffprobe), else 0.
  pub duration: f64,
  /// Why the remux didn't start (dead swarm, missing ffmpeg…), for the UI.
  pub remux_error: Option<String>,
}

/// Which file to stream inside a multi-file torrent (season packs).
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct FileHint {
  pub file_idx: Option<u32>,
  pub file_name: Option<String>,
  pub season: Option<u32>,
  pub episode: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct StreamStatus {
  pub state: String,
  pub progress_bytes: f64,
  pub total_bytes: f64,
  pub download_mbps: f64,
  pub peers: u32,
}

pub struct StreamEngine {
  api: Api,
  session: Arc<Session>,
  port: u16,
  hls_port: u16,
  dir: PathBuf,
  current: Mutex<Option<(usize, u32)>>,
  remux: Mutex<Option<remux::Remux>>,
}

fn ext_of(name: &str) -> String {
  name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}

fn is_video(name: &str) -> bool {
  VIDEO_EXT.contains(&ext_of(name).as_str()) && !name.to_ascii_lowercase().contains("sample")
}

fn basename(name: &str) -> &str {
  name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// Does `name` look like episode `season`x`episode` (S01E02, 1x02, s01.e02)?
pub fn matches_episode(name: &str, season: u32, episode: u32) -> bool {
  let n = basename(name).to_ascii_lowercase();
  let digit_after = |rest: &str| rest.chars().next().is_some_and(|c| c.is_ascii_digit());
  let candidates = [
    format!("s{season:02}e{episode:02}"),
    format!("s{season}e{episode:02}"),
    format!("s{season:02}.e{episode:02}"),
    format!("{season}x{episode:02}"),
  ];
  candidates.iter().any(|c| {
    n.match_indices(c.as_str()).any(|(i, _)| {
      let before_ok =
        c.starts_with('s') || i == 0 || !n[..i].ends_with(|ch: char| ch.is_ascii_digit());
      before_ok && !digit_after(&n[i + c.len()..])
    })
  })
}

/// Pick the largest video file; `None` when the torrent has no video.
pub fn pick_video(files: &[(String, u64)]) -> Option<usize> {
  pick_file(files, &FileHint::default())
}

/// Choose the file to stream: exact name from the source, then the episode
/// tag, then the source's index, then the largest video.
pub fn pick_file(files: &[(String, u64)], hint: &FileHint) -> Option<usize> {
  let videos = || files.iter().enumerate().filter(|(_, (n, _))| is_video(n));
  if let Some(want) = hint.file_name.as_deref().filter(|n| !n.is_empty()) {
    let want = basename(want).to_ascii_lowercase();
    if let Some((i, _)) = videos().find(|(_, (n, _))| basename(n).to_ascii_lowercase() == want) {
      return Some(i);
    }
  }
  if let (Some(s), Some(e)) = (hint.season, hint.episode) {
    if let Some((i, _)) = videos()
      .filter(|(_, (n, _))| matches_episode(n, s, e))
      .max_by_key(|(_, (_, len))| *len)
    {
      return Some(i);
    }
    // a pack without our episode: refuse rather than play the wrong one
    if videos().count() > 1 {
      return None;
    }
  }
  if let Some(i) = hint.file_idx.map(|i| i as usize)
    && files.get(i).is_some_and(|(n, _)| is_video(n))
  {
    return Some(i);
  }
  videos().max_by_key(|(_, (_, len))| *len).map(|(i, _)| i)
}

pub fn plays_in_app(name: &str) -> bool {
  IN_APP_EXT.contains(&ext_of(name).as_str())
}

async fn wipe(dir: &PathBuf) {
  let _ = tokio::fs::remove_dir_all(dir).await;
  let _ = tokio::fs::create_dir_all(dir).await;
}

impl StreamEngine {
  /// Start the session and its read-only HTTP stream server on 127.0.0.1.
  pub async fn start(dir: PathBuf) -> Result<Arc<Self>> {
    wipe(&dir).await;
    let session = Session::new_with_opts(
      dir.clone(),
      SessionOptions {
        fastresume: false,
        persistence: None,
        // random DHT port and no state outside the cache dir
        dht: Some(DhtSessionConfig {
          persistence: None,
          ..Default::default()
        }),
        ..Default::default()
      },
    )
    .await
    .context("failed to start torrent session")?;
    let api = Api::new(session.clone(), None, None);
    let listener = librqbit_dualstack_sockets::socket::MaybeDualstackSocket::bind_tcp(
      "127.0.0.1:0".parse()?,
      librqbit_dualstack_sockets::BindOpts {
        request_dualstack: false,
        ..Default::default()
      },
    )
    .context("failed to bind stream server")?;
    let port = listener.socket().local_addr()?.port();
    let http = HttpApi::new(
      api.clone(),
      Some(HttpApiOptions {
        read_only: true,
        ..Default::default()
      }),
    );
    tokio::spawn(async move {
      if let Err(e) = http.make_http_api_and_run(listener, None).await {
        tracing::error!("stream server stopped: {e:#}");
      }
    });
    let hls_port = remux::serve_dir(dir.join("hls")).await?;
    tracing::info!(port, hls_port, dir = %dir.display(), "stream engine ready");
    Ok(Arc::new(Self {
      api,
      session,
      port,
      hls_port,
      dir,
      current: Mutex::new(None),
      remux: Mutex::new(None),
    }))
  }

  pub fn port(&self) -> u16 {
    self.port
  }

  /// Drop the current torrent and its files. With `only`, nothing happens
  /// unless that torrent is still the current one (a late stop from a closed
  /// player must not kill the next episode).
  pub async fn stop_if(&self, only: Option<u32>) -> Result<()> {
    {
      let cur = self.current.lock().await;
      if let (Some(want), Some((id, _))) = (only, *cur)
        && want as usize != id
      {
        return Ok(());
      }
    }
    self.stop().await
  }

  /// Drop the current torrent, the remux and every cached file.
  pub async fn stop(&self) -> Result<()> {
    if let Some(r) = self.remux.lock().await.take() {
      r.kill().await;
    }
    if let Some((id, _)) = self.current.lock().await.take() {
      self
        .session
        .delete(TorrentIdOrHash::Id(id), true)
        .await
        .context("failed to drop torrent")?;
    }
    wipe(&self.dir).await;
    Ok(())
  }

  /// Fetch metadata, keep only the chosen video file and start streaming it;
  /// remux to HLS when the WebView can't open the container.
  pub async fn play(
    &self,
    magnet: &str,
    hint: &FileHint,
    start_at: Option<f64>,
  ) -> Result<StreamInfo> {
    self.stop().await?;
    let listed = self
      .session
      .add_torrent(
        AddTorrent::from_url(magnet),
        Some(AddTorrentOptions {
          list_only: true,
          ..Default::default()
        }),
      )
      .await
      .context("could not read torrent metadata (no peers?)")?;
    let AddTorrentResponse::ListOnly(list) = listed else {
      anyhow::bail!("unexpected response while listing torrent");
    };
    let files: Vec<(String, u64)> = list
      .info
      .iter_file_details()
      .map(|d| (d.filename.to_string(), d.len))
      .collect();
    let idx = pick_file(&files, hint).context(match (hint.season, hint.episode) {
      (Some(s), Some(e)) => format!("this torrent has no file for S{s:02}E{e:02}"),
      _ => "this torrent has no video file".to_string(),
    })?;
    let (name, size) = files[idx].clone();
    anyhow::ensure!(
      size <= CACHE_CAP_BYTES,
      "file is {:.1} GB, above the {} GB cache cap — pick a lighter version",
      size as f64 / 1e9,
      CACHE_CAP_BYTES / (1024 * 1024 * 1024)
    );
    let added = self
      .session
      .add_torrent(
        AddTorrent::from_url(magnet),
        Some(AddTorrentOptions {
          only_files: Some(vec![idx]),
          overwrite: true,
          ..Default::default()
        }),
      )
      .await
      .context("failed to start torrent")?;
    let id = match added {
      AddTorrentResponse::Added(id, _) | AddTorrentResponse::AlreadyManaged(id, _) => id,
      AddTorrentResponse::ListOnly(_) => anyhow::bail!("torrent was only listed"),
    };
    *self.current.lock().await = Some((id, idx as u32));
    let short = basename(&name).to_string();
    let url = format!(
      "http://127.0.0.1:{}/torrents/{id}/stream/{idx}/{}",
      self.port,
      url::form_urlencoded::byte_serialize(short.as_bytes()).collect::<String>()
    );
    let mut info = StreamInfo {
      torrent_id: id as u32,
      file_index: idx as u32,
      play_url: None,
      in_app: false,
      remuxed: false,
      time_offset: 0.0,
      duration: 0.0,
      remux_error: None,
      url,
      file_name: short,
      file_size: size as f64,
    };
    if plays_in_app(&info.file_name) {
      info.play_url = Some(info.url.clone());
      info.in_app = true;
      return Ok(info);
    }
    // mkv/avi…: remux; if ffmpeg is missing or fails, the UI offers VLC.
    match remux::Remux::start(
      &info.url,
      &self.dir.join("hls").join(id.to_string()),
      start_at.unwrap_or(0.0),
    )
    .await
    {
      Ok(r) => {
        info.play_url = Some(format!(
          "http://127.0.0.1:{}/{id}/index.m3u8",
          self.hls_port
        ));
        info.in_app = true;
        info.remuxed = true;
        info.time_offset = r.offset();
        info.duration = r.duration();
        *self.remux.lock().await = Some(r);
      }
      Err(e) => {
        tracing::warn!("remux unavailable, VLC fallback: {e:#}");
        info.remux_error = Some(format!("{e:#}"));
      }
    }
    Ok(info)
  }

  pub async fn status(&self) -> Result<Option<StreamStatus>> {
    let Some((id, _)) = *self.current.lock().await else {
      return Ok(None);
    };
    let s = self.api.api_stats_v1(TorrentIdOrHash::Id(id))?;
    let (mbps, peers) = s
      .live
      .as_ref()
      .map(|l| (l.download_speed.mbps, l.snapshot.peer_stats.live))
      .unwrap_or((0.0, 0));
    Ok(Some(StreamStatus {
      state: s.state.to_string(),
      progress_bytes: s.progress_bytes as f64,
      total_bytes: s.total_bytes as f64,
      download_mbps: mbps,
      peers,
    }))
  }

  /// OpenSubtitles hash of the current file (size + first/last 64 KiB).
  pub async fn current_osdb_hash(&self) -> Result<(String, u64)> {
    let (id, idx) = self.current.lock().await.context("nothing is streaming")?;
    let mut stream = self
      .api
      .api_stream(TorrentIdOrHash::Id(id), idx as usize)
      .await?;
    let len = stream.len();
    let chunk = 65536u64.min(len);
    let mut head = vec![0u8; chunk as usize];
    stream.read_exact(&mut head).await?;
    stream.seek(std::io::SeekFrom::Start(len - chunk)).await?;
    let mut tail = vec![0u8; chunk as usize];
    stream.read_exact(&mut tail).await?;
    Ok((subtitles::osdb_hash(len, &head, &tail), len))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn files(names: &[(&str, u64)]) -> Vec<(String, u64)> {
    names.iter().map(|(n, l)| (n.to_string(), *l)).collect()
  }

  #[test]
  fn picks_episode_inside_a_season_pack() {
    let f = files(&[
      ("Show S01/Show.S01E01.1080p.mkv", 900),
      ("Show S01/Show.S01E02.1080p.mkv", 800),
      ("Show S01/Show.S01E12.1080p.mkv", 1000),
      ("Show S01/sample.mkv", 5000),
    ]);
    let ep = |s, e| FileHint {
      season: Some(s),
      episode: Some(e),
      ..Default::default()
    };
    assert_eq!(pick_file(&f, &ep(1, 2)), Some(1));
    assert_eq!(pick_file(&f, &ep(1, 1)), Some(0)); // not S01E12
    assert_eq!(pick_file(&f, &ep(1, 3)), None); // absent: never the wrong one
    // no hint: largest real video (sample excluded)
    assert_eq!(pick_video(&f), Some(2));
  }

  #[test]
  fn exact_name_and_index_hints_win() {
    let f = files(&[("a.mkv", 10), ("b/Show.S02E05.mkv", 5), ("c.nfo", 1)]);
    let by_name = FileHint {
      file_name: Some("Show.S02E05.mkv".into()),
      ..Default::default()
    };
    assert_eq!(pick_file(&f, &by_name), Some(1));
    let by_idx = FileHint {
      file_idx: Some(1),
      ..Default::default()
    };
    assert_eq!(pick_file(&f, &by_idx), Some(1));
    let bad_idx = FileHint {
      file_idx: Some(2),
      ..Default::default()
    };
    assert_eq!(pick_file(&f, &bad_idx), Some(0));
  }

  #[test]
  fn episode_tags() {
    assert!(matches_episode("Breaking.Bad.S01E02.mkv", 1, 2));
    assert!(matches_episode("breaking bad 1x02 cat.mkv", 1, 2));
    assert!(!matches_episode("Breaking.Bad.S01E20.mkv", 1, 2));
    assert!(!matches_episode("Show.11x02.mkv", 1, 2));
  }
}
