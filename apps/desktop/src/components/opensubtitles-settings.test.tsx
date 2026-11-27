import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { OpenSubtitlesSettings } from "@/components/opensubtitles-settings";

afterEach(() => clearMocks());

describe("OpenSubtitlesSettings", () => {
	it("verifies before saving, shows the error, then the connected account", async () => {
		const saves: unknown[] = [];
		let attempt = 0;
		mockIPC((cmd, args) => {
			if (cmd === "opensubtitles_status")
				return { configured: false, username: null, remaining: null };
			if (cmd === "opensubtitles_save") {
				saves.push(args);
				attempt++;
				if (attempt === 1) throw { message: "OpenSubtitles 401: Invalid credentials" };
				return { configured: true, username: "bruno", remaining: 20 };
			}
			return null;
		});
		const r = render(<OpenSubtitlesSettings />);
		await act(async () => {});
		const save = screen.getByRole("button", { name: "Verificar e guardar" });
		expect(save).toBeDisabled();

		fireEvent.change(screen.getByLabelText("Chave de API"), { target: { value: "KEY" } });
		fireEvent.change(screen.getByLabelText("Utilizador"), { target: { value: "bruno" } });
		// username without password can't be saved
		expect(save).toBeDisabled();
		fireEvent.change(screen.getByLabelText("Password"), { target: { value: "pw" } });
		await act(async () => {
			fireEvent.click(save);
		});
		expect(screen.getByText("OpenSubtitles 401: Invalid credentials")).toBeInTheDocument();

		await act(async () => {
			fireEvent.click(screen.getByRole("button", { name: "Verificar e guardar" }));
		});
		expect(saves[1]).toEqual({ apiKey: "KEY", username: "bruno", password: "pw" });
		expect(screen.getByText(/Ligado como bruno · 20 downloads hoje/)).toBeInTheDocument();
		// secrets are not kept on screen
		expect(screen.queryByLabelText("Password")).toBeNull();
		r.unmount();
	});
});
