//! Extra torrent sources keyed by IMDb id, merged next to the YTS list.
//!
//! - **Torrentio** (`torrentio.strem.fun`): a Stremio addon that aggregates
//!   many indexers (RARBG archive, 1337x, TPB, Rutracker, TorrentGalaxy…).
//! - **The Pirate Bay** via `apibay.org`, its public JSON API.
//! - **KickassTorrents** (`kickasstorrents.to`): HTML search + detail pages.
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
  /// File inside a multi-file torrent (season packs), when the source says.
  pub file_idx: Option<u32>,
  /// Exact file name inside the torrent, when the source says.
  pub file_name: Option<String>,
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
  #[serde(rename = "fileIdx", default)]
  file_idx: Option<u32>,
  #[serde(rename = "behaviorHints", default)]
  hints: Option<TorrentioHints>,
}

#[derive(Deserialize, Default)]
struct TorrentioHints {
  #[serde(default)]
  filename: Option<String>,
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
  let file_name = s
    .hints
    .as_ref()
    .and_then(|h| h.filename.clone())
    .filter(|f| !f.is_empty());
  Some(ExtraTorrent {
    source,
    title,
    hash,
    quality,
    seeds,
    size,
    file_idx: s.file_idx,
    file_name,
  })
}

pub async fn torrentio_movie(imdb: &str) -> Result<Vec<ExtraTorrent>> {
  let imdb = normalize_imdb(imdb)?;
  torrentio_streams(format!("{TORRENTIO_BASE_URL}/stream/movie/{imdb}.json")).await
}

/// Streams for one episode (`tt…:season:episode`); packs carry `file_idx`.
pub async fn torrentio_episode(imdb: &str, season: u32, episode: u32) -> Result<Vec<ExtraTorrent>> {
  let imdb = normalize_imdb(imdb)?;
  torrentio_streams(format!(
    "{TORRENTIO_BASE_URL}/stream/series/{imdb}:{season}:{episode}.json"
  ))
  .await
}

async fn torrentio_streams(url: String) -> Result<Vec<ExtraTorrent>> {
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
    file_idx: None,
    file_name: None,
  })
}

pub async fn apibay_movie(imdb: &str) -> Result<Vec<ExtraTorrent>> {
  let imdb = normalize_imdb(imdb)?;
  // without `cat` apibay now answers IMDb ids with "No results returned"
  let url = format!("{APIBAY_BASE_URL}/q.php?q={imdb}&cat=0");
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

// ---------- KickassTorrents (HTML) ----------

pub const KAT_BASE_URL: &str = "https://kickasstorrents.to";
/// Detail pages fetched per search (each one is a request for the magnet).
const KAT_DETAILS: usize = 8;

#[derive(Debug, Clone, PartialEq)]
struct KatRow {
  title: String,
  href: String,
  size: String,
  seeds: u32,
}

fn strip_tags(s: &str) -> String {
  let mut out = String::new();
  let mut in_tag = false;
  for c in s.chars() {
    match c {
      '<' => in_tag = true,
      '>' => in_tag = false,
      _ if !in_tag => out.push(c),
      _ => {}
    }
  }
  let out = out
    .replace("&amp;", "&")
    .replace("&quot;", "\"")
    .replace("&#039;", "'")
    .replace("&lt;", "<")
    .replace("&gt;", ">");
  out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
  let i = s.find(start)? + start.len();
  let j = s[i..].find(end)? + i;
  Some(&s[i..j])
}

/// Parse a KAT search results page into rows.
fn parse_kat_search(html: &str) -> Vec<KatRow> {
  let mut rows = Vec::new();
  for chunk in html.split("<tr class=\"").skip(1) {
    if !(chunk.starts_with("odd") || chunk.starts_with("even")) {
      continue;
    }
    let row = chunk.split("</tr>").next().unwrap_or(chunk);
    let Some(link_start) = row.find("class=\"cellMainLink\"") else {
      continue;
    };
    // href is the attribute right before class="cellMainLink"
    let before = &row[..link_start];
    let Some(href) = before
      .rsplit("href=\"")
      .next()
      .and_then(|r| r.split('"').next())
    else {
      continue;
    };
    let title = between(&row[link_start..], ">", "</a>")
      .map(strip_tags)
      .unwrap_or_default();
    let size = between(row, "<td class=\"nobr center\">", "</td>")
      .map(strip_tags)
      .unwrap_or_default();
    let seeds = between(row, "<td class=\"green center\">", "</td>")
      .map(strip_tags)
      .and_then(|s| s.parse().ok())
      .unwrap_or(0);
    if title.is_empty() || !href.starts_with('/') {
      continue;
    }
    rows.push(KatRow {
      title,
      href: href.to_string(),
      size,
      seeds,
    });
  }
  rows
}

fn parse_kat_magnet_hash(html: &str) -> Option<String> {
  let i = html.find("magnet:?xt=urn:btih:")? + "magnet:?xt=urn:btih:".len();
  let hash: String = html[i..]
    .chars()
    .take_while(|c| c.is_ascii_alphanumeric())
    .collect();
  let hash = hash.to_ascii_lowercase();
  valid_hash(&hash).then_some(hash)
}

/// Search KAT and resolve the best-seeded results to magnet hashes.
pub async fn kat_search(query: &str) -> Result<Vec<ExtraTorrent>> {
  let q = query.trim();
  anyhow::ensure!(!q.is_empty(), "empty KAT query");
  let client = http_client()?;
  let encoded: String = url::form_urlencoded::byte_serialize(q.as_bytes()).collect();
  let url = format!("{KAT_BASE_URL}/usearch/{}/", encoded.replace('+', "%20"));
  let html = client
    .get(&url)
    .send()
    .await
    .with_context(|| format!("kat request failed: {url}"))?
    .error_for_status()?
    .text()
    .await?;
  let mut rows = parse_kat_search(&html);
  rows.sort_by_key(|r| std::cmp::Reverse(r.seeds));
  rows.truncate(KAT_DETAILS);
  let fetches = rows.into_iter().map(|row| {
    let client = client.clone();
    async move {
      let page = client
        .get(format!("{KAT_BASE_URL}{}", row.href))
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
      let hash = parse_kat_magnet_hash(&page)?;
      Some(ExtraTorrent {
        source: "KickassTorrents".to_string(),
        quality: guess_quality(&row.title),
        title: row.title,
        hash,
        seeds: row.seeds,
        size: row.size,
        file_idx: None,
        file_name: None,
      })
    }
  });
  Ok(
    futures_util::future::join_all(fetches)
      .await
      .into_iter()
      .flatten()
      .collect(),
  )
}

/// `S01E02` tag used to keep only matching KAT episode results.
pub fn episode_tag(season: u32, episode: u32) -> String {
  format!("S{season:02}E{episode:02}")
}

fn merge(results: Vec<(&str, Result<Vec<ExtraTorrent>>)>) -> Vec<ExtraTorrent> {
  let mut all = Vec::new();
  for (name, r) in results {
    match r {
      Ok(v) => all.extend(v),
      Err(e) => tracing::warn!("extra source {name} failed: {e:#}"),
    }
  }
  all.sort_by_key(|t| std::cmp::Reverse(t.seeds));
  let mut seen = std::collections::HashSet::new();
  all.retain(|t| seen.insert(t.hash.clone()));
  all
}

/// Query every extra source concurrently; a failing source is logged and
/// skipped. Duplicate hashes keep the entry with more seeds. `title`/`year`
/// feed the KAT text search (KAT has no IMDb lookup).
pub async fn extra_sources(
  imdb: &str,
  title: Option<&str>,
  year: Option<u32>,
) -> Result<Vec<ExtraTorrent>> {
  normalize_imdb(imdb)?;
  let kat_query = title.filter(|t| !t.trim().is_empty()).map(|t| match year {
    Some(y) => format!("{t} {y}"),
    None => t.to_string(),
  });
  let kat = async {
    match &kat_query {
      Some(q) => kat_search(q).await,
      None => Ok(Vec::new()),
    }
  };
  let (a, b, c) = tokio::join!(torrentio_movie(imdb), apibay_movie(imdb), kat);
  Ok(merge(vec![("torrentio", a), ("apibay", b), ("kat", c)]))
}

/// Torrents for one episode: Torrentio (with pack file hints) + KAT results
/// whose name carries the `SxxEyy` tag.
pub async fn episode_sources(
  imdb: &str,
  show: &str,
  season: u32,
  episode: u32,
) -> Result<Vec<ExtraTorrent>> {
  normalize_imdb(imdb)?;
  let tag = episode_tag(season, episode);
  let kat = async {
    if show.trim().is_empty() {
      return Ok(Vec::new());
    }
    let mut v = kat_search(&format!("{show} {tag}")).await?;
    v.retain(|t| {
      t.title
        .to_ascii_uppercase()
        .replace(' ', ".")
        .contains(&tag)
    });
    Ok(v)
  };
  let (a, b) = tokio::join!(torrentio_episode(imdb, season, episode), kat);
  Ok(merge(vec![("torrentio", a), ("kat", b)]))
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
      file_idx: None,
      hints: None,
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

  #[test]
  fn torrentio_season_pack_keeps_file_hint() {
    let s = TorrentioStream {
      name: "Torrentio\n1080p".into(),
      title: "Breaking Bad S01 1080p\nBreaking.Bad.S01E02.1080p.mkv\n👤 40 💾 1.2 GB ⚙️ 1337x"
        .into(),
      info_hash: "a584351126afcf50d32be068f3c9a62550d41ecc".into(),
      file_idx: Some(2),
      hints: Some(TorrentioHints {
        filename: Some("Breaking.Bad.S01E02.1080p.mkv".into()),
      }),
    };
    let t = parse_torrentio(&s).unwrap();
    assert_eq!(t.file_idx, Some(2));
    assert_eq!(
      t.file_name.as_deref(),
      Some("Breaking.Bad.S01E02.1080p.mkv")
    );
    assert_eq!((t.seeds, t.size.as_str()), (40, "1.2 GB"));
  }

  const KAT_ROW: &str = r#"<table><tr class="odd" >
    <td><div class="torrentname">
      <a href="/insidious-1080p-t6766520.html" class="torType filmType"></a>
      <div class="markeredBlock torType filmType">
        <a href="/insidious-1080p-t6766520.html"
        class="cellMainLink">
          <strong class="red">Insidious</strong> - Out of the Further 2026 <strong class="red">1080p</strong> Blu-Ray HEVC</a>
      </div></div></td>
    <td class="nobr center">
        5.6 GB                </td>
    <td class="center">Johhnnoo</td>
    <td class="center" title="2<br/>days">2<br/>days</td>
    <td class="green center">
        191                </td>
    <td class="red lasttd center">51</td>
  </tr><tr class="even" ><td>no link here</td></tr></table>"#;

  #[test]
  fn parses_kat_search_rows() {
    let rows = parse_kat_search(KAT_ROW);
    assert_eq!(
      rows,
      vec![KatRow {
        title: "Insidious - Out of the Further 2026 1080p Blu-Ray HEVC".into(),
        href: "/insidious-1080p-t6766520.html".into(),
        size: "5.6 GB".into(),
        seeds: 191,
      }]
    );
    let page = r#"<a href="magnet:?xt=urn:btih:3AF1684AE2B5CB57AEAE03A4216B5928AC258D42&dn=x">"#;
    assert_eq!(
      parse_kat_magnet_hash(page).as_deref(),
      Some("3af1684ae2b5cb57aeae03a4216b5928ac258d42")
    );
    assert!(parse_kat_magnet_hash("no magnet").is_none());
  }

  #[tokio::test]
  #[ignore = "hits the live network"]
  async fn live_extra_sources_return_torrents() {
    let all = extra_sources("tt0133093", Some("The Matrix"), Some(1999))
      .await
      .unwrap();
    assert!(all.iter().any(|t| t.source.starts_with("Torrentio")));
    assert!(all.iter().any(|t| t.source == "The Pirate Bay"));
    assert!(all.iter().any(|t| t.source == "KickassTorrents"));
  }

  #[tokio::test]
  #[ignore = "hits the live network"]
  async fn live_episode_sources_carry_file_hints() {
    let all = episode_sources("tt0903747", "Breaking Bad", 1, 2)
      .await
      .unwrap();
    assert!(all.len() > 10);
    assert!(
      all
        .iter()
        .any(|t| t.file_name.as_deref().is_some_and(|f| f.contains("S01E02")))
    );
  }
}
