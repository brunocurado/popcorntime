//! Fallback subtitles from the official OpenSubtitles.com REST API, used
//! only when the free Stremio addon has nothing in Portuguese (typically very
//! recent releases).
//!
//! Credentials (API key + optional username/password) live in the macOS
//! Keychain, never in files or logs. The free account allows 20 downloads a
//! day; a download is only spent when the user (or the auto-select) actually
//! picks one of these tracks.

use crate::subtitles::{SubtitleTrack, srt_to_vtt};
use anyhow::{Context, Result};
use keyring::{Credential, default::default_credential_builder};
use serde::{Deserialize, Serialize};
use std::{
  fmt,
  time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub const API_BASE: &str = "https://api.opensubtitles.com/api/v1";
/// The API requires a named client in the User-Agent.
pub const USER_AGENT: &str = concat!("PopcornTime v", env!("CARGO_PKG_VERSION"));
/// Track URLs carrying an OpenSubtitles file id instead of a download link.
pub const TRACK_SCHEME: &str = "opensubtitles:";
const KEYRING_SERVICE: &str = "Popcorn Time";
const KEYRING_USER: &str = "opensubtitles";
/// Tokens are valid for 24 h; refresh a bit earlier.
const TOKEN_TTL: Duration = Duration::from_secs(20 * 3600);

#[derive(Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Credentials {
  pub api_key: String,
  #[serde(default)]
  pub username: Option<String>,
  #[serde(default)]
  pub password: Option<String>,
}

impl fmt::Debug for Credentials {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Credentials")
      .field("api_key", &"****")
      .field("username", &self.username)
      .field("password", &self.password.as_ref().map(|_| "****"))
      .finish()
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Default)]
pub struct OpenSubtitlesStatus {
  pub configured: bool,
  pub username: Option<String>,
  /// Downloads left today, when known.
  pub remaining: Option<u32>,
}

/// Where credentials are kept (Keychain in the app, memory in tests).
pub trait SecretStore: Send + Sync {
  fn load(&self) -> Result<Option<Credentials>>;
  fn save(&self, c: &Credentials) -> Result<()>;
  fn clear(&self) -> Result<()>;
}

pub struct KeychainStore {
  credential: Box<Credential>,
}

impl KeychainStore {
  pub fn new() -> Result<Self> {
    Ok(Self {
      credential: default_credential_builder().build(None, KEYRING_SERVICE, KEYRING_USER)?,
    })
  }
}

impl SecretStore for KeychainStore {
  fn load(&self) -> Result<Option<Credentials>> {
    match self.credential.get_secret() {
      Ok(bytes) => Ok(Some(
        serde_json::from_slice(&bytes).context("keychain entry")?,
      )),
      Err(keyring::Error::NoEntry) => Ok(None),
      Err(e) => Err(e).context("failed to read OpenSubtitles keychain entry"),
    }
  }

  fn save(&self, c: &Credentials) -> Result<()> {
    self.credential.set_secret(&serde_json::to_vec(c)?)?;
    Ok(())
  }

  fn clear(&self) -> Result<()> {
    match self.credential.delete_credential() {
      Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
      Err(e) => Err(e.into()),
    }
  }
}

#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<Option<Credentials>>);

impl SecretStore for MemoryStore {
  fn load(&self) -> Result<Option<Credentials>> {
    Ok(self.0.lock().unwrap().clone())
  }
  fn save(&self, c: &Credentials) -> Result<()> {
    *self.0.lock().unwrap() = Some(c.clone());
    Ok(())
  }
  fn clear(&self) -> Result<()> {
    *self.0.lock().unwrap() = None;
    Ok(())
  }
}

struct Session {
  token: String,
  base: String,
  at: Instant,
}

pub struct OpenSubtitles {
  store: Box<dyn SecretStore>,
  api_base: String,
  session: Mutex<Option<Session>>,
  remaining: Mutex<Option<u32>>,
}

#[derive(Deserialize)]
struct LoginResponse {
  token: String,
  #[serde(default)]
  base_url: Option<String>,
  #[serde(default)]
  user: Option<LoginUser>,
}

#[derive(Deserialize)]
struct LoginUser {
  #[serde(default)]
  allowed_downloads: Option<u32>,
}

#[derive(Deserialize)]
struct SearchResponse {
  #[serde(default)]
  data: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct SearchItem {
  attributes: SearchAttributes,
}

#[derive(Deserialize)]
struct SearchAttributes {
  #[serde(default)]
  language: String,
  #[serde(default)]
  release: String,
  #[serde(default)]
  download_count: u64,
  #[serde(default)]
  moviehash_match: bool,
  #[serde(default)]
  machine_translated: bool,
  #[serde(default)]
  ai_translated: bool,
  #[serde(default)]
  files: Vec<SearchFile>,
}

#[derive(Deserialize)]
struct SearchFile {
  file_id: u64,
}

#[derive(Deserialize)]
struct DownloadResponse {
  link: String,
  #[serde(default)]
  remaining: Option<i64>,
}

/// What to look for.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
  pub imdb: String,
  pub episode: Option<(u32, u32)>,
  pub moviehash: Option<String>,
  /// OpenSubtitles.com codes, e.g. `pt-pt`, `pt-br`, `en`.
  pub languages: Vec<String>,
}

/// Map OpenSubtitles.com language codes to the addon's 3-letter codes so the
/// same ranking applies.
pub fn addon_lang(code: &str) -> &'static str {
  match code.to_ascii_lowercase().as_str() {
    "pt-pt" | "pt" => "por",
    "pt-br" => "pob",
    "en" => "eng",
    "es" => "spa",
    "fr" => "fre",
    _ => "",
  }
}

/// `/subtitles` URL. The API wants lower-case values and parameters in
/// alphabetical order (it redirects otherwise); IMDb ids without `tt`.
pub fn search_url(base: &str, q: &Query) -> Result<String> {
  let digits = q.imdb.trim().trim_start_matches("tt");
  anyhow::ensure!(
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()),
    "invalid IMDb id"
  );
  let id: u64 = digits.parse()?;
  let mut params: Vec<(String, String)> = vec![];
  if let Some((s, e)) = q.episode {
    params.push(("episode_number".into(), e.to_string()));
    params.push(("parent_imdb_id".into(), id.to_string()));
    params.push(("season_number".into(), s.to_string()));
  } else {
    params.push(("imdb_id".into(), id.to_string()));
  }
  let mut langs: Vec<String> = q.languages.iter().map(|l| l.to_ascii_lowercase()).collect();
  langs.sort();
  if !langs.is_empty() {
    params.push(("languages".into(), langs.join(",")));
  }
  if let Some(h) = q.moviehash.as_deref().filter(|h| h.len() == 16) {
    params.push(("moviehash".into(), h.to_ascii_lowercase()));
  }
  params.sort();
  let qs = params
    .iter()
    .map(|(k, v)| format!("{k}={v}"))
    .collect::<Vec<_>>()
    .join("&");
  Ok(format!("{}/subtitles?{qs}", base.trim_end_matches('/')))
}

fn label(lang: &str) -> &'static str {
  match lang {
    "por" => "Português (PT)",
    "pob" => "Português (BR)",
    "eng" => "English",
    "spa" => "Español",
    "fre" => "Français",
    _ => "Other",
  }
}

/// Turn search results into tracks: exact-file matches first, then human
/// translations by popularity; machine/AI translations last.
fn to_tracks(items: Vec<SearchItem>, preferred: &[&str]) -> Vec<SubtitleTrack> {
  let mut rows: Vec<(usize, bool, bool, u64, String, u64, String)> = items
    .into_iter()
    .filter_map(|i| {
      let a = i.attributes;
      let lang = addon_lang(&a.language);
      let pref = preferred.iter().position(|p| *p == lang)?;
      let file = a.files.first()?.file_id;
      Some((
        pref,
        !a.moviehash_match,
        a.machine_translated || a.ai_translated,
        u64::MAX - a.download_count,
        lang.to_string(),
        file,
        a.release,
      ))
    })
    .collect();
  rows.sort_by_key(|x| (x.0, x.1, x.2, x.3));
  let mut count = std::collections::HashMap::new();
  rows
    .into_iter()
    .take(12)
    .map(|(_, unmatched, machine, _, lang, file, release)| {
      let n = count.entry(lang.clone()).or_insert(0);
      *n += 1;
      let mut l = format!("{} · OpenSubtitles.com", label(&lang));
      if *n > 1 {
        l = format!("{} #{n} · OpenSubtitles.com", label(&lang));
      }
      if !unmatched {
        l.push_str(" ✓");
      }
      if machine {
        l.push_str(" (auto)");
      }
      let _ = release;
      SubtitleTrack {
        id: format!("os-{file}"),
        lang,
        label: l,
        url: format!("{TRACK_SCHEME}{file}"),
      }
    })
    .collect()
}

fn client() -> Result<reqwest::Client> {
  reqwest::ClientBuilder::new()
    .user_agent(USER_AGENT)
    .timeout(Duration::from_secs(15))
    .build()
    .context("opensubtitles http client")
}

async fn api_error(res: reqwest::Response) -> anyhow::Error {
  let status = res.status();
  let body = res.text().await.unwrap_or_default();
  let msg = serde_json::from_str::<serde_json::Value>(&body)
    .ok()
    .and_then(|v| {
      v.get("message")
        .and_then(|m| m.as_str())
        .map(str::to_string)
    })
    .unwrap_or_else(|| body.chars().take(160).collect());
  anyhow::anyhow!("OpenSubtitles {status}: {msg}")
}

impl OpenSubtitles {
  pub fn new(store: Box<dyn SecretStore>) -> Self {
    Self::with_base(store, API_BASE)
  }

  pub fn with_base(store: Box<dyn SecretStore>, api_base: &str) -> Self {
    Self {
      store,
      api_base: api_base.trim_end_matches('/').to_string(),
      session: Mutex::new(None),
      remaining: Mutex::new(None),
    }
  }

  pub async fn status(&self) -> OpenSubtitlesStatus {
    match self.store.load() {
      Ok(Some(c)) => OpenSubtitlesStatus {
        configured: true,
        username: c.username,
        remaining: *self.remaining.lock().await,
      },
      _ => OpenSubtitlesStatus::default(),
    }
  }

  /// Validate (log in, or a cheap keyed call without a user) and only then
  /// store the credentials.
  pub async fn save(&self, c: Credentials) -> Result<OpenSubtitlesStatus> {
    let c = Credentials {
      api_key: c.api_key.trim().to_string(),
      username: c
        .username
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty()),
      password: c.password.filter(|p| !p.is_empty()),
    };
    anyhow::ensure!(!c.api_key.is_empty(), "API key is empty");
    anyhow::ensure!(
      c.username.is_some() == c.password.is_some(),
      "username and password go together"
    );
    *self.session.lock().await = None;
    if c.username.is_some() {
      self.login(&c).await?;
    } else {
      // `/infos/*` answer 200 to any key; a search is what really checks it.
      let res = client()?
        .get(format!(
          "{}/subtitles?imdb_id=133093&languages=pt-pt",
          self.api_base
        ))
        .header("Api-Key", &c.api_key)
        .send()
        .await?;
      if !res.status().is_success() {
        return Err(api_error(res).await);
      }
    }
    self.store.save(&c)?;
    Ok(self.status().await)
  }

  pub async fn clear(&self) -> Result<()> {
    *self.session.lock().await = None;
    *self.remaining.lock().await = None;
    self.store.clear()
  }

  async fn login(&self, c: &Credentials) -> Result<()> {
    let (Some(user), Some(pass)) = (&c.username, &c.password) else {
      return Ok(());
    };
    let res = client()?
      .post(format!("{}/login", self.api_base))
      .header("Api-Key", &c.api_key)
      .json(&serde_json::json!({ "username": user, "password": pass }))
      .send()
      .await
      .context("OpenSubtitles login request failed")?;
    if !res.status().is_success() {
      return Err(api_error(res).await);
    }
    let body: LoginResponse = res.json().await.context("OpenSubtitles login payload")?;
    let base = match body.base_url.filter(|b| !b.is_empty()) {
      Some(host) if !host.starts_with("http") => format!("https://{host}/api/v1"),
      Some(url) => url,
      None => self.api_base.clone(),
    };
    if let Some(n) = body.user.and_then(|u| u.allowed_downloads) {
      self.remaining.lock().await.get_or_insert(n);
    }
    *self.session.lock().await = Some(Session {
      token: body.token,
      base,
      at: Instant::now(),
    });
    Ok(())
  }

  /// (credentials, api base, bearer token) — logging in when needed.
  async fn context(&self) -> Result<Option<(Credentials, String, Option<String>)>> {
    let Some(c) = self.store.load()? else {
      return Ok(None);
    };
    if c.username.is_some() {
      let fresh = matches!(&*self.session.lock().await, Some(s) if s.at.elapsed() < TOKEN_TTL);
      if !fresh {
        self.login(&c).await?;
      }
      let s = self.session.lock().await;
      let s = s.as_ref().context("not logged in")?;
      return Ok(Some((c, s.base.clone(), Some(s.token.clone()))));
    }
    Ok(Some((c, self.api_base.clone(), None)))
  }

  /// Search; `None` when no credentials are configured.
  pub async fn search(&self, q: &Query, preferred: &[&str]) -> Result<Option<Vec<SubtitleTrack>>> {
    let Some((c, base, token)) = self.context().await? else {
      return Ok(None);
    };
    let url = search_url(&base, q)?;
    let mut req = client()?.get(&url).header("Api-Key", &c.api_key);
    if let Some(t) = &token {
      req = req.bearer_auth(t);
    }
    let res = req.send().await.context("OpenSubtitles search failed")?;
    if !res.status().is_success() {
      return Err(api_error(res).await);
    }
    let body: SearchResponse = res.json().await.context("OpenSubtitles search payload")?;
    let tracks = to_tracks(body.data, preferred);
    tracing::info!(found = tracks.len(), "opensubtitles.com fallback search");
    Ok(Some(tracks))
  }

  /// Spend one download and return the subtitle as WebVTT.
  pub async fn fetch_vtt(&self, track_url: &str) -> Result<String> {
    let file_id: u64 = track_url
      .strip_prefix(TRACK_SCHEME)
      .context("not an OpenSubtitles track")?
      .parse()
      .context("bad OpenSubtitles file id")?;
    let (c, base, token) = self
      .context()
      .await?
      .context("OpenSubtitles is not configured")?;
    let mut req = client()?
      .post(format!("{base}/download"))
      .header("Api-Key", &c.api_key)
      .json(&serde_json::json!({ "file_id": file_id }));
    if let Some(t) = &token {
      req = req.bearer_auth(t);
    }
    let res = req.send().await.context("OpenSubtitles download failed")?;
    if !res.status().is_success() {
      return Err(api_error(res).await);
    }
    let body: DownloadResponse = res.json().await.context("OpenSubtitles download payload")?;
    if let Some(r) = body.remaining.filter(|r| *r >= 0) {
      *self.remaining.lock().await = Some(r as u32);
    }
    anyhow::ensure!(
      body.link.starts_with("https://"),
      "unexpected download link"
    );
    let bytes = client()?
      .get(&body.link)
      .send()
      .await?
      .error_for_status()?
      .bytes()
      .await?;
    let text = match String::from_utf8(bytes.to_vec()) {
      Ok(s) => s,
      Err(e) => e.into_bytes().iter().map(|b| *b as char).collect(),
    };
    Ok(srt_to_vtt(&text))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn builds_sorted_lowercase_queries() {
    let movie = Query {
      imdb: "tt0133093".into(),
      episode: None,
      moviehash: Some("6A4594416FCCC866".into()),
      languages: vec!["pt-BR".into(), "pt-PT".into()],
    };
    assert_eq!(
      search_url(API_BASE, &movie).unwrap(),
      format!(
        "{API_BASE}/subtitles?imdb_id=133093&languages=pt-br,pt-pt&moviehash=6a4594416fccc866"
      )
    );
    let ep = Query {
      imdb: "tt0903747".into(),
      episode: Some((1, 2)),
      moviehash: None,
      languages: vec!["pt-pt".into()],
    };
    assert_eq!(
      search_url(API_BASE, &ep).unwrap(),
      format!(
        "{API_BASE}/subtitles?episode_number=2&languages=pt-pt&parent_imdb_id=903747&season_number=1"
      )
    );
    assert!(
      search_url(
        API_BASE,
        &Query {
          imdb: "x".into(),
          ..ep
        }
      )
      .is_err()
    );
  }

  #[test]
  fn ranks_hash_matches_and_humans_first() {
    let items: Vec<SearchItem> = serde_json::from_str::<SearchResponse>(
      r#"{"data":[
        {"attributes":{"language":"pt-BR","download_count":900,"files":[{"file_id":1}]}},
        {"attributes":{"language":"pt-PT","download_count":10,"machine_translated":true,"files":[{"file_id":2}]}},
        {"attributes":{"language":"pt-PT","download_count":5,"moviehash_match":true,"files":[{"file_id":3}]}},
        {"attributes":{"language":"de","download_count":5,"files":[{"file_id":4}]}},
        {"attributes":{"language":"pt-PT","download_count":50,"files":[]}}
      ]}"#,
    )
    .unwrap()
    .data;
    let t = to_tracks(items, &["por", "pob", "eng"]);
    let ids: Vec<_> = t.iter().map(|x| x.url.as_str()).collect();
    assert_eq!(
      ids,
      ["opensubtitles:3", "opensubtitles:2", "opensubtitles:1"]
    );
    assert_eq!(t[0].label, "Português (PT) · OpenSubtitles.com ✓");
    assert_eq!(t[1].label, "Português (PT) #2 · OpenSubtitles.com (auto)");
    assert_eq!(t[2].lang, "pob");
  }

  #[test]
  fn credentials_never_print_secrets() {
    let c = Credentials {
      api_key: "SECRETKEY".into(),
      username: Some("bruno".into()),
      password: Some("hunter2".into()),
    };
    let dbg = format!("{c:?}");
    assert!(!dbg.contains("SECRETKEY") && !dbg.contains("hunter2"));
    assert!(dbg.contains("bruno"));
  }

  /// Tiny fake of the three endpoints used.
  async fn fake_api() -> String {
    use axum::{
      Json, Router,
      http::HeaderMap,
      routing::{get, post},
    };
    let check = |h: &HeaderMap| h.get("api-key").and_then(|v| v.to_str().ok()) == Some("good");
    let app = Router::new()
      .route(
        "/login",
        post(move |h: HeaderMap, Json(b): Json<serde_json::Value>| async move {
          if !check(&h) || b["password"] != "pw" {
            return (axum::http::StatusCode::UNAUTHORIZED, Json(serde_json::json!({"message":"Invalid credentials"})));
          }
          (axum::http::StatusCode::OK, Json(serde_json::json!({"token":"tok","base_url":"","user":{"allowed_downloads":20}})))
        }),
      )
      .route(
        "/subtitles",
        get(move |h: HeaderMap| async move {
          assert_eq!(h.get("authorization").unwrap(), "Bearer tok");
          Json(serde_json::json!({"data":[{"attributes":{"language":"pt-PT","download_count":3,"files":[{"file_id":77}]}}]}))
        }),
      )
      .route(
        "/download",
        post(move |Json(b): Json<serde_json::Value>| async move {
          assert_eq!(b["file_id"], 77);
          Json(serde_json::json!({"link":"https://example.invalid/x.srt","remaining":19}))
        }),
      );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://127.0.0.1:{port}")
  }

  #[tokio::test]
  async fn validates_before_saving_then_searches_with_token() {
    let base = fake_api().await;
    let os = OpenSubtitles::with_base(Box::<MemoryStore>::default(), &base);
    assert!(!os.status().await.configured);
    assert!(
      os.search(
        &Query {
          imdb: "tt1".into(),
          episode: None,
          moviehash: None,
          languages: vec!["pt-pt".into()]
        },
        &["por"]
      )
      .await
      .unwrap()
      .is_none()
    );

    let bad = os
      .save(Credentials {
        api_key: "good".into(),
        username: Some("bruno".into()),
        password: Some("wrong".into()),
      })
      .await
      .unwrap_err();
    assert!(format!("{bad:#}").contains("Invalid credentials"));
    assert!(
      !os.status().await.configured,
      "nothing stored after a failed login"
    );

    let st = os
      .save(Credentials {
        api_key: " good ".into(),
        username: Some("bruno".into()),
        password: Some("pw".into()),
      })
      .await
      .unwrap();
    assert_eq!(
      (st.configured, st.username.as_deref(), st.remaining),
      (true, Some("bruno"), Some(20))
    );

    let q = Query {
      imdb: "tt35004585".into(),
      episode: None,
      moviehash: None,
      languages: vec!["pt-pt".into()],
    };
    let tracks = os.search(&q, &["por"]).await.unwrap().unwrap();
    assert_eq!(tracks[0].url, "opensubtitles:77");

    os.clear().await.unwrap();
    assert!(!os.status().await.configured);
  }

  #[tokio::test]
  #[ignore = "hits the live network"]
  async fn live_rejects_a_bad_key_without_storing_it() {
    let os = OpenSubtitles::new(Box::<MemoryStore>::default());
    let err = os
      .save(Credentials {
        api_key: "not-a-real-key".into(),
        username: None,
        password: None,
      })
      .await
      .unwrap_err();
    assert!(format!("{err:#}").contains("OpenSubtitles 403"), "{err:#}");
    assert!(!os.status().await.configured);
  }
}
