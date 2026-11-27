import { invoke } from "@tauri-apps/api/core";

/**
 * Hand-written bindings for the YTS Tauri commands.
 *
 * These mirror `crates/popcorntime-yts-client` (snake_case wire keys).
 * The next `pnpm dev` run regenerates `@/tauri/types` via specta and will
 * include these commands — then this module can delegate to it.
 */

export interface YtsTorrent {
	url: string;
	hash: string;
	quality: string;
	type: string;
	is_repack: string;
	video_codec: string;
	bit_depth: string;
	audio_channels: string;
	seeds: number;
	peers: number;
	size: string;
	size_bytes: number;
	date_uploaded: string;
	date_uploaded_unix: number;
}

export interface YtsCast {
	name: string;
	character_name: string;
	url_small_image: string;
	imdb_code: string;
}

export interface YtsMovie {
	id: number;
	url: string;
	imdb_code: string;
	title: string;
	title_english: string;
	title_long: string;
	slug: string;
	year: number;
	rating: number;
	runtime: number;
	genres: string[];
	summary: string;
	description_full: string;
	description_intro: string;
	synopsis: string;
	yt_trailer_code: string;
	language: string;
	mpa_rating: string;
	background_image: string;
	background_image_original: string;
	small_cover_image: string;
	medium_cover_image: string;
	large_cover_image: string;
	state: string;
	torrents: YtsTorrent[];
	date_uploaded: string;
	date_uploaded_unix: number;
	like_count: number;
	download_count: number;
	medium_screenshot_image1: string;
	medium_screenshot_image2: string;
	medium_screenshot_image3: string;
	large_screenshot_image1: string;
	large_screenshot_image2: string;
	large_screenshot_image3: string;
	cast: YtsCast[];
}

export interface YtsMovieListData {
	movie_count: number;
	limit: number;
	page_number: number;
	movies: YtsMovie[];
}

export interface YtsEnvelope<T> {
	status: string;
	status_message: string;
	data: T;
}

export interface YtsListMoviesParams {
	limit?: number;
	page?: number;
	quality?: string;
	minimumRating?: number;
	queryTerm?: string;
	genre?: string;
	sortBy?: string;
	orderBy?: string;
	withRtRatings?: boolean;
}

export interface YtsMovieDetailsParams {
	movieId?: number;
	imdbId?: string;
	withImages?: boolean;
	withCast?: boolean;
}

export async function ytsListMovies(
	params: YtsListMoviesParams
): Promise<YtsEnvelope<YtsMovieListData>> {
	// NOTE: raw `invoke` throws on command error (no {status} envelope),
	// so the YTS envelope (which itself carries `status: "ok"`) passes through untouched.
	return (await invoke("yts_list_movies", { params })) as YtsEnvelope<YtsMovieListData>;
}

export async function ytsMovieDetails(
	params: YtsMovieDetailsParams
): Promise<YtsEnvelope<{ movie: YtsMovie }>> {
	return (await invoke("yts_movie_details", { params })) as YtsEnvelope<{ movie: YtsMovie }>;
}

export async function ytsMagnetUrl(hash: string, name: string): Promise<string> {
	return (await invoke("yts_magnet_url", { hash, name })) as string;
}

export async function enableGuestSession(): Promise<void> {
	await invoke("enable_guest_session");
}

/**
 * Merge a details payload over its list item so no field ever regresses to
 * empty once details arrive: every empty details value (missing, null, "",
 * 0, []) keeps the list value. Numbers use 0 as empty — list and details
 * agree on them in practice (verified live).
 */
export function mergeYtsMovie(base: YtsMovie, extra: YtsMovie | null): YtsMovie {
	if (!extra) return base;
	const pick = <K extends keyof YtsMovie>(key: K): YtsMovie[K] => {
		const a = extra[key];
		const b = base[key];
		if (Array.isArray(a)) return (a.length > 0 ? a : b) as YtsMovie[K];
		if (typeof a === "number") return (a !== 0 ? a : b) as YtsMovie[K];
		if (typeof a === "string") return (a !== "" ? a : b) as YtsMovie[K];
		return (a ?? b) as YtsMovie[K];
	};
	return {
		...base,
		id: base.id,
		url: pick("url"),
		imdb_code: pick("imdb_code"),
		title: pick("title"),
		title_english: pick("title_english"),
		title_long: pick("title_long"),
		slug: pick("slug"),
		year: pick("year"),
		rating: pick("rating"),
		runtime: pick("runtime"),
		genres: pick("genres"),
		summary: pick("summary"),
		description_full: pick("description_full"),
		description_intro: pick("description_intro"),
		synopsis: pick("synopsis"),
		yt_trailer_code: pick("yt_trailer_code"),
		language: pick("language"),
		mpa_rating: pick("mpa_rating"),
		background_image: pick("background_image"),
		background_image_original: pick("background_image_original"),
		small_cover_image: pick("small_cover_image"),
		medium_cover_image: pick("medium_cover_image"),
		large_cover_image: pick("large_cover_image"),
		state: pick("state"),
		torrents: pick("torrents"),
		date_uploaded: pick("date_uploaded"),
		date_uploaded_unix: pick("date_uploaded_unix"),
		like_count: pick("like_count"),
		download_count: pick("download_count"),
		medium_screenshot_image1: pick("medium_screenshot_image1"),
		medium_screenshot_image2: pick("medium_screenshot_image2"),
		medium_screenshot_image3: pick("medium_screenshot_image3"),
		large_screenshot_image1: pick("large_screenshot_image1"),
		large_screenshot_image2: pick("large_screenshot_image2"),
		large_screenshot_image3: pick("large_screenshot_image3"),
		cast: pick("cast"),
	};
}

export interface ExtraTorrent {
	source: string;
	title: string;
	hash: string;
	quality: string;
	seeds: number;
	size: string;
	file_idx?: number | null;
	file_name?: string | null;
}

/** Torrentio (multi-indexer) + The Pirate Bay + KickassTorrents, by IMDb id. */
export async function extraSources(
	imdbCode: string,
	title?: string,
	year?: number
): Promise<ExtraTorrent[]> {
	return ((await invoke("extra_sources", {
		imdbCode,
		title: title ?? null,
		year: year ?? null,
	})) ?? []) as ExtraTorrent[];
}

/** Torrents for one episode (Torrentio with season-pack file hints + KAT). */
export async function episodeSources(
	imdbCode: string,
	show: string,
	season: number,
	episode: number
): Promise<ExtraTorrent[]> {
	return ((await invoke("episode_sources", { imdbCode, show, season, episode })) ??
		[]) as ExtraTorrent[];
}

export interface SeriesSummary {
	id: string;
	name: string;
	poster: string;
	year: string;
	rating: string;
}

export interface Episode {
	season: number;
	episode: number;
	title: string;
	overview: string;
	thumbnail: string;
	released: string;
}

export interface SeriesDetail extends SeriesSummary {
	background: string;
	description: string;
	runtime: string;
	genres: string[];
	trailer: string;
	episodes: Episode[];
}

export async function seriesCatalog(
	genre: string | undefined,
	search: string | undefined,
	skip: number
): Promise<SeriesSummary[]> {
	return ((await invoke("series_catalog", {
		genre: genre || null,
		search: search || null,
		skip,
	})) ?? []) as SeriesSummary[];
}

export async function seriesMeta(imdbCode: string): Promise<SeriesDetail> {
	return (await invoke("series_meta", { imdbCode })) as SeriesDetail;
}

export async function trailerUrl(youtubeId: string): Promise<string> {
	return (await invoke("trailer_url", { youtubeId })) as string;
}

export interface FileHint {
	file_idx?: number | null;
	file_name?: string | null;
	season?: number | null;
	episode?: number | null;
}

export interface StreamInfo {
	torrent_id: number;
	file_index: number;
	file_name: string;
	file_size: number;
	/** raw file over HTTP (VLC, subtitle hashing) */
	url: string;
	/** what the in-app <video> loads (raw file or HLS remux) */
	play_url: string | null;
	in_app: boolean;
	remuxed: boolean;
	/** seconds skipped at the start of a remux (resume) */
	time_offset: number;
	/** film length in seconds when known, else 0 */
	duration: number;
	/** why the in-app conversion didn't start, if it didn't */
	remux_error?: string | null;
}

export interface StreamStatus {
	state: string;
	progress_bytes: number;
	total_bytes: number;
	download_mbps: number;
	peers: number;
}

export interface SubtitleTrack {
	id: string;
	lang: string;
	label: string;
	url: string;
}

export async function streamPlay(
	magnet: string,
	hint?: FileHint,
	startAt?: number
): Promise<StreamInfo> {
	return (await invoke("stream_play", {
		magnet,
		hint: hint ?? null,
		startAt: startAt && startAt > 0 ? startAt : null,
	})) as StreamInfo;
}

/** Stop streaming; with an id, only if that torrent is still the current one. */
export async function streamStop(torrentId?: number): Promise<void> {
	await invoke("stream_stop", { torrentId: torrentId ?? null });
}

export async function streamStatus(): Promise<StreamStatus | null> {
	return (await invoke("stream_status")) as StreamStatus | null;
}

export async function subtitlesList(
	imdbCode: string,
	season?: number,
	episode?: number
): Promise<SubtitleTrack[]> {
	return (
		((await invoke("subtitles_list", {
			imdbCode,
			season: season ?? null,
			episode: episode ?? null,
		})) as SubtitleTrack[]) ?? []
	);
}

export async function subtitleVtt(url: string): Promise<string> {
	return (await invoke("subtitle_vtt", { url })) as string;
}

export async function openInVlc(url: string, vtt?: string): Promise<void> {
	await invoke("open_in_vlc", { url, vtt: vtt ?? null });
}

export async function openMagnet(magnet: string): Promise<void> {
	await invoke("open_magnet", { magnet });
}

/** SRT → WebVTT for subtitles dropped onto the player. */
export function srtToVtt(srt: string): string {
	const body = srt.replace(/^\uFEFF/, "").replace(/\r\n?/g, "\n");
	if (body.trimStart().startsWith("WEBVTT")) return body;
	const lines = body.split("\n").map(l => (l.includes("-->") ? l.replace(/,/g, ".") : l));
	return `WEBVTT\n\n${lines.join("\n")}`;
}

export interface Cue {
	start: number;
	end: number;
	text: string;
}

function vttTime(s: string): number {
	const m = s.trim().match(/(?:(\d+):)?(\d+):(\d+)[.,](\d+)/);
	if (!m) return Number.NaN;
	return Number(m[1] ?? 0) * 3600 + Number(m[2]) * 60 + Number(m[3]) + Number(m[4]) / 1000;
}

/**
 * Parse WebVTT into cues. The player adds them through the TextTrack API:
 * WKWebView never loads <track src> for these streams, addCue always works.
 */
export function parseVtt(vtt: string): Cue[] {
	const cues: Cue[] = [];
	for (const block of vtt.replace(/\r\n?/g, "\n").split(/\n\n+/)) {
		const lines = block.split("\n");
		const i = lines.findIndex(l => l.includes("-->"));
		if (i < 0) continue;
		const [a = "", b = ""] = (lines[i] as string).split("-->");
		const start = vttTime(a);
		const end = vttTime(b.trim().split(/\s+/)[0] ?? "");
		const text = lines
			.slice(i + 1)
			.join("\n")
			.trim();
		if (Number.isFinite(start) && Number.isFinite(end) && text) cues.push({ start, end, text });
	}
	return cues;
}

export interface TranslationProgress {
	done: number;
	total: number;
	/** film position (s) covered from the start point without a gap */
	ready_until: number;
	vtt: string;
	finished: boolean;
	error: string | null;
}

/** Translate an English subtitle to Portuguese on the Mac; returns a job id. */
export async function subtitleTranslateStart(url: string, startAt?: number): Promise<number> {
	return (await invoke("subtitle_translate_start", { url, startAt: startAt ?? null })) as number;
}

export async function subtitleTranslateProgress(
	jobId: number
): Promise<TranslationProgress | null> {
	return (await invoke("subtitle_translate_progress", { jobId })) as TranslationProgress | null;
}

export async function subtitleTranslateCancel(): Promise<void> {
	await invoke("subtitle_translate_cancel");
}
