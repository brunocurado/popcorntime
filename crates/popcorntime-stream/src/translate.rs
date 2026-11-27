//! English → Portuguese subtitle translation on the Mac itself (Apple's
//! Translation framework, through the embedded `pt-translate` helper).
//!
//! A job translates in chunks, starting at the cue where playback will
//! begin and moving forward, then wraps around to the earlier ones. The
//! player waits for a short lead (the first minutes) while the film buffers,
//! then plays; the job keeps running ahead — it is ~4-5× faster than the film.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
  path::{Path, PathBuf},
  process::Stdio,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
};
use tokio::io::AsyncWriteExt;

/// The compiled helper (empty when built without swiftc).
static HELPER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pt-translate"));
/// First chunk small, so the first minutes are ready quickly.
const FIRST_CHUNK: usize = 25;
const CHUNK: usize = 60;
pub const TARGET_LANG: &str = "pt-PT";
const MAX_LINE: usize = 42;

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
  pub start: f64,
  pub end: f64,
  /// The `00:00:01.000 --> 00:00:02.000 …` line, kept verbatim.
  pub timing: String,
  pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Default)]
pub struct TranslationProgress {
  pub done: u32,
  pub total: u32,
  /// Film position (s) up to which every cue from the start point is ready.
  pub ready_until: f64,
  /// Cues translated so far, as WebVTT.
  pub vtt: String,
  pub finished: bool,
  pub error: Option<String>,
}

fn vtt_time(s: &str) -> Option<f64> {
  let s = s.split_whitespace().next()?;
  let parts: Vec<&str> = s.split(':').collect();
  let (h, m, rest) = match parts.as_slice() {
    [h, m, rest] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, *rest),
    [m, rest] => (0.0, m.parse::<f64>().ok()?, *rest),
    _ => return None,
  };
  let sec: f64 = rest.replace(',', ".").parse().ok()?;
  Some(h * 3600.0 + m * 60.0 + sec)
}

pub fn parse_cues(vtt: &str) -> Vec<Cue> {
  let body = vtt.replace("\r\n", "\n").replace('\r', "\n");
  body
    .split("\n\n")
    .filter_map(|block| {
      let lines: Vec<&str> = block.lines().collect();
      let i = lines.iter().position(|l| l.contains("-->"))?;
      let timing = lines[i].trim().to_string();
      let (a, b) = timing.split_once("-->")?;
      let text = lines[i + 1..].join("\n").trim().to_string();
      if text.is_empty() {
        return None;
      }
      Some(Cue {
        start: vtt_time(a)?,
        end: vtt_time(b)?,
        timing,
        text,
      })
    })
    .collect()
}

fn strip_markup(s: &str) -> String {
  let mut out = String::new();
  let mut depth_angle = false;
  let mut depth_brace = false;
  for c in s.chars() {
    match c {
      '<' => depth_angle = true,
      '>' if depth_angle => depth_angle = false,
      '{' => depth_brace = true,
      '}' if depth_brace => depth_brace = false,
      _ if !depth_angle && !depth_brace => out.push(c),
      _ => {}
    }
  }
  out
}

/// One string per cue for the translator; dialogue lines keep their dashes.
pub fn cue_source(text: &str) -> String {
  strip_markup(text)
    .lines()
    .map(str::trim)
    .filter(|l| !l.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

/// Back to subtitle shape: one line per speaker for "- A - B" dialogues,
/// long lines split near the middle.
pub fn reshape(text: &str) -> String {
  let t = text.trim();
  if t.starts_with('-') && t[1..].contains(" - ") {
    let parts: Vec<String> = t
      .split(" - ")
      .map(|p| p.trim().trim_start_matches('-').trim().to_string())
      .filter(|p| !p.is_empty())
      .map(|p| format!("- {p}"))
      .collect();
    if parts.len() > 1 {
      return parts.join("\n");
    }
  }
  if t.chars().count() <= MAX_LINE {
    return t.to_string();
  }
  let words: Vec<&str> = t.split(' ').collect();
  let half = t.chars().count() / 2;
  let mut best = (usize::MAX, 0);
  let mut pos = 0;
  for (i, w) in words.iter().enumerate().take(words.len().saturating_sub(1)) {
    pos += w.chars().count() + 1;
    let d = pos.abs_diff(half);
    if d < best.0 {
      best = (d, i + 1);
    }
  }
  if best.1 == 0 {
    return t.to_string();
  }
  format!(
    "{}\n{}",
    words[..best.1].join(" "),
    words[best.1..].join(" ")
  )
}

/// Order to translate in: from the cue at `start_at` onwards, then the rest.
pub fn priority_order(cues: &[Cue], start_at: f64) -> Vec<usize> {
  let first = cues.iter().position(|c| c.end >= start_at).unwrap_or(0);
  (first..cues.len()).chain(0..first).collect()
}

pub fn build_vtt(cues: &[Cue], translated: &[Option<String>]) -> String {
  let mut out = String::from("WEBVTT\n\n");
  for (c, t) in cues.iter().zip(translated) {
    if let Some(t) = t {
      out.push_str(&c.timing);
      out.push('\n');
      out.push_str(t);
      out.push_str("\n\n");
    }
  }
  out
}

/// Seconds of film, from `start_at`, covered without a gap.
pub fn ready_until(cues: &[Cue], translated: &[Option<String>], start_at: f64) -> f64 {
  for (c, t) in cues.iter().zip(translated) {
    if c.end < start_at {
      continue;
    }
    if t.is_none() {
      // silence before an untranslated cue needs nothing
      return c.start.max(start_at);
    }
  }
  f64::INFINITY
}

#[derive(Serialize)]
struct HelperIn<'a> {
  source: &'a str,
  target: &'a str,
  texts: &'a [String],
}

#[derive(Deserialize)]
struct HelperOut {
  #[serde(default)]
  texts: Option<Vec<String>>,
  #[serde(default)]
  error: Option<String>,
  #[serde(default)]
  status: Option<String>,
}

pub fn available() -> bool {
  !HELPER.is_empty()
}

/// Write the embedded helper next to the app cache (once per build).
pub fn install_helper(dir: &Path) -> Result<PathBuf> {
  anyhow::ensure!(
    available(),
    "subtitle translation is not available in this build"
  );
  std::fs::create_dir_all(dir)?;
  let path = dir.join("pt-translate");
  let fresh = std::fs::metadata(&path)
    .map(|m| m.len() == HELPER.len() as u64)
    .unwrap_or(false)
    && std::fs::read(&path).map(|b| b == HELPER).unwrap_or(false);
  if !fresh {
    let tmp = dir.join("pt-translate.tmp");
    std::fs::write(&tmp, HELPER)?;
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &path)?;
  }
  Ok(path)
}

/// Translate a batch with the helper.
pub async fn translate_batch(exe: &Path, texts: &[String]) -> Result<Vec<String>> {
  let mut child = tokio::process::Command::new(exe)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true)
    .spawn()
    .context("failed to start the translator")?;
  let input = serde_json::to_vec(&HelperIn {
    source: "en",
    target: TARGET_LANG,
    texts,
  })?;
  {
    let mut stdin = child.stdin.take().context("translator stdin")?;
    stdin.write_all(&input).await?;
  }
  let out = child.wait_with_output().await?;
  let parsed: HelperOut = serde_json::from_slice(&out.stdout).context("translator output")?;
  if let Some(err) = parsed.error {
    if parsed.status.as_deref() == Some("supported") {
      anyhow::bail!(
        "falta o pacote de tradução Inglês → Português: Definições do Sistema › Geral › Idioma e região › Idiomas de tradução"
      );
    }
    anyhow::bail!("translator: {err}");
  }
  let texts_out = parsed.texts.context("translator returned nothing")?;
  anyhow::ensure!(
    texts_out.len() == texts.len(),
    "translator returned a partial batch"
  );
  Ok(texts_out)
}

struct JobState {
  translated: Vec<Option<String>>,
  error: Option<String>,
  finished: bool,
}

pub struct TranslationJob {
  cues: Vec<Cue>,
  start_at: f64,
  state: Mutex<JobState>,
  cancelled: AtomicBool,
}

impl TranslationJob {
  /// Start translating `vtt` in the background, beginning at `start_at`.
  pub fn start(exe: PathBuf, vtt: &str, start_at: f64) -> Arc<Self> {
    Self::start_with(vtt, start_at, move |texts| {
      let exe = exe.clone();
      async move { translate_batch(&exe, &texts).await }
    })
  }

  /// Same, with any batch translator (tests use a fake one).
  pub fn start_with<F, Fut>(vtt: &str, start_at: f64, translate: F) -> Arc<Self>
  where
    F: Fn(Vec<String>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Vec<String>>> + Send,
  {
    let cues = parse_cues(vtt);
    let n = cues.len();
    let job = Arc::new(Self {
      cues,
      start_at,
      state: Mutex::new(JobState {
        translated: vec![None; n],
        error: None,
        finished: n == 0,
      }),
      cancelled: AtomicBool::new(false),
    });
    let worker = job.clone();
    tokio::spawn(async move {
      let order = priority_order(&worker.cues, worker.start_at);
      let mut i = 0;
      let mut first = true;
      while i < order.len() {
        if worker.cancelled.load(Ordering::Relaxed) {
          return;
        }
        let size = if first { FIRST_CHUNK } else { CHUNK };
        first = false;
        let idx: Vec<usize> = order[i..(i + size).min(order.len())].to_vec();
        i += idx.len();
        let texts: Vec<String> = idx
          .iter()
          .map(|&k| cue_source(&worker.cues[k].text))
          .collect();
        match translate(texts).await {
          Ok(out) => {
            let mut st = worker.state.lock().unwrap();
            for (k, t) in idx.into_iter().zip(out) {
              let t = t.trim();
              // an empty answer keeps the cue out rather than blank on screen
              if !t.is_empty() {
                st.translated[k] = Some(reshape(t));
              }
            }
          }
          Err(e) => {
            tracing::warn!("subtitle translation stopped: {e:#}");
            let mut st = worker.state.lock().unwrap();
            st.error = Some(format!("{e:#}"));
            st.finished = true;
            return;
          }
        }
      }
      worker.state.lock().unwrap().finished = true;
      tracing::info!(cues = worker.cues.len(), "subtitle translation finished");
    });
    job
  }

  pub fn cancel(&self) {
    self.cancelled.store(true, Ordering::Relaxed);
  }

  pub fn progress(&self) -> TranslationProgress {
    let st = self.state.lock().unwrap();
    let done = st.translated.iter().filter(|t| t.is_some()).count() as u32;
    let until = ready_until(&self.cues, &st.translated, self.start_at);
    TranslationProgress {
      done,
      total: self.cues.len() as u32,
      ready_until: if until.is_finite() { until } else { 1e9 },
      vtt: build_vtt(&self.cues, &st.translated),
      finished: st.finished,
      error: st.error.clone(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const VTT: &str = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:03.000\n<i>Are you okay?</i>\n\n2\n00:01:00.000 --> 00:01:02.000\n- You are a lifesaver.\n- Yeah, man.\n\n3\n00:05:00.000 --> 00:05:04.000 align:start\nI could have sworn the guy said south, but then all of a sudden we are off the road.\n";

  #[test]
  fn parses_and_prepares_cues() {
    let c = parse_cues(VTT);
    assert_eq!(c.len(), 3);
    assert_eq!((c[1].start, c[1].end), (60.0, 62.0));
    assert_eq!(c[2].timing, "00:05:00.000 --> 00:05:04.000 align:start");
    assert_eq!(cue_source(&c[0].text), "Are you okay?");
    assert_eq!(
      cue_source(&c[1].text),
      "- You are a lifesaver. - Yeah, man."
    );
  }

  #[test]
  fn reshapes_dialogue_and_long_lines() {
    assert_eq!(
      reshape("- És um salvador de vidas. - Sim, mano."),
      "- És um salvador de vidas.\n- Sim, mano."
    );
    let long =
      reshape("Eu podia jurar que o tipo disse sul, mas de repente estamos fora da estrada.");
    let lines: Vec<_> = long.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| l.chars().count() <= 45));
    assert_eq!(reshape("Estás bem?"), "Estás bem?");
  }

  #[test]
  fn starts_at_the_resume_point() {
    let c = parse_cues(VTT);
    assert_eq!(priority_order(&c, 0.0), [0, 1, 2]);
    assert_eq!(priority_order(&c, 61.0), [1, 2, 0]);
    let t = vec![None, Some("x".into()), None];
    assert_eq!(ready_until(&c, &t, 61.0), 300.0); // up to the next untranslated cue
    let all = vec![Some("a".into()), Some("b".into()), Some("c".into())];
    assert!(ready_until(&c, &all, 0.0).is_infinite());
  }

  #[tokio::test]
  async fn job_translates_from_start_point_and_reports_progress() {
    let job = TranslationJob::start_with(VTT, 61.0, |texts| async move {
      Ok(texts.iter().map(|t| format!("PT:{t}")).collect())
    });
    for _ in 0..50 {
      if job.progress().finished {
        break;
      }
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let p = job.progress();
    assert!(p.finished && p.error.is_none());
    assert_eq!((p.done, p.total), (3, 3));
    assert!(
      p.vtt
        .starts_with("WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nPT:Are you okay?")
    );
    assert!(
      p.vtt
        .contains("00:05:00.000 --> 00:05:04.000 align:start\nPT:")
    );
  }

  #[tokio::test]
  async fn job_reports_a_missing_language_pack() {
    let job =
      TranslationJob::start_with(VTT, 0.0, |_| async move { anyhow::bail!("falta o pacote") });
    for _ in 0..50 {
      if job.progress().finished {
        break;
      }
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(job.progress().error.unwrap().contains("falta o pacote"));
  }

  #[tokio::test]
  #[ignore = "uses the macOS translation service (~20 s)"]
  async fn live_helper_translates_to_portuguese() {
    let dir = std::env::temp_dir().join(format!("pt-tr-{}", std::process::id()));
    let exe = install_helper(&dir).unwrap();
    let out = translate_batch(
      &exe,
      &[
        "Are you okay?".to_string(),
        "- You are a lifesaver. - Yeah, man.".to_string(),
      ],
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 2);
    assert!(out[0].contains("bem"), "{out:?}");
    let _ = std::fs::remove_dir_all(dir);
  }

  #[tokio::test]
  #[ignore = "network + macOS translation service (~30 s)"]
  async fn live_first_batch_is_ready_in_about_half_a_minute() {
    let subs = crate::subtitles::list_for("tt0903747", Some((1, 2)), None)
      .await
      .unwrap();
    let en = subs
      .iter()
      .find(|s| s.lang == "eng")
      .expect("an English subtitle");
    let vtt = crate::subtitles::fetch_vtt(&en.url).await.unwrap();
    let dir = std::env::temp_dir().join(format!("pt-tr-live-{}", std::process::id()));
    let exe = install_helper(&dir).unwrap();
    let t0 = std::time::Instant::now();
    let job = TranslationJob::start(exe, &vtt, 600.0);
    loop {
      let p = job.progress();
      assert!(p.error.is_none(), "{:?}", p.error);
      if p.done >= FIRST_CHUNK as u32 {
        let secs = t0.elapsed().as_secs_f64();
        println!(
          "FIRST_BATCH {} cues in {secs:.1}s, film covered 600s → {:.0}s, of {} cues",
          p.done, p.ready_until, p.total
        );
        let sample: Vec<&str> = p
          .vtt
          .lines()
          .filter(|l| !l.contains("-->") && !l.is_empty())
          .skip(1)
          .take(4)
          .collect();
        println!("SAMPLE {sample:?}");
        assert!(secs < 60.0);
        assert!(p.ready_until > 600.0);
        break;
      }
      assert!(t0.elapsed().as_secs() < 90, "too slow");
      tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    job.cancel();
    let _ = std::fs::remove_dir_all(dir);
  }
}
