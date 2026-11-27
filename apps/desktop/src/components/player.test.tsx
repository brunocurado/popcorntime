import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Player, type PlayRequest } from "@/components/player";
import { getProgress, resetLibrary } from "@/lib/library";
import { parseVtt, srtToVtt } from "@/tauri/yts";

const REQ: PlayRequest = {
	key: "movie:tt0133093",
	kind: "movie",
	magnet: "magnet:?xt=urn:btih:abc",
	title: "The Matrix",
	imdbCode: "tt0133093",
};
const URL_MP4 = "http://127.0.0.1:5000/torrents/0/stream/3/The.Matrix.mp4";

let calls: string[] = [];

function mockStream(inApp: boolean) {
	mockIPC((cmd, args) => {
		calls.push(cmd);
		if (cmd === "stream_play") {
			return {
				torrent_id: 0,
				file_index: 3,
				file_name: inApp ? "The.Matrix.mp4" : "The.Matrix.mkv",
				file_size: 1e9,
				url: URL_MP4,
				play_url: inApp ? URL_MP4 : null,
				in_app: inApp,
				remuxed: false,
				time_offset: 0,
				duration: 0,
			};
		}
		if (cmd === "stream_status") return null;
		if (cmd === "subtitles_list") {
			return [
				{ id: "1", lang: "por", label: "Português (PT)", url: "https://subs/1" },
				{ id: "2", lang: "eng", label: "English", url: "https://subs/2" },
			];
		}
		if (cmd === "subtitle_vtt") return "WEBVTT\n\n1\n00:00:01.000 --> 00:00:02.000\nOlá\n";
		if (cmd === "open_in_vlc") {
			expect((args as { vtt: string | null }).vtt).toContain("Olá");
			return null;
		}
		return null;
	});
}

beforeEach(() => {
	calls = [];
	resetLibrary();
});

afterEach(() => {
	clearMocks();
});

describe("Player", () => {
	it("streams mp4 in-app with the Portuguese subtitle preselected", async () => {
		mockStream(true);
		const onClose = vi.fn();
		const r = render(<Player request={REQ} onClose={onClose} />);
		await act(async () => {});
		await act(async () => {});

		const video = document.querySelector("video");
		expect(video?.getAttribute("src")).toBe(URL_MP4);
		const picker = (await screen.findByLabelText("Subtitles")) as HTMLSelectElement;
		expect(picker.value).toBe("1");
		await act(async () => {});
		expect(screen.getByText("+0.5s")).toBeInTheDocument();

		fireEvent.click(screen.getByText("✕ Close"));
		expect(onClose).toHaveBeenCalled();
		r.unmount();
		await act(async () => {});
		// closing drops the torrent and its cache
		expect(calls).toContain("stream_stop");
	});

	it("offers VLC (with the subtitle) for formats the WebView can't play", async () => {
		mockStream(false);
		const r = render(<Player request={REQ} onClose={() => {}} />);
		await act(async () => {});
		await act(async () => {});
		await act(async () => {});

		expect(document.querySelector("video")).toBeNull();
		expect(screen.getByText(/not a format the built-in player supports/)).toBeInTheDocument();
		const buttons = screen.getAllByRole("button", { name: "Open in VLC" });
		await act(async () => {
			fireEvent.click(buttons[buttons.length - 1] as HTMLElement);
		});
		expect(calls).toContain("open_in_vlc");
		r.unmount();
	});
});

describe("Player resume, progress and next episode", () => {
	function mockRemux(playArgs: unknown[]) {
		mockIPC((cmd, args) => {
			calls.push(cmd);
			if (cmd === "stream_play") {
				playArgs.push(args);
				return {
					torrent_id: 4,
					file_index: 1,
					file_name: "Show.S01E02.mkv",
					file_size: 1e9,
					url: "http://127.0.0.1:5000/torrents/4/stream/1/Show.S01E02.mkv",
					play_url: "http://127.0.0.1:5001/4/index.m3u8",
					in_app: true,
					remuxed: true,
					time_offset: 754,
					duration: 3000,
				};
			}
			if (cmd === "subtitles_list") {
				expect(args).toMatchObject({ imdbCode: "tt1", season: 1, episode: 2 });
				return [];
			}
			return null;
		});
	}

	const EP: PlayRequest = {
		key: "ep:tt1:1:2",
		kind: "episode",
		magnet: "magnet:?xt=urn:btih:def",
		title: "Show · S01E02",
		imdbCode: "tt1",
		showTitle: "Show",
		season: 1,
		episode: 2,
		hint: { file_name: "Show.S01E02.mkv", season: 1, episode: 2 },
		startAt: 754,
	};

	it("resumes a remuxed episode at the saved point, saves progress and stops only its torrent", async () => {
		const playArgs: unknown[] = [];
		mockRemux(playArgs);
		const onClose = vi.fn();
		const r = render(<Player request={EP} onClose={onClose} />);
		await act(async () => {});
		await act(async () => {});
		expect(playArgs[0]).toMatchObject({
			magnet: EP.magnet,
			hint: { file_name: "Show.S01E02.mkv", season: 1, episode: 2 },
			startAt: 754,
		});
		expect(document.querySelector("video")?.getAttribute("src")).toBe(
			"http://127.0.0.1:5001/4/index.m3u8"
		);
		expect(screen.getByText("Retomado em 12:34")).toBeInTheDocument();
		expect(screen.getByText("mkv → conversão ao vivo")).toBeInTheDocument();

		// 20 s into the remux = 12:34 + 0:20 of the film
		const video = document.querySelector("video") as HTMLVideoElement;
		Object.defineProperty(video, "currentTime", { value: 20, configurable: true });
		fireEvent.click(screen.getByText("✕ Close"));
		expect(getProgress("ep:tt1:1:2")).toMatchObject({
			position: 774,
			duration: 3000,
			season: 1,
			episode: 2,
		});
		r.unmount();
		await act(async () => {});
		expect(calls).toContain("stream_stop");
	});

	it("counts down to the next episode when one ends", async () => {
		mockRemux([]);
		const start = vi.fn();
		const r = render(
			<Player
				request={{ ...EP, startAt: 0 }}
				onClose={() => {}}
				next={{ label: "S01E03 · Next", start }}
			/>
		);
		await act(async () => {});
		await act(async () => {});
		vi.useFakeTimers();
		await act(async () => {
			fireEvent(document.querySelector("video") as HTMLVideoElement, new Event("ended"));
		});
		expect(screen.getByText("S01E03 · Next")).toBeInTheDocument();
		expect(screen.getByText("A seguir, em 10s")).toBeInTheDocument();
		expect(getProgress("ep:tt1:1:2")?.position).toBe(3000); // marked as watched
		for (let i = 0; i < 11; i++) {
			await act(async () => {
				vi.advanceTimersByTime(1000);
			});
		}
		vi.useRealTimers();
		expect(start).toHaveBeenCalledTimes(1);
		r.unmount();
	});
});

describe("Player waits for a translated subtitle while the film buffers", () => {
	function mockTranslation(progress: () => unknown, startArgs: unknown[]) {
		mockIPC((cmd, args) => {
			calls.push(cmd);
			if (cmd === "stream_play") {
				return {
					torrent_id: 2,
					file_index: 0,
					file_name: "Insidious.2026.mp4",
					file_size: 1e9,
					url: URL_MP4,
					play_url: URL_MP4,
					in_app: true,
					remuxed: false,
					time_offset: 0,
					duration: 5400,
				};
			}
			if (cmd === "subtitles_list") {
				// no Portuguese anywhere, English available
				return [
					{ id: "e1", lang: "eng", label: "English", url: "https://subs/e1" },
					{ id: "s1", lang: "spa", label: "Español", url: "https://subs/s1" },
				];
			}
			if (cmd === "subtitle_translate_start") {
				startArgs.push(args);
				return 7;
			}
			if (cmd === "subtitle_translate_progress") return progress();
			if (cmd === "subtitle_vtt") return "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nHello\n";
			return null;
		});
	}

	it("offers PT translated from English, holds the video until the first batch, then plays", async () => {
		const startArgs: unknown[] = [];
		let done = 10;
		mockTranslation(
			() => ({
				done,
				total: 900,
				ready_until: 40,
				vtt: "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nOlá\n",
				finished: false,
				error: null,
			}),
			startArgs
		);
		vi.useFakeTimers({ shouldAdvanceTime: true });
		const r = render(<Player request={{ ...REQ, startAt: 0 }} onClose={() => {}} />);
		await act(async () => {});
		await act(async () => {});
		const picker = (await screen.findByLabelText("Subtitles")) as HTMLSelectElement;
		expect(picker.value).toBe("tr-por");
		expect(
			screen.getByRole("option", { name: "Português (traduzido do inglês)" })
		).toBeInTheDocument();
		expect(startArgs[0]).toEqual({ url: "https://subs/e1", startAt: 0 });

		// first batch not in yet: video kept hidden, waiting screen shown
		expect(screen.getByText("A preparar o filme com legenda em português…")).toBeInTheDocument();
		expect(screen.getByText(/A traduzir a legenda: 10\/25 falas/)).toBeInTheDocument();
		expect((document.querySelector("video") as HTMLVideoElement).style.visibility).toBe("hidden");

		done = 25;
		await act(async () => {
			vi.advanceTimersByTime(2100);
		});
		await act(async () => {});
		expect(screen.queryByText("A preparar o filme com legenda em português…")).toBeNull();
		expect((document.querySelector("video") as HTMLVideoElement).style.visibility).toBe("");
		expect(screen.getByText("tradução 25/900")).toBeInTheDocument();
		vi.useRealTimers();
		r.unmount();
		await act(async () => {});
		expect(calls).toContain("subtitle_translate_cancel");
	});

	it("falls back to English when the translator is unavailable", async () => {
		mockTranslation(
			() => ({
				done: 0,
				total: 900,
				ready_until: 0,
				vtt: "WEBVTT\n\n",
				finished: true,
				error: "falta o pacote de tradução Inglês → Português",
			}),
			[]
		);
		const r = render(<Player request={REQ} onClose={() => {}} />);
		await act(async () => {});
		await act(async () => {});
		await act(async () => {});
		expect((screen.getByLabelText("Subtitles") as HTMLSelectElement).value).toBe("e1");
		expect(screen.getByText(/Tradução indisponível — a usar inglês/)).toBeInTheDocument();
		r.unmount();
	});

	it("'Começar já' plays without waiting", async () => {
		mockTranslation(
			() => ({
				done: 0,
				total: 900,
				ready_until: 0,
				vtt: "WEBVTT\n\n",
				finished: false,
				error: null,
			}),
			[]
		);
		const r = render(<Player request={REQ} onClose={() => {}} />);
		await act(async () => {});
		await act(async () => {});
		fireEvent.click(screen.getByRole("button", { name: "Começar já" }));
		expect((document.querySelector("video") as HTMLVideoElement).style.visibility).toBe("");
		r.unmount();
	});
});

describe("Player fullscreen", () => {
	it("toggles the native window fullscreen with the button and F, and leaves it on close", async () => {
		const fs: boolean[] = [];
		mockIPC((cmd, args) => {
			if (cmd === "plugin:window|set_fullscreen") fs.push((args as { value: boolean }).value);
			if (cmd === "stream_play") {
				return {
					torrent_id: 0,
					file_index: 0,
					file_name: "a.mp4",
					file_size: 1,
					url: URL_MP4,
					in_app: true,
				};
			}
			if (cmd === "subtitles_list") return [];
			return null;
		});
		mockWindows("main");
		const onClose = vi.fn();
		const r = render(<Player request={REQ} onClose={onClose} />);
		await act(async () => {});
		await act(async () => {
			fireEvent.click(screen.getByRole("button", { name: /Fullscreen/ }));
		});
		expect(screen.getByRole("button", { name: /Exit fullscreen/ })).toBeInTheDocument();
		await act(async () => {
			fireEvent.keyDown(window, { key: "f" });
		});
		expect(screen.getByRole("button", { name: /⤢ Fullscreen/ })).toBeInTheDocument();
		await act(async () => {
			fireEvent.keyDown(window, { key: "f" });
		});
		// Esc in fullscreen only leaves fullscreen
		await act(async () => {
			fireEvent.keyDown(window, { key: "Escape" });
		});
		expect(onClose).not.toHaveBeenCalled();
		await act(async () => {
			fireEvent.keyDown(window, { key: "Escape" });
		});
		expect(onClose).toHaveBeenCalled();
		expect(fs).toEqual([true, false, true, false, false]);
		r.unmount();
	});
});

describe("srtToVtt", () => {
	it("converts timestamps and keeps commas in text", () => {
		const vtt = srtToVtt("\uFEFF1\r\n00:00:01,500 --> 00:00:02,000\r\nSim, claro\r\n");
		expect(vtt).toBe("WEBVTT\n\n1\n00:00:01.500 --> 00:00:02.000\nSim, claro\n");
	});
});

describe("parseVtt", () => {
	it("parses cues with hours, settings and multi-line text", () => {
		const cues = parseVtt(
			"WEBVTT\n\n1\n00:00:47.812 --> 00:00:51.088 align:start\n- Está tudo preparado?\n- Sim\n\n2\n01:02:03.500 --> 01:02:04.000\nFim\n"
		);
		expect(cues).toEqual([
			{ start: 47.812, end: 51.088, text: "- Está tudo preparado?\n- Sim" },
			{ start: 3723.5, end: 3724, text: "Fim" },
		]);
	});
});
