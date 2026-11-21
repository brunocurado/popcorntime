//! Tauri commands backed by the YTS API v2
//! (`https://movies-api.accel.li/api/v2/`, see <https://yts.gg/api>).
//!
//! These commands are independent of the GraphQL backend: no account,
//! no auth token, no `GRAPHQL_SERVER` needed. YTS is a free no-key REST
//! API, so the commands deliberately skip `AuthorizationService`.

use crate::error::Error;
use popcorntime_yts_client::ExtraTorrent;
use popcorntime_yts_client::{
  YtsEnvelope, YtsListMoviesParams, YtsMovieDetailsData, YtsMovieDetailsParams, YtsMovieIdParams,
  YtsMovieListData, YtsParentalGuidesData,
};
use tracing::instrument;

#[tauri::command(async)]
#[specta::specta]
#[instrument(err(Debug))]
pub async fn yts_list_movies(
  params: YtsListMoviesParams,
) -> Result<YtsEnvelope<YtsMovieListData>, Error> {
  popcorntime_yts_client::list_movies(&params)
    .await
    .map_err(Into::into)
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(err(Debug))]
pub async fn yts_movie_details(
  params: YtsMovieDetailsParams,
) -> Result<YtsEnvelope<YtsMovieDetailsData>, Error> {
  popcorntime_yts_client::movie_details(&params)
    .await
    .map_err(Into::into)
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(err(Debug))]
pub async fn yts_movie_suggestions(
  params: YtsMovieIdParams,
) -> Result<YtsEnvelope<YtsMovieListData>, Error> {
  popcorntime_yts_client::movie_suggestions(&params)
    .await
    .map_err(Into::into)
}

#[tauri::command(async)]
#[specta::specta]
#[instrument(err(Debug))]
pub async fn yts_movie_parental_guides(
  params: YtsMovieIdParams,
) -> Result<YtsEnvelope<YtsParentalGuidesData>, Error> {
  popcorntime_yts_client::movie_parental_guides(&params)
    .await
    .map_err(Into::into)
}

/// Build a `magnet:` URL for a YTS torrent (hash + display name).
/// Trackers default to the documented recommended list.
#[tauri::command(async)]
#[specta::specta]
#[instrument(err(Debug))]
pub async fn yts_magnet_url(hash: String, name: String) -> Result<String, Error> {
  popcorntime_yts_client::magnet_url(&hash, &name, &[]).map_err(Into::into)
}

/// Extra torrents for a movie from Torrentio (multi-indexer) and The Pirate
/// Bay, keyed by IMDb id. Sources fail independently.
#[tauri::command(async)]
#[specta::specta]
#[instrument(err(Debug))]
pub async fn extra_sources(imdb_code: String) -> Result<Vec<ExtraTorrent>, Error> {
  popcorntime_yts_client::extra_sources(&imdb_code)
    .await
    .map_err(Into::into)
}
