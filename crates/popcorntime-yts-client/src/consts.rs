/// Canonical YTS API v2 base URL.
///
/// Per <https://yts.gg/api>, the old base `https://yts.gg/api/v2/` is
/// deprecated — integrations must use the host below. Only the `.json`
/// variants are used: `.jsonp` returns the same JSON payload and `.xml`
/// needs a separate parser, so neither is a correct machine format here.
pub const YTS_API_BASE_URL: &str = "https://movies-api.accel.li/api/v2";

/// Trackers appended to every magnet we build. The list the YTS docs give is
/// mostly dead (6 of 8 time out or no longer resolve); these answered a UDP
/// `connect` in under 0.6 s when checked.
pub const YTS_DEFAULT_TRACKERS: &[&str] = &[
  "udp://tracker.opentrackr.org:1337/announce",
  "udp://open.stealth.si:80/announce",
  "udp://tracker.torrent.eu.org:451/announce",
  "udp://open.demonii.com:1337/announce",
  "udp://tracker.dler.org:6969/announce",
  "udp://tracker.qu.ax:6969/announce",
  "udp://open.tracker.cl:1337/announce",
  "udp://tracker.theoks.net:6969/announce",
  "udp://tracker.internetwarriors.net:1337/announce",
  "udp://tracker.bittor.pw:1337/announce",
];
