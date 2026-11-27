/**
 * "Play automático": rank every torrent we know for a title and pick the one
 * most likely to start fast and play inside the app.
 */

export interface Candidate {
	hash: string;
	title: string;
	quality: string;
	seeds: number;
	/** human size, e.g. "1.85 GB" */
	size: string;
	source: string;
	fileIdx?: number | null;
	fileName?: string | null;
}

/** Same cap the stream engine enforces per file. */
export const CACHE_CAP_BYTES = 8 * 1024 ** 3;

const UNITS: Record<string, number> = {
	B: 1,
	KB: 1024,
	KIB: 1024,
	MB: 1024 ** 2,
	MIB: 1024 ** 2,
	GB: 1024 ** 3,
	GIB: 1024 ** 3,
	TB: 1024 ** 4,
	TIB: 1024 ** 4,
};

export function parseSize(size: string): number | null {
	const m = size.trim().match(/^([\d.,]+)\s*([KMGT]?i?B)$/i);
	if (!m?.[1] || !m[2]) return null;
	const n = Number(m[1].replace(",", "."));
	const unit = UNITS[m[2].toUpperCase()];
	return Number.isFinite(n) && unit ? n * unit : null;
}

const LOW_QUALITY =
	/\b(cam|camrip|hdcam|hd-?ts|ts|telesync|tc|telecine|scr|screener|dvdscr|workprint)\b/i;

function qualityScore(c: Candidate): number | null {
	const q = `${c.quality} ${c.title}`.toLowerCase();
	if (/\b3d\b/.test(q)) return null;
	if (/2160p|\b4k\b|\buhd\b/.test(q)) return 1.6;
	if (/1080p/.test(q)) return 3;
	if (/720p/.test(q)) return 2.2;
	if (/480p|\bsd\b/.test(q)) return 1;
	return 1.5;
}

export function episodeTag(season: number, episode: number): string {
	return `S${String(season).padStart(2, "0")}E${String(episode).padStart(2, "0")}`;
}

/** Score a torrent; `null` when it must never be auto-picked. */
export function scoreCandidate(
	c: Candidate,
	episode?: { season: number; episode: number }
): number | null {
	if (c.seeds < 1) return null;
	const text = `${c.title} ${c.fileName ?? ""}`;
	if (LOW_QUALITY.test(text) || /\bremux\b/i.test(text)) return null;
	const bytes = parseSize(c.size);
	if (bytes !== null && bytes > CACHE_CAP_BYTES) return null;
	const q = qualityScore(c);
	if (q === null) return null;

	let score = q * 2 + Math.log2(c.seeds + 1) * 1.2;
	if (c.seeds < 3) score -= 2;
	if (/x264|h\.?264|\bavc\b/i.test(text)) score += 0.6;
	if (/x265|hevc|h\.?265/i.test(text)) score += 0.3;
	// Dolby Vision without an HDR10 base shows purple/green on normal screens
	if (/\bdv\b|dolby.?vision/i.test(text) && !/hdr/i.test(text)) score -= 1.5;
	if (c.source === "YTS") score += 1; // small mp4s that play directly
	if (bytes === null) score -= 0.5;
	if (episode) {
		const tag = episodeTag(episode.season, episode.episode);
		const own = text.toUpperCase().replace(/\s+/g, ".");
		if (own.includes(tag)) score += 0.5;
		else if (!c.fileName && c.fileIdx == null) return null; // pack without a file hint
	}
	return score;
}

export function rankCandidates(
	list: Candidate[],
	episode?: { season: number; episode: number }
): Candidate[] {
	return list
		.map(c => ({ c, s: scoreCandidate(c, episode) }))
		.filter((x): x is { c: Candidate; s: number } => x.s !== null)
		.sort((a, b) => b.s - a.s)
		.map(x => x.c);
}

export function pickBest(
	list: Candidate[],
	episode?: { season: number; episode: number }
): Candidate | null {
	return rankCandidates(list, episode)[0] ?? null;
}
