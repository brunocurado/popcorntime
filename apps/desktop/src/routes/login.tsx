import { Button, buttonVariants } from "@popcorntime/ui/components/button";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import logo from "@/assets/logo.png";
import { useTauri } from "@/hooks/useTauri";
import { useGlobalStore } from "@/stores/global";
import { enableGuestSession } from "@/tauri/yts";

export function LoginRoute() {
	const { api, on } = useTauri();
	const appBoot = useGlobalStore(state => state.app.boot);
	const navigate = useNavigate();
	const [guestError, setGuestError] = useState<string | null>(null);

	async function initialize_session_authorization() {
		api.initializeSessionAuthorization();
	}

	async function continue_as_guest() {
		setGuestError(null);
		try {
			await enableGuestSession();
			await api.validate();
			useGlobalStore.getState().sessionSucceeded(true);
			navigate("/movies", { replace: true });
		} catch (e) {
			setGuestError(e instanceof Error ? e.message : "Could not enter guest mode");
		}
	}

	useEffect(() => {
		let unlisten: (() => void) | undefined;

		on.sessionServerReady
			.listen(event => {
				openUrl(event.payload.authorization_url);
			})
			.then(fn => {
				unlisten = fn;
			});

		return unlisten;
	}, [on.sessionServerReady]);

	useEffect(() => {
		if (appBoot !== "booted") return;
		navigate("/", { replace: true });
	}, [appBoot, navigate]);

	return (
		<main className="flex h-full">
			<div className="m-auto flex w-xs flex-col items-center justify-center gap-4">
				<img src={logo} alt="Popcorn Time" className="size-12 xl:size-14 dark:opacity-80" />

				<form
					className="flex flex-col gap-3"
					onSubmit={e => {
						e.preventDefault();
						initialize_session_authorization();
					}}
				>
					<Button size="xl" type="submit">
						Login
					</Button>
					<Button size="xl" variant="secondary" type="button" onClick={continue_as_guest}>
						Continue as guest
					</Button>
					{guestError && <p className="text-center text-sm text-red-500">{guestError}</p>}
					<Link
						className={buttonVariants({ variant: "link", size: "xl" })}
						to="https://watch.popcorntime.app/signup"
						target="_blank"
					>
						No account? Signup
					</Link>
				</form>
			</div>
		</main>
	);
}
