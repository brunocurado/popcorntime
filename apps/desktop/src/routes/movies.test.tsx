import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
		expect(screen.getByText(/Torrentio · 1337x/)).toBeInTheDocument();
		expect(screen.queryByText("13 (2010) dup of YTS")).not.toBeInTheDocument();

		r.unmount();
	});
});
