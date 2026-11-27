import { useEffect } from "react";
import { useGlobalStore } from "@/stores/global";

/**
 * Region is fixed to Brazil and nothing is fetched: the popcorntime.app
 * preferences API answers "Unauthorized" to guests, so it could never load
 * or save a choice.
 */
export const DEFAULT_COUNTRY = "BR";

export function PreferencesLoaderMount() {
	useEffect(() => {
		const { preferencesRequested, preferencesSucceeded } = useGlobalStore.getState();
		preferencesRequested();
		preferencesSucceeded({ country: DEFAULT_COUNTRY });
	}, []);

	return null;
}
