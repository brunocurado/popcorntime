//! Async client for the YTS API v2.
//!
//! All requests hit `<base>/<endpoint>.json` with `GET`. URL building is
//! kept in pure functions so it stays unit-testable without network.

use crate::{
  consts::{YTS_API_BASE_URL, YTS_DEFAULT_TRACKERS},
  types::{
    YtsEnvelope, YtsListMoviesParams, YtsMovieDetailsData, YtsMovieDetailsParams, YtsMovieIdParams,
    YtsMovieListData, YtsParentalGuidesData,
  },
};
use anyhow::{Context, Result};
use std::time::Duration;

static USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

fn http_client() -> Result<reqwest::Client> {
  reqwest::ClientBuilder::new()
    .user_agent(USER_AGENT)
    .timeout(Duration::from_secs(10))
    .build()
    .context("failed to build YTS http client")
}

/// Clamp `limit` to the API contract (1–50, default 20).
pub fn normalize_limit(limit: Option<u32>) -> u32 {
  match limit {
    Some(n) if (1..=50).contains(&n) => n,
    Some(_) => 20,
    None => 20,
  }
}

/// Clamp `minimum_rating` to the API contract (0–9, default 0).
pub fn normalize_minimum_rating(rating: Option<u8>) -> u8 {
  rating.map(|r| r.min(9)).unwrap_or(0)
}

/// Join an endpoint file onto the base without dropping the version segment
/// (`Url::join` would replace `v2` when the base has no trailing slash).
fn endpoint_url(base: &str, file: &str) -> Result<url::Url> {
  let prefix = base.trim_end_matches('/');
  url::Url::parse(&format!("{prefix}/{file}"))
    .with_context(|| format!("invalid YTS endpoint url: {prefix}/{file}"))
}

/// Build the `list_movies.json` URL for the given params.
pub fn list_movies_url(base: &str, params: &YtsListMoviesParams) -> Result<url::Url> {
  let mut url = endpoint_url(base, "list_movies.json")?;
  {
    let mut q = url.query_pairs_mut();
    q.append_pair("limit", &normalize_limit(params.limit).to_string());
    if let Some(page) = params.page {
      q.append_pair("page", &page.to_string());
    }
    if let Some(quality) = params.quality.as_deref().filter(|s| !s.is_empty()) {
      q.append_pair("quality", quality);
    }
    q.append_pair(
      "minimum_rating",
      &normalize_minimum_rating(params.minimum_rating).to_string(),
    );
    if let Some(term) = params.query_term.as_deref().filter(|s| !s.is_empty()) {
      q.append_pair("query_term", term);
    }
    if let Some(genre) = params.genre.as_deref().filter(|s| !s.is_empty()) {
      q.append_pair("genre", genre);
    }
    if let Some(sort) = params.sort_by.as_deref().filter(|s| !s.is_empty()) {
      q.append_pair("sort_by", sort);
    }
    if let Some(order) = params.order_by.as_deref().filter(|s| !s.is_empty()) {
      q.append_pair("order_by", order);
    }
    if let Some(true) = params.with_rt_ratings {
      q.append_pair("with_rt_ratings", "true");
    }
  }
  Ok(url)
}

/// Build the `movie_details.json` URL for the given params.
pub fn movie_details_url(base: &str, params: &YtsMovieDetailsParams) -> Result<url::Url> {
  let mut url = endpoint_url(base, "movie_details.json")?;
  {
    let mut q = url.query_pairs_mut();
    match (params.movie_id, params.imdb_id.as_deref()) {
      (Some(id), _) => {
        q.append_pair("movie_id", &id.to_string());
      }
      (None, Some(imdb)) => {
        q.append_pair("imdb_id", imdb);
      }
      (None, None) => anyhow::bail!("movie_details requires movie_id or imdb_id"),
    }
    if let Some(true) = params.with_images {
      q.append_pair("with_images", "true");
    }
    if let Some(true) = params.with_cast {
      q.append_pair("with_cast", "true");
    }
  }
  Ok(url)
}

/// Build a single-`movie_id` endpoint URL (`movie_suggestions`, …).
pub fn movie_id_url(base: &str, endpoint: &str, params: &YtsMovieIdParams) -> Result<url::Url> {
  let mut url = endpoint_url(base, endpoint)?;
  url
    .query_pairs_mut()
    .append_pair("movie_id", &params.movie_id.to_string());
  Ok(url)
}

/// Build a `magnet:` URL for a torrent, per the YTS docs:
/// `magnet:?xt=urn:btih:<HASH>&dn=<url-encoded name>&tr=<tracker>…`.
///
/// `hash` must be the 40-char hex info-hash the API returns per torrent;
/// `trackers` defaults to [`YTS_DEFAULT_TRACKERS`] when empty.
pub fn magnet_url(hash: &str, name: &str, trackers: &[&str]) -> Result<String> {
  let hash = hash.trim();
  if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
    anyhow::bail!("invalid torrent hash for magnet url: {hash:?} (want 40 hex chars)");
  }
  let name: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect();
  let mut magnet = format!("magnet:?xt=urn:btih:{hash}&dn={name}");
  let trackers = if trackers.is_empty() {
    YTS_DEFAULT_TRACKERS
  } else {
    trackers
  };
  for tr in trackers {
    use std::fmt::Write as _;
    write!(magnet, "&tr={tr}").expect("writing to String cannot fail");
  }
  Ok(magnet)
}

async fn get_json<T>(url: url::Url) -> Result<YtsEnvelope<T>>
where
  T: serde::de::DeserializeOwned,
{
  let response = http_client()?
    .get(url.clone())
    .send()
    .await
    .with_context(|| format!("YTS request failed: {url}"))?;
  let status = response.status();
  if !status.is_success() {
    anyhow::bail!("YTS request failed with HTTP {status}: {url}");
  }
  // The API emits explicit nulls for unknown fields (seen live: synopsis,
  // summary). Our models use #[serde(default)] for absent keys, which does
  // not cover explicit null — so strip nulls first and let defaults apply.
  let mut raw: serde_json::Value = response
    .json()
    .await
    .with_context(|| format!("failed to decode YTS response: {url}"))?;
  strip_nulls(&mut raw);
  let envelope: YtsEnvelope<T> =
    serde_json::from_value(raw).with_context(|| format!("failed to decode YTS response: {url}"))?;
  if envelope.status != "ok" {
    anyhow::bail!("YTS API error: {}", envelope.status_message);
  }
  Ok(envelope)
}

/// Recursively drop nulls: null-valued object keys are removed (so
/// `#[serde(default)]` applies) and null array elements are filtered out.
fn strip_nulls(value: &mut serde_json::Value) {
  match value {
    serde_json::Value::Object(map) => {
      map.retain(|_, v| !v.is_null());
      for v in map.values_mut() {
        strip_nulls(v);
      }
    }
    serde_json::Value::Array(items) => {
      items.retain(|v| !v.is_null());
      for v in items.iter_mut() {
        strip_nulls(v);
      }
    }
    _ => {}
  }
}

/// List / search movies.
pub async fn list_movies(params: &YtsListMoviesParams) -> Result<YtsEnvelope<YtsMovieListData>> {
  get_json(list_movies_url(YTS_API_BASE_URL, params)?).await
}

/// Full information about one movie.
pub async fn movie_details(
  params: &YtsMovieDetailsParams,
) -> Result<YtsEnvelope<YtsMovieDetailsData>> {
  get_json(movie_details_url(YTS_API_BASE_URL, params)?).await
}

/// Up to 4 related movies.
pub async fn movie_suggestions(params: &YtsMovieIdParams) -> Result<YtsEnvelope<YtsMovieListData>> {
  get_json(movie_id_url(
    YTS_API_BASE_URL,
    "movie_suggestions.json",
    params,
  )?)
  .await
}

/// Parental guide ratings for one movie.
pub async fn movie_parental_guides(
  params: &YtsMovieIdParams,
) -> Result<YtsEnvelope<YtsParentalGuidesData>> {
  get_json(movie_id_url(
    YTS_API_BASE_URL,
    "movie_parental_guides.json",
    params,
  )?)
  .await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn list_url_uses_documented_param_names() {
    let url = list_movies_url(
      YTS_API_BASE_URL,
      &YtsListMoviesParams {
        limit: Some(15),
        quality: Some("1080p".to_string()),
        minimum_rating: Some(7),
        query_term: Some("matrix".to_string()),
        genre: Some("sci-fi".to_string()),
        sort_by: Some("seeds".to_string()),
        order_by: Some("desc".to_string()),
        with_rt_ratings: Some(true),
        page: Some(2),
      },
    )
    .unwrap()
    .to_string();
    assert!(url.starts_with("https://movies-api.accel.li/api/v2/list_movies.json?"));
    for expected in [
      "limit=15",
      "quality=1080p",
      "minimum_rating=7",
      "query_term=matrix",
      "genre=sci-fi",
      "sort_by=seeds",
      "order_by=desc",
      "with_rt_ratings=true",
      "page=2",
    ] {
      assert!(url.contains(expected), "missing {expected} in {url}");
    }
    // The documented param is `sort_by`, never the bare `sort` alias.
    assert!(
      !url.contains("sort=seeds"),
      "must not use bare sort alias: {url}"
    );
  }

  #[test]
  fn list_url_clamps_limit_and_rating() {
    let url = list_movies_url(
      YTS_API_BASE_URL,
      &YtsListMoviesParams {
        limit: Some(500),
        minimum_rating: Some(42),
        ..Default::default()
      },
    )
    .unwrap()
    .to_string();
    assert!(
      url.contains("limit=20"),
      "out-of-range limit falls back to 20: {url}"
    );
    assert!(
      url.contains("minimum_rating=9"),
      "rating clamps to 9: {url}"
    );
  }

  #[test]
  fn details_url_requires_an_id() {
    assert!(movie_details_url(YTS_API_BASE_URL, &YtsMovieDetailsParams::default()).is_err());
    let url = movie_details_url(
      YTS_API_BASE_URL,
      &YtsMovieDetailsParams {
        movie_id: Some(10),
        with_cast: Some(true),
        ..Default::default()
      },
    )
    .unwrap()
    .to_string();
    assert!(url.contains("movie_details.json?"));
    assert!(url.contains("movie_id=10"));
    assert!(url.contains("with_cast=true"));
  }

  #[test]
  fn envelope_decodes_real_list_payload() {
    let raw = serde_json::json!({
      "status": "ok",
      "status_message": "Query was successful",
      "data": {
        "movie_count": 1,
        "limit": 20,
        "page_number": 1,
        "movies": [{
          "id": 10,
          "title": "13",
          "year": 2010,
          "rating": 6.0,
          "runtime": 91,
          "genres": ["Action"],
          "torrents": [{
            "url": "https://yts.gg/torrent/download/ABC",
            "hash": "ABC",
            "quality": "720p",
            "type": "web",
            "is_repack": "0",
            "video_codec": "x264",
            "bit_depth": "8",
            "audio_channels": "2.0",
            "seeds": 5,
            "peers": 2,
            "size": "700 MB",
            "size_bytes": 734003200,
            "date_uploaded": "2026-01-01 00:00:00",
            "date_uploaded_unix": 1767225600
          }]
        }]
      }
    });
    let env: YtsEnvelope<YtsMovieListData> = serde_json::from_value(raw).unwrap();
    assert_eq!(env.data.movies.len(), 1);
    assert_eq!(env.data.movies[0].torrents.len(), 1);
    assert_eq!(env.data.movies[0].torrents[0].torrent_type, "web");
  }

  #[test]
  fn magnet_url_matches_documented_format() {
    let magnet = magnet_url(
      "F27D21006A58C4073663B9363E7B2F2FBE8452F3",
      "The Lost Generation",
      &[],
    )
    .unwrap();
    assert!(
      magnet.starts_with(
        "magnet:?xt=urn:btih:F27D21006A58C4073663B9363E7B2F2FBE8452F3&dn=The+Lost+Generation&tr="
      ),
      "unexpected magnet: {magnet}"
    );
    for tr in YTS_DEFAULT_TRACKERS {
      assert!(
        magnet.contains(&format!("&tr={tr}")),
        "missing tracker {tr}"
      );
    }
    assert_eq!(magnet.matches("&tr=").count(), 8);
  }

  #[test]
  fn magnet_url_rejects_bad_hash() {
    assert!(magnet_url("", "x", &[]).is_err());
    assert!(magnet_url("zzzz", "x", &[]).is_err());
    assert!(magnet_url("ABCDEF", "x", &[]).is_err());
  }

  #[test]
  fn envelope_decodes_details_with_nulls() {
    // Real shape: fresh uploads carry explicit nulls (seen live on id 78351).
    let mut raw: serde_json::Value = serde_json::json!({
      "status": "ok",
      "status_message": "Query was successful",
      "data": {"movie": {
        "id": 78351,
        "imdb_code": "tt31501724",
        "title": "The Lost Generation",
        "year": 2025,
        "rating": 3.3,
        "runtime": 72,
        "genres": ["Drama", "Romance"],
        "synopsis": null,
        "description_full": "Fiesta.",
        "summary": null,
        "torrents": []
      }}
    });
    // get_json strips nulls before decoding — mirror that here.
    strip_nulls(&mut raw);
    let env: YtsEnvelope<YtsMovieDetailsData> = serde_json::from_value(raw).unwrap();
    assert_eq!(env.data.movie.imdb_code, "tt31501724");
    assert_eq!(env.data.movie.synopsis, "");
  }

  /// Hits the real API — ignored by default so CI stays offline.
  /// Run with: `cargo test -p popcorntime-yts-client -- --ignored`
  #[tokio::test]
  #[ignore]
  async fn live_list_movies_returns_movies() {
    let env = list_movies(&YtsListMoviesParams {
      limit: Some(2),
      ..Default::default()
    })
    .await
    .expect("live YTS list_movies failed");
    assert_eq!(env.status, "ok");
    assert!(!env.data.movies.is_empty(), "expected at least one movie");
  }
}
