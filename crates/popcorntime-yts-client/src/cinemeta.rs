//! Series catalogue from Cinemeta (Stremio's public metadata addon):
//! popular/genre/search listings and per-show episode lists. No key needed.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const CINEMETA_BASE_URL: &str = "https://v3-cinemeta.strem.io";
/// Cinemeta pages are 50 items long.
pub const CINEMETA_PAGE: u32 = 50;

/// Genre values Cinemeta accepts for `series/top`.
pub const SERIES_GENRES: &[&str] = &[
  "Action",
  "Adventure",
  "Animation",
  "Biography",
  "Comedy",
  "Crime",
  "Documentary",
  "Drama",
  "Family",
  "Fantasy",
  "History",
  "Horror",
  "Mystery",
  "Romance",
  "Sci-Fi",
  "Sport",
  "Thriller",
  "War",
  "Western",
  "Reality-TV",
  "Talk-Show",
  "Game-Show",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SeriesSummary {
  pub id: String,
  pub name: String,
  pub poster: String,
  pub year: String,
  pub rating: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Episode {
  pub season: u32,
  pub episode: u32,
  pub title: String,
  pub overview: String,
  pub thumbnail: String,
  /// ISO date, empty when unknown.
  pub released: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SeriesDetail {
  pub id: String,
  pub name: String,
  pub poster: String,
  pub background: String,
  pub description: String,
  pub year: String,
  pub rating: String,
  pub runtime: String,
  pub genres: Vec<String>,
  /// YouTube id of the trailer, empty when none.
  pub trailer: String,
  pub episodes: Vec<Episode>,
}

#[derive(Deserialize)]
struct CatalogResponse {
  #[serde(default)]
  metas: Vec<RawMeta>,
}

#[derive(Deserialize)]
struct MetaResponse {
  meta: RawMeta,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawMeta {
  id: String,
  name: String,
  poster: String,
  background: String,
  description: String,
  #[serde(rename = "releaseInfo")]
  release_info: String,
  year: String,
  #[serde(rename = "imdbRating")]
  imdb_rating: String,
  runtime: String,
  genres: Vec<String>,
  trailers: Vec<RawTrailer>,
  videos: Vec<RawVideo>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawTrailer {
  source: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawVideo {
  season: u32,
  episode: u32,
  number: u32,
  name: String,
  title: String,
  overview: String,
  description: String,
  thumbnail: String,
  released: String,
}

fn client() -> Result<reqwest::Client> {
  reqwest::ClientBuilder::new()
    .user_agent("Mozilla/5.0 (Macintosh) PopcornTime")
    .timeout(Duration::from_secs(15))
    .build()
    .context("cinemeta http client")
}

/// `catalog/series/top[/genre=X][&search=Y][&skip=N].json`
pub fn catalog_url(genre: Option<&str>, search: Option<&str>, skip: u32) -> String {
  let mut extra: Vec<String> = Vec::new();
  if let Some(g) = genre.filter(|g| !g.is_empty()) {
    extra.push(format!("genre={}", enc(g)));
  }
  if let Some(q) = search.map(str::trim).filter(|q| !q.is_empty()) {
    extra.push(format!("search={}", enc(q)));
  }
  if skip > 0 {
    extra.push(format!("skip={skip}"));
  }
  if extra.is_empty() {
    format!("{CINEMETA_BASE_URL}/catalog/series/top.json")
  } else {
    format!(
      "{CINEMETA_BASE_URL}/catalog/series/top/{}.json",
      extra.join("&")
    )
  }
}

fn enc(s: &str) -> String {
  url::form_urlencoded::byte_serialize(s.as_bytes())
    .collect::<String>()
    .replace('+', "%20")
}

fn summary(m: RawMeta) -> SeriesSummary {
  SeriesSummary {
    year: if m.release_info.is_empty() {
      m.year
    } else {
      m.release_info
    },
    id: m.id,
    name: m.name,
    poster: m.poster,
    rating: m.imdb_rating,
  }
}

pub async fn series_catalog(
  genre: Option<&str>,
  search: Option<&str>,
  skip: u32,
) -> Result<Vec<SeriesSummary>> {
  if let Some(g) = genre.filter(|g| !g.is_empty()) {
    anyhow::ensure!(SERIES_GENRES.contains(&g), "unknown genre: {g}");
  }
  let url = catalog_url(genre, search, skip);
  let res: CatalogResponse = client()?
    .get(&url)
    .send()
    .await
    .with_context(|| format!("cinemeta request failed: {url}"))?
    .error_for_status()?
    .json()
    .await
    .context("cinemeta catalog payload")?;
  Ok(
    res
      .metas
      .into_iter()
      .filter(|m| m.id.starts_with("tt"))
      .map(summary)
      .collect(),
  )
}

fn detail(m: RawMeta) -> SeriesDetail {
  let mut episodes: Vec<Episode> = m
    .videos
    .into_iter()
    .map(|v| Episode {
      season: v.season,
      episode: if v.episode > 0 { v.episode } else { v.number },
      title: if v.name.is_empty() { v.title } else { v.name },
      overview: if v.overview.is_empty() {
        v.description
      } else {
        v.overview
      },
      thumbnail: v.thumbnail,
      released: v.released,
    })
    .filter(|e| e.episode > 0)
    .collect();
  // Seasons in order, specials (season 0) last.
  episodes.sort_by_key(|e| (e.season == 0, e.season, e.episode));
  episodes.dedup_by_key(|e| (e.season, e.episode));
  SeriesDetail {
    year: if m.release_info.is_empty() {
      m.year
    } else {
      m.release_info
    },
    trailer: m
      .trailers
      .into_iter()
      .map(|t| t.source)
      .find(|s| !s.is_empty())
      .unwrap_or_default(),
    id: m.id,
    name: m.name,
    poster: m.poster,
    background: m.background,
    description: m.description,
    rating: m.imdb_rating,
    runtime: m.runtime,
    genres: m.genres,
    episodes,
  }
}

pub async fn series_meta(imdb: &str) -> Result<SeriesDetail> {
  let imdb = crate::sources::normalize_imdb(imdb)?;
  let url = format!("{CINEMETA_BASE_URL}/meta/series/{imdb}.json");
  let res: MetaResponse = client()?
    .get(&url)
    .send()
    .await
    .with_context(|| format!("cinemeta request failed: {url}"))?
    .error_for_status()?
    .json()
    .await
    .context("cinemeta meta payload")?;
  Ok(detail(res.meta))
}

/// The episode after `(season, episode)` in broadcast order (specials skipped).
pub fn next_episode(episodes: &[Episode], season: u32, episode: u32) -> Option<&Episode> {
  episodes
    .iter()
    .filter(|e| e.season > 0)
    .find(|e| (e.season, e.episode) > (season, episode))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn builds_catalog_urls() {
    assert_eq!(
      catalog_url(None, None, 0),
      format!("{CINEMETA_BASE_URL}/catalog/series/top.json")
    );
    assert_eq!(
      catalog_url(Some("Sci-Fi"), None, 50),
      format!("{CINEMETA_BASE_URL}/catalog/series/top/genre=Sci-Fi&skip=50.json")
    );
    assert_eq!(
      catalog_url(None, Some(" breaking bad "), 0),
      format!("{CINEMETA_BASE_URL}/catalog/series/top/search=breaking%20bad.json")
    );
  }

  #[test]
  fn detail_orders_episodes_and_finds_next() {
    let raw: RawMeta = serde_json::from_str(
      r#"{"id":"tt1","name":"Show","releaseInfo":"2008–2013","imdbRating":"9.5",
      "trailers":[{"source":"abc","type":"Trailer"}],
      "videos":[
        {"season":2,"episode":1,"name":"B1"},
        {"season":0,"episode":1,"name":"Special"},
        {"season":1,"number":2,"name":"A2"},
        {"season":1,"episode":1,"name":"A1","overview":"pilot"}
      ]}"#,
    )
    .unwrap();
    let d = detail(raw);
    assert_eq!(d.year, "2008–2013");
    assert_eq!(d.trailer, "abc");
    let order: Vec<_> = d
      .episodes
      .iter()
      .map(|e| (e.season, e.episode, e.title.as_str()))
      .collect();
    assert_eq!(
      order,
      [(1, 1, "A1"), (1, 2, "A2"), (2, 1, "B1"), (0, 1, "Special")]
    );
    assert_eq!(
      next_episode(&d.episodes, 1, 2).map(|e| e.title.as_str()),
      Some("B1")
    );
    assert!(next_episode(&d.episodes, 2, 1).is_none());
  }

  #[tokio::test]
  #[ignore = "hits the live network"]
  async fn live_catalog_and_meta() {
    assert_eq!(series_catalog(None, None, 0).await.unwrap().len(), 50);
    assert!(
      !series_catalog(Some("Horror"), None, 0)
        .await
        .unwrap()
        .is_empty()
    );
    let bb = series_meta("tt0903747").await.unwrap();
    assert_eq!(bb.name, "Breaking Bad");
    assert!(bb.episodes.len() >= 62);
  }
}
