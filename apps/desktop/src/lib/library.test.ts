import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
	continueWatching,
	formatClock,
	getProgress,
	inWatchlist,
	isWatched,
	lastEpisode,
	type ProgressEntry,
	resetLibrary,
	resumePosition,
	saveProgress,
	toggleWatchlist,
	useLibrary,
} from "@/lib/library";

const base = (over: Partial<ProgressEntry>): Omit<ProgressEntry, "updatedAt"> => ({
	key: "movie:tt1",
	kind: "movie",
	imdb: "tt1",
	title: "Film",
	magnet: "magnet:?xt=urn:btih:a",
	position: 600,
	duration: 6000,
	...over,
});

afterEach(() => {
	resetLibrary();
	vi.useRealTimers();
});

describe("library", () => {
	it("resumes only between one minute and 92 %", () => {
		saveProgress(base({ position: 30 }));
		expect(resumePosition(getProgress("movie:tt1"))).toBe(0);
		saveProgress(base({ position: 600 }));
		expect(resumePosition(getProgress("movie:tt1"))).toBe(600);
		saveProgress(base({ position: 5600 }));
		expect(isWatched(getProgress("movie:tt1"))).toBe(true);
		expect(resumePosition(getProgress("movie:tt1"))).toBe(0);
	});

	it("lists one entry per show, newest first, and remembers the last episode", () => {
		vi.useFakeTimers();
		vi.setSystemTime(1000);
		saveProgress(base({ key: "ep:tt9:1:1", kind: "episode", imdb: "tt9", season: 1, episode: 1 }));
		vi.setSystemTime(2000);
		saveProgress(base({ key: "ep:tt9:1:2", kind: "episode", imdb: "tt9", season: 1, episode: 2 }));
		vi.setSystemTime(3000);
		saveProgress(base({ key: "movie:tt5", imdb: "tt5" }));
		const { result } = renderHook(() => useLibrary());
		expect(continueWatching(result.current, "episode").map(p => p.key)).toEqual(["ep:tt9:1:2"]);
		expect(continueWatching(result.current).map(p => p.key)).toEqual(["movie:tt5", "ep:tt9:1:2"]);
		expect(lastEpisode("tt9")?.episode).toBe(2);
	});

	it("toggles the watchlist and notifies subscribers", () => {
		const { result } = renderHook(() => useLibrary());
		act(() => toggleWatchlist({ kind: "series", imdb: "tt0903747", title: "Breaking Bad" }));
		expect(inWatchlist(result.current, "tt0903747")).toBe(true);
		act(() => toggleWatchlist({ kind: "series", imdb: "tt0903747", title: "Breaking Bad" }));
		expect(inWatchlist(result.current, "tt0903747")).toBe(false);
	});

	it("formats clock times", () => {
		expect(formatClock(754)).toBe("12:34");
		expect(formatClock(3725)).toBe("1:02:05");
	});
});
