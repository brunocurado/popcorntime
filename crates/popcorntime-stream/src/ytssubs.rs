//! Movie subtitles from yts-subs.com (YIFY Subtitles). No API: the movie page
//! is scraped, each subtitle page carries a base64 `data-link` to a .zip with
//! the .srt inside. Movies only — the site has no series.

use crate::subtitles::{SubtitleTrack, srt_to_vtt};
use anyhow::{Context, Result};
use std::{io::Read, time::Duration};

pub const YTS_SUBS_BASE: &str = "https://yts-subs.com";
/// Track URLs carrying a yts-subs page path, resolved on download.
pub const TRACK_SCHEME: &str = "ytssubs:";
/// Keep the picker short: best few per language.
const PER_LANG: usize = 3;

fn client() -> Result<reqwest::Client> {
  reqwest::ClientBuilder::new()
    .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")
    .timeout(Duration::from_secs(15))
    .build()
    .context("yts-subs http client")
}

pub fn lang_code(name: &str) -> &'static str {
  match name.trim() {
    "Portuguese" => "por",
    "Brazilian Portuguese" => "pob",
    "English" => "eng",
    "Spanish" => "spa",
    "French" => "fre",
    _ => "",
  }
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
  let i = s.find(start)? + start.len();
  let j = s[i..].find(end)? + i;
  Some(&s[i..j])
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
  pub lang: &'static str,
  pub rating: i32,
  pub path: String,
}

/// Rows of the movie page in the wanted languages.
pub fn parse_movie_page(html: &str) -> Vec<Row> {
  html
    .split("<tr")
    .skip(1)
    .filter(|r| r.contains("data-id"))
    .filter_map(|r| {
      let r = r.split("</tr>").next().unwrap_or(r);
      let lang = lang_code(between(r, "class=\"sub-lang\">", "<")?);
      if lang.is_empty() {
        return None;
      }
      let path = between(r, "href=\"/subtitles/", "\"")?;
      if path.is_empty() || !path.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
      }
      let rating = between(r, "class=\"rating-cell\">", "</td>")
        .and_then(|cell| between(cell, ">", "<"))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
      Some(Row {
        lang,
        rating,
        path: format!("/subtitles/{path}"),
      })
    })
    .collect()
}

/// Best `PER_LANG` rows per language, as player tracks.
pub fn to_tracks(mut rows: Vec<Row>, preferred: &[&str]) -> Vec<SubtitleTrack> {
  rows.sort_by_key(|r| {
    (
      preferred.iter().position(|p| *p == r.lang).unwrap_or(99),
      -r.rating,
    )
  });
  let mut count = std::collections::HashMap::new();
  rows
    .into_iter()
    .filter(|r| preferred.contains(&r.lang))
    .filter(|r| {
      let n = count.entry(r.lang).or_insert(0);
      *n += 1;
      *n <= PER_LANG
    })
    .map(|r| {
      let base = crate::subtitles::lang_label(r.lang);
      let label = if r.rating > 0 {
        format!("{base} · YTS-Subs ★{}", r.rating)
      } else {
        format!("{base} · YTS-Subs")
      };
      SubtitleTrack {
        id: format!("ys-{}", r.path.trim_start_matches("/subtitles/")),
        lang: r.lang.to_string(),
        label,
        url: format!("{TRACK_SCHEME}{}", r.path),
      }
    })
    .collect()
}

pub async fn list(imdb: &str, preferred: &[&str]) -> Result<Vec<SubtitleTrack>> {
  let imdb = imdb.trim();
  anyhow::ensure!(
    imdb.starts_with("tt") && imdb.len() > 2 && imdb[2..].chars().all(|c| c.is_ascii_digit()),
    "invalid IMDb id"
  );
  let res = client()?
    .get(format!("{YTS_SUBS_BASE}/movie-imdb/{imdb}"))
    .send()
    .await
    .context("yts-subs request failed")?;
  if res.status() == reqwest::StatusCode::NOT_FOUND {
    return Ok(Vec::new()); // movie not on the site
  }
  let html = res.error_for_status()?.text().await?;
  Ok(to_tracks(parse_movie_page(&html), preferred))
}

/// `data-link` on a subtitle page is the base64 of the .zip URL.
pub fn parse_zip_link(html: &str) -> Option<String> {
  use base64::Engine;
  let encoded = between(html, "data-link=\"", "\"")?;
  let bytes = base64::engine::general_purpose::STANDARD
    .decode(encoded)
    .ok()?;
  let url = String::from_utf8(bytes).ok()?;
  (url.starts_with("https://") && url.ends_with(".zip")).then_some(url)
}

/// First .srt inside the zip, as text (UTF-8, else Latin-1).
pub fn srt_from_zip(bytes: &[u8]) -> Result<String> {
  let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("bad subtitle zip")?;
  for i in 0..zip.len() {
    let mut f = zip.by_index(i)?;
    if !f.name().to_ascii_lowercase().ends_with(".srt") {
      continue;
    }
    let mut raw = Vec::new();
    f.read_to_end(&mut raw)?;
    return Ok(match String::from_utf8(raw) {
      Ok(s) => s,
      Err(e) => e.into_bytes().iter().map(|b| *b as char).collect(),
    });
  }
  anyhow::bail!("no .srt in the subtitle zip")
}

pub async fn fetch_vtt(track_url: &str) -> Result<String> {
  let path = track_url
    .strip_prefix(TRACK_SCHEME)
    .filter(|p| p.starts_with("/subtitles/"))
    .context("not a yts-subs track")?;
  let c = client()?;
  let page = c
    .get(format!("{YTS_SUBS_BASE}{path}"))
    .send()
    .await?
    .error_for_status()?
    .text()
    .await?;
  let zip_url = parse_zip_link(&page).context("download link not found on yts-subs")?;
  let bytes = c
    .get(&zip_url)
    .send()
    .await?
    .error_for_status()?
    .bytes()
    .await?;
  Ok(srt_to_vtt(&srt_from_zip(&bytes)?))
}

#[cfg(test)]
mod tests {
  use super::*;

  const PAGE: &str = r#"<table>
  <tr data-id="1"><td class="rating-cell"> <span class="label label-success">2</span> </td>
    <td class="flag-cell"><span class="sub-lang">Arabic</span></td>
    <td><a href="/subtitles/the-matrix-1999-arabic-yify-119081">x</a></td></tr>
  <tr data-id="2"><td class="rating-cell"> <span class="label ">0</span> </td>
    <td class="flag-cell"><span class="flag flag-br"></span> <span class="sub-lang">Brazilian Portuguese</span></td>
    <td><a href="/subtitles/the-matrix-1999-brazilian-portuguese-yify-119086">x</a></td></tr>
  <tr data-id="3"><td class="rating-cell"> <span class="label label-success">5</span> </td>
    <td class="flag-cell"><span class="sub-lang">Portuguese</span></td>
    <td><a href="/subtitles/the-matrix-1999-portuguese-yify-1">x</a></td></tr>
  <tr><td>header row without data-id</td></tr>
  </table>"#;

  #[test]
  fn parses_rows_and_ranks_portuguese_first() {
    let rows = parse_movie_page(PAGE);
    assert_eq!(rows.len(), 2); // Arabic dropped
    let t = to_tracks(rows, &["por", "pob", "eng"]);
    assert_eq!(
      t[0].url,
      "ytssubs:/subtitles/the-matrix-1999-portuguese-yify-1"
    );
    assert_eq!(t[0].label, "Português (PT) · YTS-Subs ★5");
    assert_eq!(t[1].lang, "pob");
  }

  #[test]
  fn decodes_the_zip_link_and_reads_the_srt() {
    let page = r#"<a class="btn-icon download-subtitle" data-link="aHR0cHM6Ly9zdWJ0aXRsZXMueXRzLXN1YnMuY29tL3N1YnRpdGxlcy90aGUtbWF0cml4LTE5OTktYnJhemlsaWFuLXBvcnR1Z3Vlc2UteWlmeS0xMTkwODYuemlw">"#;
    assert_eq!(
      parse_zip_link(page).as_deref(),
      Some(
        "https://subtitles.yts-subs.com/subtitles/the-matrix-1999-brazilian-portuguese-yify-119086.zip"
      )
    );
    let mut buf = std::io::Cursor::new(Vec::new());
    {
      let mut w = zip::ZipWriter::new(&mut buf);
      w.start_file("readme.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
      std::io::Write::write_all(&mut w, b"yifysubtitles.org").unwrap();
      w.start_file(
        "Movie-Portuguese.srt",
        zip::write::SimpleFileOptions::default(),
      )
      .unwrap();
      std::io::Write::write_all(
        &mut w,
        "1\r\n00:00:01,000 --> 00:00:02,000\r\nEst\u{e1} pronto\r\n".as_bytes(),
      )
      .unwrap();
      w.finish().unwrap();
    }
    let srt = srt_from_zip(buf.get_ref()).unwrap();
    assert!(srt.contains("Está pronto"));
  }

  #[tokio::test]
  #[ignore = "hits the live network"]
  async fn live_matrix_has_portuguese_and_downloads() {
    let t = list("tt0133093", &["por", "pob"]).await.unwrap();
    assert!(t.iter().any(|x| x.lang == "por") && t.iter().any(|x| x.lang == "pob"));
    let vtt = fetch_vtt(&t[0].url).await.unwrap();
    assert!(vtt.starts_with("WEBVTT") && vtt.contains("-->"));
    assert!(list("tt35004585", &["por"]).await.unwrap().is_empty()); // not on the site
  }
}
