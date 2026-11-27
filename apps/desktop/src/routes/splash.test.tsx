import { act, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { afterEach, describe, expect, it } from "vitest";
import { SplashRoute } from "@/routes/splash";
import { resetGlobalStore, useGlobalStore } from "@/stores/global";

function LocationProbe() {
	const { pathname } = useLocation();
	return <div data-testid="loc">{pathname}</div>;
}

function renderWithRouter() {
	return render(
		<MemoryRouter initialEntries={["/"]}>
			<Routes>
				<Route
					path="/"
					element={
						<div data-testid="splash">
							<SplashRoute />
						</div>
					}
				/>
				<Route path="/movies" element={<div data-testid="movies" />} />
			</Routes>
			<LocationProbe />
		</MemoryRouter>
	);
}

afterEach(() => {
	resetGlobalStore();
});

describe("SplashRoute", () => {
	it("shows the splash while the app is still cold", async () => {
		useGlobalStore.setState(s => {
			s.app.boot = "cold";
		});
		const r = renderWithRouter();
		await act(async () => {});
		expect(screen.getByTestId("splash")).toBeInTheDocument();
		expect(screen.getByTestId("loc")).toHaveTextContent("/");
		r.unmount();
	});

	it("goes straight to Filmes — no onboarding, login or region questions", async () => {
		for (const state of [
			{ onboarded: false, active: false },
			{ onboarded: true, active: false },
			{ onboarded: true, active: true },
		]) {
			useGlobalStore.setState(s => {
				s.app.boot = "booting";
				s.settings.onboardingComplete = state.onboarded;
				s.session.isActive = state.active;
				s.session.status = "ready";
			});
			const r = renderWithRouter();
			await act(async () => {});
			expect(screen.getByTestId("loc")).toHaveTextContent("/movies");
			r.unmount();
			resetGlobalStore();
		}
	});
});
