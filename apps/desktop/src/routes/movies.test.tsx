import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resetLibrary } from "@/lib/library";
import { MoviesRoute } from "@/routes/movies";

vi.mock("@tauri-apps/plugin-opener", () => ({
	openUrl: vi.fn(),
}));

const MOVIES = [
	{
		id: 10,
		title: "13",
		title_long: "13 (2010)",
		imdb_code: "tt0798817",
		year: 2010,
		rating: 6,
		runtime: 91,
		genres: ["Action"],
		synopsis: "Roulette.",
		mpa_rating: "R",
		small_cover_image: "https://img.yts.gg/small.jpg",
		medium_cover_image: "https://img.yts.gg/medium.jpg",
		large_cover_image: "",
		background_image: "",
		torrents: [],
	},
];

function renderMovies() {
	return render(
		<MemoryRouter initialEntries={["/movies"]}>
			<Routes>
				<Route path="/movies" element={<MoviesRoute />} />
			</Routes>
		</MemoryRouter>
	);
}

afterEach(() => {
	resetLibrary();
	clearMocks();
	vi.unstubAllGlobals();
});

beforeEach(() => {
	// jsdom has no IntersectionObserver (used by the infinite-scroll sentinel).
	vi.stubGlobal(
		"IntersectionObserver",
		class {
			observe() {}
			unobserve() {}
			disconnect() {}
		}
	);
});

describe("MoviesRoute", () => {
	it("renders posters from the YTS api", async () => {
		mockIPC(cmd => {
			if (cmd === "yts_list_movies") {
				return {
					status: "ok",
					status_message: "Query was successful",
					data: { movie_count: 1, limit: 20, page_number: 1, movies: MOVIES },
				};
			}
		});

		const r = renderMovies();
		const poster = await screen.findByAltText("13");
		expect(poster).toBeInTheDocument();
		expect(poster.getAttribute("src")).toBe("https://img.yts.gg/medium.jpg");
		expect(await screen.findByText("1 / 1 movies")).toBeInTheDocument();

		r.unmount();
	});

	it("falls back when a cover fails to load", async () => {
		mockIPC(cmd => {
			if (cmd === "yts_list_movies") {
				return {
					status: "ok",
					status_message: "Query was successful",
					data: { movie_count: 1, limit: 20, page_number: 1, movies: MOVIES },
				};
			}
		});

		const r = renderMovies();
		const poster = (await screen.findByAltText("13")) as HTMLImageElement;
		await act(async () => {
			fireEvent.error(poster);
		});
		expect(poster.getAttribute("src")).toBe("https://img.yts.gg/small.jpg");

		r.unmount();
	});

	it("toggles a dialog on cover click", async () => {
		mockIPC(cmd => {
			if (cmd === "yts_list_movies") {
				return {
					status: "ok",
					status_message: "Query was successful",
					data: { movie_count: 1, limit: 20, page_number: 1, movies: MOVIES },
				};
			}
			if (cmd === "yts_movie_details") {
				return {
					status: "ok",
					status_message: "Query was successful",
					data: {
						movie: {
							...MOVIES[0],
							// sparse details: no imdb_code — merge must keep the list value
							imdb_code: "",
							rating: 6,
							like_count: 79,
							synopsis: "",
							description_full: "Russian roulette fallback.",
							cast: [{ name: "Jason Statham", character_name: "Jasper", url_small_image: "" }],
							torrents: [
								{
									hash: "F27D21006A58C4073663B9363E7B2F2FBE8452F3",
									quality: "720p",
									type: "web",
									size: "700 MB",
									seeds: 5,
									peers: 2,
								},
							],
						},
					},
				};
			}
		});

		const r = renderMovies();
		const poster = await screen.findByAltText("13");

		// grid position is untouched: no inline details panel
		expect(screen.queryByText("Jason Statham")).not.toBeInTheDocument();

		await act(async () => {
			fireEvent.click(poster);
		});
		// merge semantics: empty details synopsis keeps the list one,
		// empty details imdb_code keeps the list one, cast is new from details
		expect(await screen.findByText("Roulette.")).toBeInTheDocument();
		expect(screen.getByText("IMDb: tt0798817")).toBeInTheDocument();
		expect(screen.getByText("Jason Statham")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Copy magnet" })).toBeInTheDocument();

		// clicking the same cover closes the dialog again
		await act(async () => {
			fireEvent.click(poster);
		});
		expect(screen.queryByText("Jason Statham")).not.toBeInTheDocument();

		r.unmount();
	});

	it("lists extra sources and hides torrents YTS already has", async () => {
		const ytsHash = "F27D21006A58C4073663B9363E7B2F2FBE8452F3";
		mockIPC(cmd => {
			if (cmd === "yts_list_movies") {
				return {
					status: "ok",
					status_message: "ok",
					data: {
						movie_count: 1,
						limit: 20,
						page_number: 1,
						movies: [
							{
								...MOVIES[0],
								torrents: [
									{
										hash: ytsHash,
										quality: "720p",
										type: "web",
										size: "700 MB",
										seeds: 5,
										peers: 2,
									},
								],
							},
						],
					},
				};
			}
			if (cmd === "yts_movie_details") return { status: "error", status_message: "x", data: {} };
			if (cmd === "extra_sources") {
				return [
					{
						source: "Torrentio · 1337x",
						title: "13.2010.1080p.BluRay",
						hash: "a".repeat(40),
						quality: "1080p",
						seeds: 40,
						size: "1.4 GB",
					},
					{
						source: "The Pirate Bay",
						title: "13 (2010) dup of YTS",
						hash: ytsHash.toLowerCase(),
						quality: "720p",
						seeds: 9,
						size: "700 MB",
					},
				];
			}
		});

		const r = renderMovies();
		const poster = await screen.findByAltText("13");
		await act(async () => {
			fireEvent.click(poster);
		});
		expect(await screen.findByText("13.2010.1080p.BluRay")).toBeInTheDocument();
		expect(screen.getAllByText(/Torrentio · 1337x/).length).toBeGreaterThan(0);
		expect(screen.queryByText("13 (2010) dup of YTS")).not.toBeInTheDocument();

		r.unmount();
	});

	it("re-queries as soon as the sort changes, with a sort YTS honours", async () => {
		const seen: { sortBy?: string; orderBy?: string }[] = [];
		mockIPC((cmd, args) => {
			if (cmd === "yts_list_movies") {
				seen.push((args as { params: { sortBy?: string; orderBy?: string } }).params);
				return {
					status: "ok",
					status_message: "ok",
					data: { movie_count: 1, limit: 20, page_number: 1, movies: MOVIES },
				};
			}
		});
		const r = renderMovies();
		await screen.findByAltText("13");
		expect(seen[0]?.sortBy).toBe("download_count");
		const options = Array.from((screen.getByLabelText("Sort by") as HTMLSelectElement).options).map(
			o => o.value
		);
		expect(options).not.toContain("seeds");
		await act(async () => {
			fireEvent.change(screen.getByLabelText("Sort by"), { target: { value: "title" } });
		});
		expect(seen.at(-1)).toMatchObject({ sortBy: "title", orderBy: "asc" });
		r.unmount();
	});

	it("filters by genre from the genre bar", async () => {
		const seen: { genre?: string; sortBy?: string }[] = [];
		mockIPC((cmd, args) => {
			if (cmd === "yts_list_movies") {
				seen.push((args as { params: { genre?: string; sortBy?: string } }).params);
				return {
					status: "ok",
					status_message: "ok",
					data: { movie_count: 1, limit: 20, page_number: 1, movies: MOVIES },
				};
			}
		});
		const r = renderMovies();
		await screen.findByAltText("13");
		expect(seen[0]?.genre).toBeUndefined();
		await act(async () => {
			fireEvent.click(screen.getByRole("button", { name: "Terror" }));
		});
		expect(seen.at(-1)).toMatchObject({ genre: "horror", sortBy: "download_count" });
		expect(screen.getByRole("button", { name: "Terror" })).toHaveAttribute("aria-pressed", "true");
		await act(async () => {
			fireEvent.change(screen.getByLabelText("Sort by"), { target: { value: "rating" } });
		});
		// changing the sort keeps the genre
		expect(seen.at(-1)).toMatchObject({ genre: "horror", sortBy: "rating" });
		r.unmount();
	});

	it("Play picks the best source, and Ver depois adds the movie to the row", async () => {
		const playArgs: unknown[] = [];
		mockIPC((cmd, args) => {
			if (cmd === "yts_list_movies") {
				return {
					status: "ok",
					status_message: "ok",
					data: {
						movie_count: 1,
						limit: 20,
						page_number: 1,
						movies: [
							{
								...MOVIES[0],
								yt_trailer_code: "abcdefghijk",
								torrents: [
									{
										hash: "1".repeat(40),
										quality: "720p",
										type: "web",
										size: "700 MB",
										seeds: 40,
										peers: 2,
									},
									{
										hash: "2".repeat(40),
										quality: "1080p",
										type: "bluray",
										size: "1.4 GB",
										seeds: 90,
										peers: 9,
									},
								],
							},
						],
					},
				};
			}
			if (cmd === "yts_movie_details") return { status: "error", status_message: "x", data: {} };
			if (cmd === "extra_sources") {
				expect(args).toMatchObject({ imdbCode: "tt0798817", title: "13", year: 2010 });
				return [
					{
						source: "Torrentio · RARBG",
						title: "13.2010.HDCAM",
						hash: "3".repeat(40),
						quality: "",
						seeds: 999,
						size: "1 GB",
					},
				];
			}
			if (cmd === "yts_magnet_url") return `magnet:?xt=urn:btih:${(args as { hash: string }).hash}`;
			if (cmd === "stream_play") {
				playArgs.push(args);
				return {
					torrent_id: 0,
					file_index: 0,
					file_name: "13.mp4",
					file_size: 1,
					url: "http://127.0.0.1:1/x.mp4",
					play_url: "http://127.0.0.1:1/x.mp4",
					in_app: true,
					remuxed: false,
					time_offset: 0,
					duration: 0,
				};
			}
			if (cmd === "subtitles_list") return [];
			return null;
		});
		const r = renderMovies();
		const poster = await screen.findByAltText("13");
		await act(async () => {
			fireEvent.click(poster);
		});
		await act(async () => {});
		expect(screen.getByText(/melhor fonte: 1080p · YTS · 90 seeds/)).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "🎬 Trailer" })).toBeInTheDocument();

		await act(async () => {
			fireEvent.click(screen.getByRole("button", { name: "+ Ver depois" }));
		});
		expect(screen.getByRole("button", { name: "✓ Na lista" })).toBeInTheDocument();
		// the row sits behind the modal (aria-hidden while it is open)
		expect(screen.getByRole("region", { name: "Ver depois", hidden: true })).toBeInTheDocument();

		await act(async () => {
			// the first ▶ Play is the auto-pick at the top of the window
			fireEvent.click(screen.getAllByRole("button", { name: "▶ Play" })[0] as HTMLElement);
		});
		await act(async () => {});
		expect(playArgs[0]).toMatchObject({ magnet: `magnet:?xt=urn:btih:${"2".repeat(40)}` });
		r.unmount();
	});
});
