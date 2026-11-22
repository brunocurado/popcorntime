//! Live smoke test: `cargo run -p popcorntime-stream --example smoke -- <magnet> <imdb>`
//! Streams the main video, proves Range reads + ffprobe work, then lists
//! subtitles matched by the OpenSubtitles hash and drops everything.
use popcorntime_stream::{StreamEngine, subtitles};
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let mut args = std::env::args().skip(1);
  let magnet = args.next().expect("magnet");
  let imdb = args.next().expect("imdb");
  let dir = std::env::temp_dir().join("popcorn-smoke");
  let engine = StreamEngine::start(dir.clone()).await?;
  let t0 = Instant::now();
  let info = engine.play(&magnet).await?;
  println!("PLAY {:?} after {:?}", info, t0.elapsed());
  // first bytes + a seek near the end, like a player does
  let c = reqwest::Client::new();
  for range in [
    "bytes=0-65535",
    &format!("bytes={}-", info.file_size as u64 - 65536),
  ] {
    let t = Instant::now();
    let r = c
      .get(&info.url)
      .header("Range", range)
      .timeout(Duration::from_secs(180))
      .send()
      .await?;
    let st = r.status();
    let ct = r.headers().get("content-type").cloned();
    let cr = r.headers().get("content-range").cloned();
    let b = r.bytes().await?;
    println!(
      "RANGE {range} -> {st} {ct:?} {cr:?} {} bytes in {:?}",
      b.len(),
      t.elapsed()
    );
  }
  let (hash, size) = engine.current_osdb_hash().await?;
  println!("OSDB {hash} {size}");
  let subs = subtitles::list(&imdb, Some((hash, size))).await?;
  println!(
    "SUBS {}: {:?}",
    subs.len(),
    subs.iter().take(4).map(|s| &s.label).collect::<Vec<_>>()
  );
  if let Some(s) = subs.first() {
    let vtt = subtitles::fetch_vtt(&s.url).await?;
    println!(
      "VTT {} chars, head: {:?}",
      vtt.len(),
      vtt.chars().take(80).collect::<String>()
    );
  }
  println!("URL {}", info.url);
  if let Ok(wait) = std::env::var("SMOKE_HOLD") {
    // keep serving so an external player (ffprobe) can hit the URL
    tokio::time::sleep(Duration::from_secs(wait.parse().unwrap_or(30))).await;
  }
  println!("STATUS {:?}", engine.status().await?);
  engine.stop().await?;
  let left = std::fs::read_dir(&dir)?.count();
  println!("STOPPED, entries left in cache dir: {left}");
  Ok(())
}
