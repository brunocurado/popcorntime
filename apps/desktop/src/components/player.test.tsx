import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Player } from "@/components/player";
import { srtToVtt } from "@/tauri/yts";

const REQ = { magnet: "magnet:?xt=urn:btih:abc", title: "The Matrix", imdbCode: "tt0133093" };
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
				in_app: inApp,
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
	URL.createObjectURL = vi.fn(() => "blob:sub");
	URL.revokeObjectURL = vi.fn();
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
		expect(document.querySelector("track")?.getAttribute("src")).toBe("blob:sub");
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

describe("srtToVtt", () => {
	it("converts timestamps and keeps commas in text", () => {
		const vtt = srtToVtt("\uFEFF1\r\n00:00:01,500 --> 00:00:02,000\r\nSim, claro\r\n");
		expect(vtt).toBe("WEBVTT\n\n1\n00:00:01.500 --> 00:00:02.000\nSim, claro\n");
	});
});
