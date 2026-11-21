//! Typed models for the YTS API v2 (`https://movies-api.accel.li/api/v2/`).
//!
//! Shapes verified live against `list_movies`, `movie_details`,
//! `movie_suggestions` and `movie_parental_guides`. Every field the API
//! may omit is `Option`/`#[serde(default)]` so a missing key degrades to
//! `None`/empty instead of failing the whole response.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// Top-level envelope shared by every YTS endpoint.
#[derive(Debug, Clone, specta::Type, Serialize, Deserialize)]
pub struct YtsEnvelope<T> {
  pub status: String,
  #[serde(default)]
  pub status_message: String,
  pub data: T,
}

// ---------------------------------------------------------------------------
// Movies
// ---------------------------------------------------------------------------

/// A torrent file attached to a movie.
#[derive(Debug, Clone, specta::Type, Serialize, Deserialize)]
pub struct YtsTorrent {
  pub url: String,
  pub hash: String,
  #[serde(default)]
  pub quality: String,
  /// Torrent source type (`web`, `bluray`, …).
  #[serde(default, rename = "type")]
  pub torrent_type: String,
  /// The API encodes this as `"0"`/`"1"` strings.
  #[serde(default)]
  pub is_repack: String,
  #[serde(default)]
  pub video_codec: String,
  /// The API encodes this as a string (`"8"`).
  #[serde(default)]
  pub bit_depth: String,
  #[serde(default)]
  pub audio_channels: String,
  #[serde(default)]
  pub seeds: u64,
  #[serde(default)]
  pub peers: u64,
  #[serde(default)]
  pub size: String,
  #[serde(default)]
  pub size_bytes: u64,
  #[serde(default)]
  pub date_uploaded: String,
  #[serde(default)]
  pub date_uploaded_unix: u64,
}

/// A cast member (only present when `with_cast=true`).
#[derive(Debug, Clone, specta::Type, Serialize, Deserialize)]
pub struct YtsCast {
  pub name: String,
  #[serde(default)]
  pub character_name: String,
  #[serde(default)]
  pub url_small_image: String,
  #[serde(default)]
  pub imdb_code: String,
}

/// A movie as returned by list / details / suggestions.
#[derive(Debug, Clone, specta::Type, Serialize, Deserialize)]
pub struct YtsMovie {
  pub id: u64,
  #[serde(default)]
  pub url: String,
  #[serde(default)]
  pub imdb_code: String,
  #[serde(default)]
  pub title: String,
  #[serde(default)]
  pub title_english: String,
  #[serde(default)]
  pub title_long: String,
  #[serde(default)]
  pub slug: String,
  #[serde(default)]
  pub year: u16,
  #[serde(default)]
  pub rating: f64,
  #[serde(default)]
  pub runtime: u32,
  #[serde(default)]
  pub genres: Vec<String>,
  #[serde(default)]
  pub summary: String,
  #[serde(default)]
  pub description_full: String,
  #[serde(default)]
  pub description_intro: String,
  #[serde(default)]
  pub synopsis: String,
  #[serde(default)]
  pub yt_trailer_code: String,
  #[serde(default)]
  pub language: String,
  #[serde(default)]
  pub mpa_rating: String,
  #[serde(default)]
  pub background_image: String,
  #[serde(default)]
  pub background_image_original: String,
  #[serde(default)]
  pub small_cover_image: String,
  #[serde(default)]
  pub medium_cover_image: String,
  #[serde(default)]
  pub large_cover_image: String,
  #[serde(default)]
  pub state: String,
  #[serde(default)]
  pub torrents: Vec<YtsTorrent>,
  #[serde(default)]
  pub date_uploaded: String,
  #[serde(default)]
  pub date_uploaded_unix: u64,
  /// Details endpoint extras.
  #[serde(default)]
  pub like_count: u64,
  #[serde(default)]
  pub download_count: u64,
  #[serde(default)]
  pub medium_screenshot_image1: String,
  #[serde(default)]
  pub medium_screenshot_image2: String,
  #[serde(default)]
  pub medium_screenshot_image3: String,
  #[serde(default)]
  pub large_screenshot_image1: String,
  #[serde(default)]
  pub large_screenshot_image2: String,
  #[serde(default)]
  pub large_screenshot_image3: String,
  #[serde(default)]
  pub cast: Vec<YtsCast>,
}

// ---------------------------------------------------------------------------
// Endpoint payloads
// ---------------------------------------------------------------------------

/// `list_movies` / `movie_suggestions` payload.
#[derive(Debug, Clone, specta::Type, Serialize, Deserialize)]
pub struct YtsMovieListData {
  #[serde(default)]
  pub movie_count: u64,
  #[serde(default)]
  pub limit: u32,
  #[serde(default)]
  pub page_number: u32,
  #[serde(default)]
  pub movies: Vec<YtsMovie>,
}

/// `movie_details` payload.
#[derive(Debug, Clone, specta::Type, Serialize, Deserialize)]
pub struct YtsMovieDetailsData {
  pub movie: YtsMovie,
}

/// One parental guide entry.
#[derive(Debug, Clone, specta::Type, Serialize, Deserialize)]
pub struct YtsParentalGuide {
  #[serde(default, rename = "type")]
  pub guide_type: String,
  #[serde(default)]
  pub parental_guide_text: String,
}

/// `movie_parental_guides` payload.
#[derive(Debug, Clone, specta::Type, Serialize, Deserialize)]
pub struct YtsParentalGuidesData {
  #[serde(default)]
  pub parental_guide_count: u64,
  #[serde(default)]
  pub parental_guides: Vec<YtsParentalGuide>,
}

// ---------------------------------------------------------------------------
// Request params (Tauri-facing)
// ---------------------------------------------------------------------------

/// `list_movies` query params. Names mirror the API docs; `sort_by` is the
/// documented name (the docs' `sort=seeds` example is an alias the API also
/// accepts, but clients must send `sort_by`).
#[derive(Debug, Clone, Default, specta::Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YtsListMoviesParams {
  #[specta(optional)]
  pub limit: Option<u32>,
  #[specta(optional)]
  pub page: Option<u32>,
  #[specta(optional)]
  pub quality: Option<String>,
  #[specta(optional)]
  pub minimum_rating: Option<u8>,
  #[specta(optional)]
  pub query_term: Option<String>,
  #[specta(optional)]
  pub genre: Option<String>,
  #[specta(optional)]
  pub sort_by: Option<String>,
  #[specta(optional)]
  pub order_by: Option<String>,
  #[specta(optional)]
  pub with_rt_ratings: Option<bool>,
}

/// `movie_details` query params — pass `movie_id` or `imdb_id`.
#[derive(Debug, Clone, Default, specta::Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YtsMovieDetailsParams {
  #[specta(optional)]
  pub movie_id: Option<u64>,
  #[specta(optional)]
  pub imdb_id: Option<String>,
  #[specta(optional)]
  pub with_images: Option<bool>,
  #[specta(optional)]
  pub with_cast: Option<bool>,
}

/// `movie_suggestions` / `movie_parental_guides` query params.
#[derive(Debug, Clone, specta::Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YtsMovieIdParams {
  pub movie_id: u64,
}
