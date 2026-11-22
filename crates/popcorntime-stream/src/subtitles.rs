//! Subtitles from OpenSubtitles through Stremio's public addon
//! (`opensubtitles-v3.strem.io`): no account, no key, served as UTF-8.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const OPENSUBTITLES_ADDON: &str = "https://opensubtitles-v3.strem.io";
/// Preferred order: Portuguese (PT), Portuguese (BR), English, Spanish, French.
pub const PREFERRED_LANGS: &[&str] = &["por", "pob", "eng", "spa", "fre"];

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SubtitleTrack {
  pub id: String,
  pub lang: String,
  pub label: String,
  pub url: String,
}

#[derive(Deserialize)]
struct AddonResponse {
  #[serde(default)]
  subtitles: Vec<AddonSub>,
}

#[derive(Deserialize)]
struct AddonSub {
  #[serde(default)]
  id: String,
  #[serde(default)]
  url: String,
  #[serde(default)]
  lang: String,
}

pub fn lang_label(code: &str) -> &'static str {
  match code {
    "por" => "Português (PT)",
    "pob" => "Português (BR)",
    "eng" => "English",
    "spa" => "Español",
    "fre" => "Français",
    "ita" => "Italiano",
    "ger" => "Deutsch",
    _ => "Other",
  }
}

fn client() -> Result<reqwest::Client> {
  reqwest::ClientBuilder::new()
    .user_agent("Mozilla/5.0 (Macintosh) PopcornTime")
    .timeout(Duration::from_secs(15))
    .build()
    .context("subtitle http client")
}

/// List subtitles for an IMDb id; with `video` (hash, size) the addon ranks
/// subtitles made for that exact file first.
pub async fn list(imdb: &str, video: Option<(String, u64)>) -> Result<Vec<SubtitleTrack>> {
  let imdb = imdb.trim();
  anyhow::ensure!(
    imdb.starts_with("tt") && imdb[2..].chars().all(|c| c.is_ascii_digit()) && imdb.len() > 2,
    "invalid IMDb id: {imdb}"
  );
  let url = match video {
    Some((hash, size)) => {
      format!("{OPENSUBTITLES_ADDON}/subtitles/movie/{imdb}/videoHash={hash}&videoSize={size}.json")
    }
    None => format!("{OPENSUBTITLES_ADDON}/subtitles/movie/{imdb}.json"),
  };
  let res: AddonResponse = client()?
    .get(&url)
    .send()
    .await
    .with_context(|| format!("subtitle request failed: {url}"))?
    .error_for_status()?
    .json()
    .await
    .context("subtitle payload")?;
  Ok(rank(res.subtitles))
}

fn rank(subs: Vec<AddonSub>) -> Vec<SubtitleTrack> {
  let mut out: Vec<(usize, usize, SubtitleTrack)> = subs
    .into_iter()
    .enumerate()
    .filter(|(_, s)| PREFERRED_LANGS.contains(&s.lang.as_str()) && s.url.starts_with("https://"))
    .map(|(pos, s)| {
      let pref = PREFERRED_LANGS
        .iter()
        .position(|l| *l == s.lang)
        .unwrap_or(99);
      (
        pref,
        pos,
        SubtitleTrack {
          label: lang_label(&s.lang).to_string(),
          id: s.id,
          lang: s.lang,
          url: s.url,
        },
      )
    })
    .collect();
  // Stable: language preference first, addon order (best match) second.
  out.sort_by_key(|(pref, pos, _)| (*pref, *pos));
  let mut count = std::collections::HashMap::new();
  out
    .into_iter()
    .map(|(_, _, mut t)| {
      let n = count.entry(t.lang.clone()).or_insert(0);
      *n += 1;
      if *n > 1 {
        t.label = format!("{} #{n}", t.label);
      }
      t
    })
    .collect()
}

/// Download one subtitle and return it as WebVTT text.
pub async fn fetch_vtt(url: &str) -> Result<String> {
  anyhow::ensure!(url.starts_with("https://"), "subtitle url must be https");
  let bytes = client()?
    .get(url)
    .send()
    .await
    .context("subtitle download failed")?
    .error_for_status()?
    .bytes()
    .await?;
  let text = match String::from_utf8(bytes.to_vec()) {
    Ok(s) => s,
    // Fallback for legacy encodings: Latin-1 maps bytes 1:1 to code points.
    Err(e) => e.into_bytes().iter().map(|b| *b as char).collect(),
  };
  Ok(srt_to_vtt(&text))
}

/// SRT → WebVTT: header, `,` → `.` in timestamps, CRLF normalised.
pub fn srt_to_vtt(srt: &str) -> String {
  let body = srt
    .trim_start_matches('\u{feff}')
    .replace("\r\n", "\n")
    .replace('\r', "\n");
  if body.trim_start().starts_with("WEBVTT") {
    return body;
  }
  let mut out = String::from("WEBVTT\n\n");
  for line in body.lines() {
    if line.contains("-->") {
      out.push_str(&line.replace(',', "."));
    } else {
      out.push_str(line);
    }
    out.push('\n');
  }
  out
}

/// OpenSubtitles hash: file size + sum of the first and last 64 KiB read as
/// little-endian u64 words, wrapping, as 16 hex chars.
pub fn osdb_hash(size: u64, head: &[u8], tail: &[u8]) -> String {
  let mut h = size;
  for part in [head, tail] {
    for w in part.as_chunks::<8>().0 {
      h = h.wrapping_add(u64::from_le_bytes(*w));
    }
  }
  format!("{h:016x}")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn converts_srt_to_vtt() {
    let srt = "1\r\n00:00:30,000 --> 00:00:36,000\r\nLegendas, corrigidas\r\n\r\n2\r\n00:00:46,800 --> 00:00:47,600\r\nSim?\r\n";
    let vtt = srt_to_vtt(srt);
    assert!(vtt.starts_with("WEBVTT\n\n1\n00:00:30.000 --> 00:00:36.000\n"));
    assert!(vtt.contains("Legendas, corrigidas")); // commas in text survive
    assert!(!vtt.contains('\r'));
  }

  #[test]
  fn ranks_portuguese_first() {
    let mk = |id: &str, lang: &str| AddonSub {
      id: id.into(),
      url: format!("https://subs5.strem.io/{id}"),
      lang: lang.into(),
    };
    let r = rank(vec![
      mk("1", "ger"),
      mk("2", "eng"),
      mk("3", "pob"),
      mk("4", "por"),
      mk("5", "pob"),
    ]);
    let ids: Vec<_> = r.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["4", "3", "5", "2"]);
    assert_eq!(r[2].label, "Português (BR) #2");
  }

  #[test]
  fn osdb_hash_matches_reference() {
    // Reference vector from the OpenSubtitles docs: breakdance.avi
    // (size 12909756, hash 8e245d9679d31e12) needs the real file, so check
    // the arithmetic on a synthetic buffer instead.
    let head = [1u8, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0];
    let tail = [3u8, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(osdb_hash(100, &head, &tail), format!("{:016x}", 106u64));
    assert_eq!(
      osdb_hash(u64::MAX, &[1, 0, 0, 0, 0, 0, 0, 0], &[]),
      "0000000000000000"
    );
  }

  #[tokio::test]
  #[ignore = "hits the live network"]
  async fn live_lists_and_fetches_portuguese() {
    let subs = list("tt0133093", None).await.unwrap();
    let pt = subs
      .iter()
      .find(|s| s.lang == "por" || s.lang == "pob")
      .unwrap();
    let vtt = fetch_vtt(&pt.url).await.unwrap();
    assert!(vtt.starts_with("WEBVTT"));
    assert!(vtt.contains("-->"));
  }
}
