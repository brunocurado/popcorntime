import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { PreferencesLoaderMount } from "@/loaders/preferences";
import { resetGlobalStore, useGlobalStore } from "@/stores/global";

afterEach(() => {
	clearMocks();
	resetGlobalStore();
});

describe("PreferencesLoaderMount", () => {
	it("fixes the region to Brazil without calling the account API", async () => {
		const calls: string[] = [];
		mockIPC(cmd => {
			calls.push(cmd);
		});
		const r = render(<PreferencesLoaderMount />);
		await act(async () => {});
		expect(useGlobalStore.getState().preferences.status).toBe("ready");
		expect(useGlobalStore.getState().preferences.country).toBe("BR");
		expect(calls).not.toContain("user_preferences");
		r.unmount();
	});
});
