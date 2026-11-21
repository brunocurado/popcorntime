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
