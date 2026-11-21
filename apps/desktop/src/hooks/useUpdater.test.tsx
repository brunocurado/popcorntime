import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UpdaterProvider } from "@/hooks/useUpdater";
import { resetGlobalStore, useGlobalStore } from "@/stores/global";
import { toast } from "@/test/mock";

const renderWithProvider = () =>
	render(
		<UpdaterProvider>
			<div data-testid="root" />
		</UpdaterProvider>
	);

let checks = 0;

beforeEach(() => {
	checks = 0;
	vi.stubEnv("VITE_ENABLE_UPDATER", "true");
	mockIPC((cmd, _args) => {
		if (cmd === "plugin:app|name") return "Popcorn Time";
		if (cmd === "plugin:app|version") return "1.0.0";
		if (cmd === "plugin:updater|check") {
			checks++;
			return {
				available: true,
				version: "9.9.9",
				download: async (
					_onEvt: (ev: { event: "Started" | "Progress" | "Finished" }) => void
				) => {},
				install: async () => {},
			};
		}
	});
});

afterEach(() => {
	vi.unstubAllEnvs();
	clearMocks();
	resetGlobalStore();
});

describe("UpdaterProvider with mockIPC", () => {
	it("mount check + sets available update", async () => {
		const r = renderWithProvider();
		await act(async () => {});

		const s = useGlobalStore.getState();
		expect(s.updater.availableUpdate).toBeDefined();
		expect(s.updater.availableUpdate?.version).toBe("9.9.9");

		r.unmount();
	});

	it("shows toast on update available", async () => {
		const r = renderWithProvider();
		await act(async () => {});

		const s = useGlobalStore.getState();
		expect(s.updater.availableUpdate).toBeDefined();
		expect(s.updater.availableUpdate?.version).toBe("9.9.9");
		expect(toast).toHaveBeenCalled();

		r.unmount();
	});

	it("never checks the official channel unless enabled at build time", async () => {
		vi.stubEnv("VITE_ENABLE_UPDATER", "");
		const r = renderWithProvider();
		await act(async () => {});

		expect(checks).toBe(0);
		expect(useGlobalStore.getState().updater.availableUpdate).toBeUndefined();

		r.unmount();
	});
});
