/// Canonical YTS API v2 base URL.
///
/// Per <https://yts.gg/api>, the old base `https://yts.gg/api/v2/` is
/// deprecated — integrations must use the host below. Only the `.json`
/// variants are used: `.jsonp` returns the same JSON payload and `.xml`
/// needs a separate parser, so neither is a correct machine format here.
pub const YTS_API_BASE_URL: &str = "https://movies-api.accel.li/api/v2";

/// Trackers the YTS docs recommend appending to self-built magnet URLs.
pub const YTS_DEFAULT_TRACKERS: &[&str] = &[
  "udp://glotorrents.pw:6969/announce",
  "udp://tracker.opentrackr.org:1337/announce",
  "udp://torrent.gresille.org:80/announce",
  "udp://tracker.openbittorrent.com:80",
  "udp://tracker.coppersurfer.tk:6969",
  "udp://tracker.leechers-paradise.org:6969",
  "udp://p4p.arenabg.ch:1337",
  "udp://tracker.internetwarriors.net:1337",
];
