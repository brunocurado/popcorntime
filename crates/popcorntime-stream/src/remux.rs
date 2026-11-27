//! On-the-fly remux of containers WKWebView can't open (mkv, avi…) into HLS
//! with fMP4 segments, which Safari/WebKit plays natively.
//!
//! - video: copied when H.264 or HEVC (tagged `hvc1` for Apple decoders),
//!   otherwise re-encoded with the hardware `h264_videotoolbox` encoder;
//! - audio: copied when AAC, otherwise AAC stereo 192 kb/s (AC3/DTS/EAC3…);
//!   original-language track preferred over a Portuguese dub;
//! - subtitles dropped (the player overlays OpenSubtitles instead).
//!
//! The playlist is an EVENT playlist: it grows while the torrent downloads.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
  path::{Path, PathBuf},
  process::Stdio,
  time::{Duration, Instant},
};
use tokio::process::{Child, Command};

/// How long to wait for the first playable segment before giving up.
const FIRST_SEGMENT_TIMEOUT: Duration = Duration::from_secs(180);
const SEGMENT_SECONDS: u32 = 4;

/// ffmpeg/ffprobe from Homebrew (apps launched from Finder get a bare PATH).
pub fn find_tool(name: &str) -> Option<PathBuf> {
  let mut dirs = vec![
    PathBuf::from("/opt/homebrew/bin"),
    PathBuf::from("/usr/local/bin"),
  ];
  if let Some(path) = std::env::var_os("PATH") {
    dirs.extend(std::env::split_paths(&path));
  }
  dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

#[derive(Debug, Deserialize, Default)]
struct Probe {
  #[serde(default)]
  streams: Vec<ProbeStream>,
  #[serde(default)]
  format: ProbeFormat,
}

#[derive(Debug, Deserialize, Default)]
struct ProbeFormat {
  #[serde(default)]
  duration: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct ProbeStream {
  index: u32,
  #[serde(default)]
  codec_type: String,
  #[serde(default)]
  codec_name: String,
  #[serde(default)]
  tags: ProbeTags,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct ProbeTags {
  #[serde(default)]
  language: Option<String>,
}

/// The ffmpeg plan derived from a probe.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
  pub video_index: u32,
  pub copy_video: bool,
  pub hevc: bool,
  pub audio_index: Option<u32>,
  pub copy_audio: bool,
  pub duration: f64,
}

fn plan(p: &Probe) -> Result<Plan> {
  let video = p
    .streams
    .iter()
    .find(|s| s.codec_type == "video" && s.codec_name != "mjpeg" && s.codec_name != "png")
    .context("no video stream")?;
  let audios: Vec<&ProbeStream> = p
    .streams
    .iter()
    .filter(|s| s.codec_type == "audio")
    .collect();
  let is_pt = |s: &&ProbeStream| {
    matches!(
      s.tags
        .language
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref(),
      Some("por" | "pt" | "pob" | "pt-br")
    )
  };
  // Original audio first: skip a leading Portuguese dub when there's another.
  let audio = audios
    .iter()
    .find(|s| !is_pt(s))
    .or_else(|| audios.first())
    .copied();
  let vc = video.codec_name.as_str();
  Ok(Plan {
    video_index: video.index,
    copy_video: matches!(vc, "h264" | "hevc"),
    hevc: vc == "hevc",
    audio_index: audio.map(|a| a.index),
    copy_audio: audio.is_some_and(|a| a.codec_name == "aac"),
    duration: p
      .format
      .duration
      .as_deref()
      .and_then(|d| d.parse().ok())
      .unwrap_or(0.0),
  })
}

/// ffmpeg arguments for a plan (pure, unit-tested).
pub fn ffmpeg_args(input: &str, out_dir: &Path, plan: &Plan, start_at: f64) -> Vec<String> {
  let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin"]
    .iter()
    .map(|s| s.to_string())
    .collect();
  if start_at > 0.0 {
    a.extend(["-ss".into(), format!("{start_at:.3}")]);
  }
  a.extend([
    "-fflags".into(),
    "+genpts".into(),
    "-i".into(),
    input.to_string(),
    "-map".into(),
    format!("0:{}", plan.video_index),
  ]);
  if let Some(ai) = plan.audio_index {
    a.extend(["-map".into(), format!("0:{ai}")]);
  }
  if plan.copy_video {
    a.extend(["-c:v".into(), "copy".into()]);
    if plan.hevc {
      a.extend(["-tag:v".into(), "hvc1".into()]);
    }
  } else {
    a.extend([
      "-c:v".into(),
      "h264_videotoolbox".into(),
      "-b:v".into(),
      "6M".into(),
      "-pix_fmt".into(),
      "yuv420p".into(),
    ]);
  }
  if plan.audio_index.is_some() {
    if plan.copy_audio {
      a.extend(["-c:a".into(), "copy".into()]);
    } else {
      a.extend([
        "-c:a".into(),
        "aac".into(),
        "-ac".into(),
        "2".into(),
        "-b:a".into(),
        "192k".into(),
      ]);
    }
  }
  a.extend([
    "-sn".into(),
    "-dn".into(),
    "-f".into(),
    "hls".into(),
    "-hls_time".into(),
    SEGMENT_SECONDS.to_string(),
    "-hls_playlist_type".into(),
    "event".into(),
    "-hls_segment_type".into(),
    "fmp4".into(),
    "-hls_flags".into(),
    "independent_segments".into(),
    "-hls_segment_filename".into(),
    out_dir.join("seg%05d.m4s").display().to_string(),
    out_dir.join("index.m3u8").display().to_string(),
  ]);
  a
}

pub struct Remux {
  child: Child,
  offset: f64,
  duration: f64,
}

impl Remux {
  /// Probe the stream, start ffmpeg and wait for the first segment.
  pub async fn start(input: &str, out_dir: &Path, start_at: f64) -> Result<Self> {
    let ffmpeg = find_tool("ffmpeg").context("ffmpeg not found (brew install ffmpeg)")?;
    let ffprobe = find_tool("ffprobe").context("ffprobe not found")?;
    let probe_out = tokio::time::timeout(
      Duration::from_secs(120),
      Command::new(&ffprobe)
        .args([
          "-v",
          "error",
          "-show_entries",
          "stream=index,codec_type,codec_name:stream_tags=language:format=duration",
          "-of",
          "json",
          input,
        ])
        .kill_on_drop(true)
        .output(),
    )
    .await
    .context("ffprobe timed out (no peers?)")?
    .context("ffprobe failed to run")?;
    anyhow::ensure!(
      probe_out.status.success(),
      "ffprobe: {}",
      String::from_utf8_lossy(&probe_out.stderr).trim()
    );
    let probe: Probe = serde_json::from_slice(&probe_out.stdout).context("ffprobe json")?;
    let plan = plan(&probe)?;
    let start_at = if plan.duration > 0.0 && start_at >= plan.duration - 5.0 {
      0.0
    } else {
      start_at.max(0.0)
    };
    tokio::fs::create_dir_all(out_dir).await?;
    let mut child = Command::new(&ffmpeg)
      .args(ffmpeg_args(input, out_dir, &plan, start_at))
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::piped())
      .kill_on_drop(true)
      .spawn()
      .context("failed to start ffmpeg")?;
    tracing::info!(?plan, start_at, "remux started");

    let playlist = out_dir.join("index.m3u8");
    let t0 = Instant::now();
    loop {
      if let Ok(text) = tokio::fs::read_to_string(&playlist).await {
        if text.contains("#EXTINF") {
          break;
        }
      }
      if let Some(status) = child.try_wait()? {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
          use tokio::io::AsyncReadExt;
          let _ = e.read_to_string(&mut err).await;
        }
        anyhow::bail!("ffmpeg exited ({status}): {}", err.trim());
      }
      anyhow::ensure!(
        t0.elapsed() < FIRST_SEGMENT_TIMEOUT,
        "remux produced nothing in {}s",
        FIRST_SEGMENT_TIMEOUT.as_secs()
      );
      tokio::time::sleep(Duration::from_millis(300)).await;
    }
    Ok(Self {
      child,
      offset: start_at,
      duration: plan.duration,
    })
  }

  pub fn offset(&self) -> f64 {
    self.offset
  }

  pub fn duration(&self) -> f64 {
    self.duration
  }

  pub async fn kill(mut self) {
    let _ = self.child.kill().await;
  }
}

/// An EVENT playlist without `#EXT-X-ENDLIST` looks live to WebKit, which
/// then starts at the live edge (minutes into the film). Pin the start to 0.
pub fn with_start_at_zero(playlist: &str) -> String {
  if playlist.contains("#EXT-X-START") {
    return playlist.to_string();
  }
  match playlist.split_once('\n') {
    Some((first, rest)) if first.trim() == "#EXTM3U" => {
      format!("{first}\n#EXT-X-START:TIME-OFFSET=0,PRECISE=YES\n{rest}")
    }
    _ => playlist.to_string(),
  }
}

async fn playlist_response(dir: &Path, id: &str) -> axum::response::Response {
  use axum::response::IntoResponse;
  if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
    return axum::http::StatusCode::NOT_FOUND.into_response();
  }
  match tokio::fs::read_to_string(dir.join(id).join("index.m3u8")).await {
    Ok(text) => (
      [(
        axum::http::header::CONTENT_TYPE,
        "application/vnd.apple.mpegurl",
      )],
      with_start_at_zero(&text),
    )
      .into_response(),
    Err(_) => axum::http::StatusCode::NOT_FOUND.into_response(),
  }
}

/// Serve `dir` over HTTP on 127.0.0.1 (random port) for the HLS playlists.
pub async fn serve_dir(dir: PathBuf) -> Result<u16> {
  tokio::fs::create_dir_all(&dir).await.ok();
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .context("failed to bind HLS server")?;
  let port = listener.local_addr()?.port();
  let playlists = dir.clone();
  let app = axum::Router::new()
    .route(
      "/{id}/index.m3u8",
      axum::routing::get(
        move |axum::extract::Path(id): axum::extract::Path<String>| {
          let dir = playlists.clone();
          async move { playlist_response(&dir, &id).await }
        },
      ),
    )
    .fallback_service(tower_http::services::ServeDir::new(dir))
    .layer(tower_http::cors::CorsLayer::permissive())
    .layer(axum::middleware::map_response(
      |mut res: axum::response::Response| async move {
        // playlists grow: never let the WebView cache them
        res.headers_mut().insert(
          axum::http::header::CACHE_CONTROL,
          axum::http::HeaderValue::from_static("no-cache"),
        );
        res
      },
    ));
  tokio::spawn(async move {
    if let Err(e) = axum::serve(listener, app).await {
      tracing::error!("HLS server stopped: {e:#}");
    }
  });
  Ok(port)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn probe(json: &str) -> Probe {
    serde_json::from_str(json).unwrap()
  }

  #[test]
  fn copies_hevc_and_converts_dts() {
    let p = probe(
      r#"{"streams":[
        {"index":0,"codec_type":"video","codec_name":"hevc"},
        {"index":1,"codec_type":"audio","codec_name":"dts","tags":{"language":"eng"}},
        {"index":2,"codec_type":"subtitle","codec_name":"subrip"}],
        "format":{"duration":"6012.5"}}"#,
    );
    let plan = plan(&p).unwrap();
    assert_eq!(
      plan,
      Plan {
        video_index: 0,
        copy_video: true,
        hevc: true,
        audio_index: Some(1),
        copy_audio: false,
        duration: 6012.5
      }
    );
    let args = ffmpeg_args("http://x/f.mkv", Path::new("/tmp/h"), &plan, 0.0).join(" ");
    assert!(args.contains("-c:v copy -tag:v hvc1"));
    assert!(args.contains("-c:a aac -ac 2 -b:a 192k"));
    assert!(args.contains("-hls_segment_type fmp4"));
    assert!(!args.contains("-ss"));
  }

  #[test]
  fn prefers_original_audio_over_portuguese_dub() {
    let p = probe(
      r#"{"streams":[
        {"index":0,"codec_type":"video","codec_name":"h264"},
        {"index":1,"codec_type":"audio","codec_name":"ac3","tags":{"language":"por"}},
        {"index":2,"codec_type":"audio","codec_name":"aac","tags":{"language":"eng"}}]}"#,
    );
    let plan = plan(&p).unwrap();
    assert_eq!(
      (plan.audio_index, plan.copy_audio, plan.hevc),
      (Some(2), true, false)
    );
  }

  #[test]
  fn reencodes_xvid_and_seeks_on_resume() {
    let p = probe(r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"mpeg4"}]}"#);
    let plan = plan(&p).unwrap();
    let args = ffmpeg_args("http://x/f.avi", Path::new("/tmp/h"), &plan, 125.5).join(" ");
    assert!(args.starts_with("-hide_banner -loglevel error -nostdin -ss 125.500 -fflags"));
    assert!(args.contains("-c:v h264_videotoolbox"));
    assert!(!args.contains("-c:a"));
  }

  #[test]
  fn pins_event_playlists_to_the_start() {
    let p = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-PLAYLIST-TYPE:EVENT\n";
    let out = with_start_at_zero(p);
    assert!(out.starts_with("#EXTM3U\n#EXT-X-START:TIME-OFFSET=0,PRECISE=YES\n#EXT-X-VERSION:7"));
    assert_eq!(with_start_at_zero(&out), out);
  }

  #[tokio::test]
  async fn serves_playlists_with_start_tag_and_segments() {
    let dir = std::env::temp_dir().join(format!("pt-hls-test-{}", std::process::id()));
    tokio::fs::create_dir_all(dir.join("7")).await.unwrap();
    tokio::fs::write(
      dir.join("7/index.m3u8"),
      "#EXTM3U\n#EXTINF:4,\nseg00000.m4s\n",
    )
    .await
    .unwrap();
    tokio::fs::write(dir.join("7/seg00000.m4s"), b"abc")
      .await
      .unwrap();
    let port = serve_dir(dir.clone()).await.unwrap();
    let base = format!("http://127.0.0.1:{port}/7");
    let pl = reqwest::get(format!("{base}/index.m3u8")).await.unwrap();
    assert_eq!(pl.headers()["cache-control"], "no-cache");
    assert!(
      pl.text()
        .await
        .unwrap()
        .contains("#EXT-X-START:TIME-OFFSET=0")
    );
    let seg = reqwest::get(format!("{base}/seg00000.m4s")).await.unwrap();
    assert_eq!(seg.bytes().await.unwrap().as_ref(), b"abc");
    assert_eq!(
      reqwest::get(format!("http://127.0.0.1:{port}/x/index.m3u8"))
        .await
        .unwrap()
        .status(),
      404
    );
    let _ = tokio::fs::remove_dir_all(dir).await;
  }

  #[test]
  fn ignores_cover_art_as_video() {
    let p = probe(
      r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"mjpeg"},
      {"index":1,"codec_type":"video","codec_name":"h264"}]}"#,
    );
    assert_eq!(plan(&p).unwrap().video_index, 1);
  }
}
