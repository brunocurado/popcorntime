import { Button, buttonVariants } from "@popcorntime/ui/components/button";
import {
	Menubar,
	MenubarContent,
	MenubarItem,
	MenubarMenu,
	MenubarTrigger,
} from "@popcorntime/ui/components/menubar";
import { cn } from "@popcorntime/ui/lib/utils";
import { CircleUserIcon, Cog, Film, Tv } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link, useLocation } from "react-router";
import { useGlobalStore } from "@/stores/global";

/**
 * Two tabs: Filmes (YTS + extra sources) and Séries (Cinemeta). The original
 * Popcorn Time tabs, region/provider preferences and sign-out lived on the
 * popcorntime.app account API, which answers "Unauthorized" to guests — so
 * they never loaded or saved and are gone.
 */
export function Header() {
	const toggleSettings = useGlobalStore(state => state.toggleSettings);
	const direction = useGlobalStore(state => state.i18n.direction);
	const { t } = useTranslation();
	const { pathname } = useLocation();
	const onMovies = pathname === "/movies";
	const onSeries = pathname === "/series";

	return (
		<header className="border-border bg-background/95 supports-[backdrop-filter]:bg-background/90 fixed top-0 z-50 h-14 w-full overscroll-none border-b backdrop-blur select-none">
			<div className="macos:ml-20 mr-4 ml-4 flex h-full items-center">
				<div className="mx-auto grid w-full grid-cols-[auto_1fr_auto] items-center gap-2">
					<div className="z-[200] flex items-center gap-1" />
					<nav className="z-[50] flex gap-4 justify-self-center rtl:space-x-reverse">
						<Link
							className={cn(
								buttonVariants({ variant: "link" }),
								onMovies && "text-accent-foreground bg-accent",
								"flex gap-2"
							)}
							to="/movies"
						>
							<Film className="h-4 w-4" />
							<span>Filmes</span>
						</Link>
						<Link
							className={cn(
								buttonVariants({ variant: "link" }),
								onSeries && "text-accent-foreground bg-accent",
								"flex gap-2"
							)}
							to="/series"
						>
							<Tv className="h-4 w-4" />
							<span>Séries</span>
						</Link>
					</nav>

					<div className="flex items-center gap-1 justify-self-end">
						<Menubar className="z-[200]">
							<MenubarMenu>
								<MenubarTrigger asChild>
									<Button
										title={t("menu.settings")}
										variant="link"
										size="icon"
										className="text-muted-foreground hover:bg-muted"
									>
										<CircleUserIcon className="size-8" />
									</Button>
								</MenubarTrigger>

								<MenubarContent align={direction === "ltr" ? "end" : "start"} className="z-[400]">
									<MenubarItem onClick={toggleSettings} className="flex gap-2">
										<Cog className="size-4 shrink-0" />
										<span>{t("menu.settings")}</span>
									</MenubarItem>
								</MenubarContent>
							</MenubarMenu>
						</Menubar>
					</div>
				</div>
			</div>
			<div className="absolute top-0 left-0 z-40 h-14 w-full" data-tauri-drag-region></div>
		</header>
	);
}
