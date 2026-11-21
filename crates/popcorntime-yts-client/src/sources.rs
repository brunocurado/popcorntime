//! Extra torrent sources keyed by IMDb id, merged next to the YTS list.
//!
//! - **Torrentio** (`torrentio.strem.fun`): a Stremio addon that aggregates
//!   many indexers (RARBG archive, 1337x, TPB, Rutracker, TorrentGalaxy…).
//! - **The Pirate Bay** via `apibay.org`, its public JSON API.
//!
//! Each source fails independently: one being down never hides the other.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const TORRENTIO_BASE_URL: &str = "https://torrentio.strem.fun";
pub const APIBAY_BASE_URL: &str = "https://apibay.org";
/// Upper bound per source so the dialog stays readable.
pub const MAX_PER_SOURCE: usize = 25;

/// One torrent from a non-YTS source, normalised for the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ExtraTorrent {
  /// Where it came from, e.g. `Torrentio · 1337x` or `The Pirate Bay`.
  pub source: String,
  /// Release name as published.
  pub title: String,
  /// 40-char hex info-hash, lowercase.
  pub hash: String,
  /// Quality label (`4k`, `1080p`…), empty when unknown.
  pub quality: String,
  pub seeds: u32,
  /// Human-readable size (`1.85 GB`), empty when unknown.
  pub size: String,
}

fn http_client() -> Result<reqwest::Client> {
  reqwest::ClientBuilder::new()
    // Both endpoints sit behind Cloudflare and reject unknown agents.
    .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")
    .timeout(Duration::from_secs(12))
    .build()
    .context("failed to build sources http client")
}

fn valid_hash(h: &str) -> bool {
  h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit()) && h.chars().any(|c| c != '0')
}

/// `tt0133093` → `tt0133093`; `0133093` → `tt0133093`. Rejects anything else.
pub fn normalize_imdb(imdb: &str) -> Result<String> {
  let digits = imdb.trim().trim_start_matches("tt");
  anyhow::ensure!(
    !digits.is_empty() && digits.len() <= 10 && digits.chars().all(|c| c.is_ascii_digit()),
    "invalid IMDb id: {imdb}"
  );
  Ok(format!("tt{digits}"))
}

pub fn human_size(bytes: u64) -> String {
  const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
  let mut v = bytes as f64;
  let mut i = 0;
  while v >= 1024.0 && i < UNITS.len() - 1 {
    v /= 1024.0;
    i += 1;
  }
  if i == 0 {
    format!("{bytes} B")
  } else {
    format!("{v:.2} {}", UNITS[i])
  }
}

// ---------- Torrentio ----------

#[derive(Deserialize)]
struct TorrentioResponse {
  #[serde(default)]
  streams: Vec<TorrentioStream>,
}

#[derive(Deserialize)]
struct TorrentioStream {
  #[serde(default)]
  name: String,
  #[serde(default)]
  title: String,
  #[serde(rename = "infoHash", default)]
  info_hash: String,
}

/// Parse a Torrentio stream. `name` is `Torrentio\n<quality…>`; `title` is
/// `<release>\n👤 <seeds> 💾 <size> ⚙️ <provider>[\n…]`.
fn parse_torrentio(s: &TorrentioStream) -> Option<ExtraTorrent> {
  let hash = s.info_hash.to_ascii_lowercase();
  if !valid_hash(&hash) {
    return None;
  }
  let quality = s
    .name
    .lines()
    .nth(1)
    .and_then(|l| l.split_whitespace().next())
    .unwrap_or("")
    .to_string();
  let mut lines = s.title.lines();
  let title = lines.next().unwrap_or("").trim().to_string();
  let meta = s
    .title
    .lines()
    .find(|l| l.contains('👤') || l.contains('💾') || l.contains('⚙'))
    .unwrap_or("");
  let after = |marker: &str| -> Option<&str> { meta.split(marker).nth(1).map(str::trim) };
  let seeds = after("👤")
    .and_then(|r| r.split_whitespace().next())
    .and_then(|n| n.parse().ok())
    .unwrap_or(0);
  let size = after("💾")
    .map(|r| r.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
    .unwrap_or_default();
  let provider = after("⚙️")
    .or_else(|| after("⚙"))
    .map(|r| r.trim_start_matches('\u{fe0f}').trim().to_string())
    .unwrap_or_default();
  let source = if provider.is_empty() {
    "Torrentio".to_string()
  } else {
    format!("Torrentio · {provider}")
  };
  Some(ExtraTorrent {
    source,
    title,
    hash,
    quality,
    seeds,
    size,
  })
}

pub async fn torrentio_movie(imdb: &str) -> Result<Vec<ExtraTorrent>> {
  let imdb = normalize_imdb(imdb)?;
  let url = format!("{TORRENTIO_BASE_URL}/stream/movie/{imdb}.json");
  let res: TorrentioResponse = http_client()?
    .get(&url)
    .send()
    .await
    .with_context(|| format!("torrentio request failed: {url}"))?
    .error_for_status()?
    .json()
    .await
    .context("torrentio payload")?;
  let mut out: Vec<_> = res.streams.iter().filter_map(parse_torrentio).collect();
  out.sort_by_key(|t| std::cmp::Reverse(t.seeds));
  out.truncate(MAX_PER_SOURCE);
  Ok(out)
}

// ---------- The Pirate Bay (apibay) ----------

#[derive(Deserialize)]
struct ApibayItem {
  #[serde(default)]
  name: String,
  #[serde(default)]
  info_hash: String,
  #[serde(default)]
  seeders: String,
  #[serde(default)]
  size: String,
}

fn guess_quality(title: &str) -> String {
  let t = title.to_ascii_lowercase();
  for (needle, label) in [
    ("2160p", "4k"),
    ("4k", "4k"),
    ("1080p", "1080p"),
    ("720p", "720p"),
    ("480p", "480p"),
  ] {
    if t.contains(needle) {
      return label.to_string();
    }
  }
  String::new()
}

fn parse_apibay(i: &ApibayItem) -> Option<ExtraTorrent> {
  let hash = i.info_hash.to_ascii_lowercase();
  if !valid_hash(&hash) {
    return None; // "No results returned" row carries an all-zero hash
  }
  let size = i.size.parse::<u64>().map(human_size).unwrap_or_default();
  Some(ExtraTorrent {
    source: "The Pirate Bay".to_string(),
    quality: guess_quality(&i.name),
    title: i.name.clone(),
    hash,
    seeds: i.seeders.parse().unwrap_or(0),
    size,
  })
}

pub async fn apibay_movie(imdb: &str) -> Result<Vec<ExtraTorrent>> {
  let imdb = normalize_imdb(imdb)?;
  let url = format!("{APIBAY_BASE_URL}/q.php?q={imdb}");
  let items: Vec<ApibayItem> = http_client()?
    .get(&url)
    .send()
    .await
    .with_context(|| format!("apibay request failed: {url}"))?
    .error_for_status()?
    .json()
    .await
    .context("apibay payload")?;
  let mut out: Vec<_> = items.iter().filter_map(parse_apibay).collect();
  out.sort_by_key(|t| std::cmp::Reverse(t.seeds));
  out.truncate(MAX_PER_SOURCE);
  Ok(out)
}

/// Query every extra source concurrently; a failing source is logged and
/// skipped. Duplicate hashes keep the entry with more seeds.
pub async fn extra_sources(imdb: &str) -> Result<Vec<ExtraTorrent>> {
  normalize_imdb(imdb)?;
  let (a, b) = tokio::join!(torrentio_movie(imdb), apibay_movie(imdb));
  let mut all = Vec::new();
  for (name, r) in [("torrentio", a), ("apibay", b)] {
    match r {
      Ok(v) => all.extend(v),
      Err(e) => tracing::warn!("extra source {name} failed: {e:#}"),
    }
  }
  all.sort_by_key(|t| std::cmp::Reverse(t.seeds));
  let mut seen = std::collections::HashSet::new();
  all.retain(|t| seen.insert(t.hash.clone()));
  Ok(all)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn imdb_normalisation() {
    assert_eq!(normalize_imdb("tt0133093").unwrap(), "tt0133093");
    assert_eq!(normalize_imdb("0133093").unwrap(), "tt0133093");
    assert!(normalize_imdb("tt01a").is_err());
    assert!(normalize_imdb("").is_err());
  }

  #[test]
  fn parses_real_torrentio_stream() {
    let s = TorrentioStream {
      name: "Torrentio\n4k HDR".into(),
      title: "The.Matrix.1999.2160p.UHD.BluRay.x265-IAMABLE\n👤 124 💾 35.09 GB ⚙️ RARBG".into(),
      info_hash: "03DD34FEA0FF15A451C1723062A901AA3A0AD458".into(),
    };
    let t = parse_torrentio(&s).unwrap();
    assert_eq!(t.source, "Torrentio · RARBG");
    assert_eq!(t.quality, "4k");
    assert_eq!(t.seeds, 124);
    assert_eq!(t.size, "35.09 GB");
    assert_eq!(t.hash, "03dd34fea0ff15a451c1723062a901aa3a0ad458");
    assert_eq!(t.title, "The.Matrix.1999.2160p.UHD.BluRay.x265-IAMABLE");
  }

  #[test]
  fn parses_apibay_and_skips_empty_row() {
    let ok = ApibayItem {
      name: "The Matrix (1999) 1080p BrRip x264 - YIFY".into(),
      info_hash: "D7A46713EAEE18C746B3254B7D1492A50FD9D6CE".into(),
      seeders: "1075".into(),
      size: "1992277407".into(),
    };
    let t = parse_apibay(&ok).unwrap();
    assert_eq!(
      (t.seeds, t.quality.as_str(), t.size.as_str()),
      (1075, "1080p", "1.86 GB")
    );
    let none = ApibayItem {
      name: "No results returned".into(),
      info_hash: "0000000000000000000000000000000000000000".into(),
      seeders: "0".into(),
      size: "0".into(),
    };
    assert!(parse_apibay(&none).is_none());
  }

  #[tokio::test]
  #[ignore = "hits the live network"]
  async fn live_extra_sources_return_torrents() {
    let all = extra_sources("tt0133093").await.unwrap();
    assert!(all.iter().any(|t| t.source.starts_with("Torrentio")));
    assert!(all.iter().any(|t| t.source == "The Pirate Bay"));
  }
}
