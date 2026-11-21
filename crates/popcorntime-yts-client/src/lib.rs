pub mod client;
pub mod consts;
pub mod types;

pub use client::{
  list_movies, list_movies_url, magnet_url, movie_details, movie_details_url, movie_id_url,
  movie_parental_guides, movie_suggestions,
};
pub use consts::{YTS_API_BASE_URL, YTS_DEFAULT_TRACKERS};
pub use types::{
  YtsCast, YtsEnvelope, YtsListMoviesParams, YtsMovie, YtsMovieDetailsData, YtsMovieDetailsParams,
  YtsMovieIdParams, YtsMovieListData, YtsParentalGuide, YtsParentalGuidesData, YtsTorrent,
};
