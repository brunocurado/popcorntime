import { useSyncExternalStore } from "react";
import type { FileHint } from "@/tauri/yts";

/**
 * Local library: playback progress ("Continuar a ver") and the watchlist
 * ("Ver depois"). Lives in localStorage — nothing leaves the Mac.
 */

const STORAGE_KEY = "pt.library.v1";
const EVENT = "pt-library-change";
/** Under this many seconds it isn't worth resuming. */
export const RESUME_MIN_SECONDS = 60;
/** Past this share of the runtime a title counts as watched. */
export const WATCHED_RATIO = 0.92;

export type ProgressKind = "movie" | "episode";

export interface ProgressEntry {
	key: string;
	kind: ProgressKind;
	imdb: string;
	title: string;
	poster?: string;
	showTitle?: string;
	season?: number;
	episode?: number;
	episodeTitle?: string;
	/** what was playing, so "continue" resumes the same release */
	magnet: string;
	hint?: FileHint;
	position: number;
	duration: number;
	updatedAt: number;
}

export interface WatchlistEntry {
	kind: "movie" | "series";
	imdb: string;
	title: string;
	poster?: string;
	year?: string;
	addedAt: number;
}

interface LibraryState {
	progress: Record<string, ProgressEntry>;
	watchlist: WatchlistEntry[];
}

const EMPTY: LibraryState = { progress: {}, watchlist: [] };
let cache: LibraryState | null = null;

/** localStorage, or an in-memory stand-in where it doesn't exist (tests). */
const memory = new Map<string, string>();
function storage(): Pick<Storage, "getItem" | "setItem" | "removeItem"> {
	try {
		const ls = typeof window !== "undefined" ? window.localStorage : undefined;
		if (ls && typeof ls.getItem === "function") return ls;
	} catch {
		// access denied
	}
	return {
		getItem: k => memory.get(k) ?? null,
		setItem: (k, v) => void memory.set(k, v),
		removeItem: k => void memory.delete(k),
	};
}

function load(): LibraryState {
	if (cache) return cache;
	try {
		const raw = storage().getItem(STORAGE_KEY);
		const parsed = raw ? (JSON.parse(raw) as Partial<LibraryState>) : {};
		cache = { progress: parsed.progress ?? {}, watchlist: parsed.watchlist ?? [] };
	} catch {
		cache = { ...EMPTY };
	}
	return cache;
}

function save(next: LibraryState) {
	cache = next;
	try {
		storage().setItem(STORAGE_KEY, JSON.stringify(next));
	} catch {
		// storage full/blocked: keep the in-memory copy
	}
	window.dispatchEvent(new Event(EVENT));
}

/** Test helper. */
export function resetLibrary() {
	cache = null;
	storage().removeItem(STORAGE_KEY);
	window.dispatchEvent(new Event(EVENT));
}

export const movieKey = (imdb: string) => `movie:${imdb}`;
export const episodeKey = (imdb: string, season: number, episode: number) =>
	`ep:${imdb}:${season}:${episode}`;

export function getProgress(key: string): ProgressEntry | undefined {
	return load().progress[key];
}

export function saveProgress(entry: Omit<ProgressEntry, "updatedAt">) {
	if (!Number.isFinite(entry.position) || entry.position < 0) return;
	const state = load();
	save({
		...state,
		progress: { ...state.progress, [entry.key]: { ...entry, updatedAt: Date.now() } },
	});
}

export function isWatched(p?: ProgressEntry): boolean {
	return !!p && p.duration > 0 && p.position >= p.duration * WATCHED_RATIO;
}

/** Position worth resuming from, else 0. */
export function resumePosition(p?: ProgressEntry): number {
	if (!p || isWatched(p) || p.position < RESUME_MIN_SECONDS) return 0;
	return p.position;
}

/** In-progress titles, newest first; one entry per show for episodes. */
export function continueWatching(state: LibraryState, kind?: ProgressKind): ProgressEntry[] {
	const seenShows = new Set<string>();
	return Object.values(state.progress)
		.filter(p => (kind ? p.kind === kind : true))
		.filter(p => resumePosition(p) > 0)
		.sort((a, b) => b.updatedAt - a.updatedAt)
		.filter(p => {
			if (p.kind !== "episode") return true;
			if (seenShows.has(p.imdb)) return false;
			seenShows.add(p.imdb);
			return true;
		})
		.slice(0, 20);
}

/** Most recently watched episode of a show (to reopen on its season). */
export function lastEpisode(imdb: string): ProgressEntry | undefined {
	return Object.values(load().progress)
		.filter(p => p.kind === "episode" && p.imdb === imdb)
		.sort((a, b) => b.updatedAt - a.updatedAt)[0];
}

export function inWatchlist(state: LibraryState, imdb: string): boolean {
	return state.watchlist.some(w => w.imdb === imdb);
}

export function toggleWatchlist(item: Omit<WatchlistEntry, "addedAt">) {
	const state = load();
	const exists = state.watchlist.some(w => w.imdb === item.imdb);
	save({
		...state,
		watchlist: exists
			? state.watchlist.filter(w => w.imdb !== item.imdb)
			: [{ ...item, addedAt: Date.now() }, ...state.watchlist],
	});
}

function subscribe(cb: () => void) {
	const onStorage = (e: StorageEvent) => {
		if (e.key === STORAGE_KEY) {
			cache = null;
			cb();
		}
	};
	window.addEventListener(EVENT, cb);
	window.addEventListener("storage", onStorage);
	return () => {
		window.removeEventListener(EVENT, cb);
		window.removeEventListener("storage", onStorage);
	};
}

export function useLibrary(): LibraryState {
	return useSyncExternalStore(subscribe, load, load);
}

export function formatClock(seconds: number): string {
	const s = Math.max(0, Math.floor(seconds));
	const h = Math.floor(s / 3600);
	const m = Math.floor((s % 3600) / 60);
	const sec = s % 60;
	const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
	return `${h > 0 ? `${h}:` : ""}${mm}:${String(sec).padStart(2, "0")}`;
}
