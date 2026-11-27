import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resetLibrary, saveProgress } from "@/lib/library";
import { nextEpisode, SeriesRoute } from "@/routes/series";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

const SHOW = {
	id: "tt0903747",
	name: "Breaking Bad",
	poster: "https://img/poster",
	year: "2008–2013",
	rating: "9.5",
};
const EPISODES = [
	{
		season: 1,
		episode: 1,
		title: "Pilot",
		overview: "",
		thumbnail: "",
		released: "2008-01-20T00:00:00Z",
	},
	{
		season: 1,
		episode: 2,
		title: "Cat's in the Bag...",
		overview: "",
		thumbnail: "",
		released: "2008-01-27T00:00:00Z",
	},
	{
		season: 2,
		episode: 1,
		title: "Seven Thirty-Seven",
		overview: "",
		thumbnail: "",
		released: "2009-03-08T00:00:00Z",
	},
	{ season: 0, episode: 1, title: "Special", overview: "", thumbnail: "", released: "" },
];

let calls: { cmd: string; args: unknown }[] = [];

function mockSeries() {
	mockIPC((cmd, args) => {
		calls.push({ cmd, args });
		if (cmd === "series_catalog") return [SHOW];
		if (cmd === "series_meta") {
			return {
				...SHOW,
				background: "",
				description: "A teacher turns cook.",
				runtime: "49 min",
				genres: ["Crime", "Drama"],
				trailer: "VFkjBy2b50Q",
				episodes: EPISODES,
			};
		}
		if (cmd === "episode_sources") {
			const { season, episode } = args as { season: number; episode: number };
			const tag = `S0${season}E0${episode}`;
			return [
				{
					source: "Torrentio · 1337x",
					title: "Breaking Bad S01 1080p pack",
					hash: "a".repeat(40),
					quality: "1080p",
					seeds: 80,
					size: "1.2 GB",
					file_idx: 2,
					file_name: `Breaking.Bad.${tag}.1080p.mkv`,
				},
			];
		}
		if (cmd === "yts_magnet_url") return "magnet:?xt=urn:btih:aaaa";
		if (cmd === "stream_play") {
			return {
				torrent_id: 1,
				file_index: 2,
				file_name: "Breaking.Bad.S01E02.1080p.mkv",
				file_size: 1e9,
				url: "http://127.0.0.1:1/torrents/1/stream/2/x.mkv",
				play_url: "http://127.0.0.1:2/1/index.m3u8",
				in_app: true,
				remuxed: true,
				time_offset: 0,
				duration: 2800,
			};
		}
		if (cmd === "subtitles_list") return [];
		return null;
	});
}

function renderSeries() {
	return render(
		<MemoryRouter initialEntries={["/series"]}>
			<Routes>
				<Route path="/series" element={<SeriesRoute />} />
			</Routes>
		</MemoryRouter>
	);
}

beforeEach(() => {
	calls = [];
	resetLibrary();
	vi.stubGlobal(
		"IntersectionObserver",
		class {
			observe() {}
			unobserve() {}
			disconnect() {}
		}
	);
});

afterEach(() => {
	clearMocks();
	vi.unstubAllGlobals();
});

describe("nextEpisode", () => {
	it("rolls into the next season and skips specials", () => {
		expect(nextEpisode(EPISODES, 1, 1)?.title).toBe("Cat's in the Bag...");
		expect(nextEpisode(EPISODES, 1, 2)?.title).toBe("Seven Thirty-Seven");
		expect(nextEpisode(EPISODES, 2, 1)).toBeNull();
	});
});

describe("SeriesRoute", () => {
	it("filters by genre, opens a show and auto-plays the right file of a season pack", async () => {
		mockSeries();
		const r = renderSeries();
		const poster = await screen.findByAltText("Breaking Bad");
		expect(calls.find(c => c.cmd === "series_catalog")?.args).toMatchObject({
			skip: 0,
			genre: null,
		});

		await act(async () => {
			fireEvent.click(screen.getByRole("button", { name: "Terror" }));
		});
		expect(calls.filter(c => c.cmd === "series_catalog").at(-1)?.args).toMatchObject({
			genre: "Horror",
		});

		await act(async () => {
			fireEvent.click(poster);
		});
		expect(await screen.findByText("A teacher turns cook.")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Temporada 1" })).toHaveAttribute(
			"aria-pressed",
			"true"
		);
		expect(screen.getByRole("button", { name: "Especiais" })).toBeInTheDocument();
		expect(screen.getByText("S01E02 · Cat's in the Bag...")).toBeInTheDocument();

		const plays = screen.getAllByRole("button", { name: "▶ Play" });
		await act(async () => {
			fireEvent.click(plays[1] as HTMLElement); // S01E02
		});
		await act(async () => {});
		const play = calls.find(c => c.cmd === "stream_play");
		expect(play?.args).toMatchObject({
			hint: { file_idx: 2, file_name: "Breaking.Bad.S01E02.1080p.mkv", season: 1, episode: 2 },
		});
		expect(
			screen.getByRole("dialog", { name: /Playing Breaking Bad · S01E02/ })
		).toBeInTheDocument();
		// next episode is offered
		expect(screen.getByRole("button", { name: "⏭ Próximo" })).toBeInTheDocument();
		r.unmount();
	});

	it("shows Continuar a ver for an episode in progress", async () => {
		saveProgress({
			key: "ep:tt0903747:1:2",
			kind: "episode",
			imdb: "tt0903747",
			title: "Breaking Bad · S01E02",
			showTitle: "Breaking Bad",
			season: 1,
			episode: 2,
			magnet: "magnet:?xt=urn:btih:aaaa",
			position: 754,
			duration: 2800,
		});
		mockSeries();
		const r = renderSeries();
		expect(await screen.findByText("Continuar a ver")).toBeInTheDocument();
		expect(screen.getByText("S01E02 · ▶ 12:34")).toBeInTheDocument();
		r.unmount();
	});
});
