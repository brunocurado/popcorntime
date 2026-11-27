import { useEffect, useRef } from "react";
import { useNavigate } from "react-router";
import { SplashScreen } from "@/components/splash-screen";
import { useGlobalStore } from "@/stores/global";

/**
 * Straight to Filmes once the app has booted. No onboarding, login or
 * region questions: they depended on the popcorntime.app account API, which
 * rejects guests ("Unauthorized"), so answers were never saved and the
 * questions came back on every launch. Region is fixed to Brazil.
 */
export function SplashRoute() {
	const appBoot = useGlobalStore(s => s.app.boot);
	const redirected = useRef(false);
	const navigate = useNavigate();

	useEffect(() => {
		if (appBoot === "cold" || redirected.current) return;
		redirected.current = true;
		navigate("/movies", { flushSync: true });
	}, [appBoot, navigate]);

	return <SplashScreen />;
}
