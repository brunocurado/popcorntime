//! In-app torrent streaming (librqbit) plus subtitles (OpenSubtitles).
//!
//! One movie at a time: `play` drops whatever was streaming before (files
//! included), so the cache never holds more than the current film. The cache
//! dir is wiped on start and on `stop`.

pub mod subtitles;

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
  pub url: String,
  /// Container the in-app player can try (mp4/m4v/mov/webm).
  pub in_app: bool,
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
  dir: PathBuf,
  current: Mutex<Option<(usize, u32)>>,
}

fn ext_of(name: &str) -> String {
  name.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}

/// Pick the largest video file; `None` when the torrent has no video.
pub fn pick_video(files: &[(String, u64)]) -> Option<usize> {
  files
    .iter()
    .enumerate()
    .filter(|(_, (n, _))| VIDEO_EXT.contains(&ext_of(n).as_str()))
    .max_by_key(|(_, (_, len))| *len)
    .map(|(i, _)| i)
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
    tracing::info!(port, dir = %dir.display(), "stream engine ready");
    Ok(Arc::new(Self {
      api,
      session,
      port,
      dir,
      current: Mutex::new(None),
    }))
  }

  pub fn port(&self) -> u16 {
    self.port
  }

  /// Drop the current torrent and its files.
  pub async fn stop(&self) -> Result<()> {
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

  /// Fetch metadata, keep only the main video file and start streaming it.
  pub async fn play(&self, magnet: &str) -> Result<StreamInfo> {
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
    let idx = pick_video(&files).context("this torrent has no video file")?;
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
    let short = name.rsplit('/').next().unwrap_or(&name).to_string();
    Ok(StreamInfo {
      torrent_id: id as u32,
      file_index: idx as u32,
      url: format!(
        "http://127.0.0.1:{}/torrents/{id}/stream/{idx}/{}",
        self.port,
        url::form_urlencoded::byte_serialize(short.as_bytes()).collect::<String>()
      ),
      in_app: plays_in_app(&short),
      file_name: short,
      file_size: size as f64,
    })
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
