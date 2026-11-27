import { Button } from "@popcorntime/ui/components/button";
import placeholderImg from "@/assets/placeholder.svg";
import {
	continueWatching,
	formatClock,
	type ProgressEntry,
	type ProgressKind,
	toggleWatchlist,
	useLibrary,
	type WatchlistEntry,
} from "@/lib/library";
import { episodeTag } from "@/lib/pick";

function Poster({ src, alt }: { src?: string; alt: string }) {
	return (
		<img
			src={src || placeholderImg}
			alt={alt}
			loading="lazy"
			className="aspect-[2/3] w-full object-cover"
			onError={e => {
				(e.target as HTMLImageElement).src = placeholderImg;
			}}
		/>
	);
}

/** "Continuar a ver": in-progress titles with a progress bar. */
export function ContinueRow({
	kind,
	onResume,
}: {
	kind: ProgressKind;
	onResume: (p: ProgressEntry) => void;
}) {
	const lib = useLibrary();
	const items = continueWatching(lib, kind);
	if (items.length === 0) return null;
	return (
		<section className="flex flex-col gap-2" aria-label="Continuar a ver">
			<h2 className="text-lg font-semibold">Continuar a ver</h2>
			<div className="flex gap-3 overflow-x-auto pb-2">
				{items.map(p => {
					const pct = p.duration > 0 ? Math.min(100, (p.position / p.duration) * 100) : 0;
					return (
						<button
							key={p.key}
							type="button"
							onClick={() => onResume(p)}
							className="group flex w-36 shrink-0 flex-col overflow-hidden rounded-xl border text-left transition hover:ring-2 hover:ring-accent"
						>
							<div className="relative">
								<Poster src={p.poster} alt={p.showTitle ?? p.title} />
								<div className="absolute right-0 bottom-0 left-0 h-1 bg-black/50">
									<div className="h-full bg-red-500" style={{ width: `${pct}%` }} />
								</div>
							</div>
							<div className="flex flex-col gap-0.5 p-2">
								<span className="truncate text-xs font-medium">{p.showTitle ?? p.title}</span>
								<span className="text-muted-foreground text-[11px]">
									{p.kind === "episode" && p.season && p.episode
										? `${episodeTag(p.season, p.episode)} · `
										: ""}
									▶ {formatClock(p.position)}
								</span>
							</div>
						</button>
					);
				})}
			</div>
		</section>
	);
}

/** "Ver depois": saved titles, with a quick remove. */
export function WatchlistRow({
	kind,
	onOpen,
}: {
	kind: WatchlistEntry["kind"];
	onOpen: (w: WatchlistEntry) => void;
}) {
	const lib = useLibrary();
	const items = lib.watchlist.filter(w => w.kind === kind);
	if (items.length === 0) return null;
	return (
		<section className="flex flex-col gap-2" aria-label="Ver depois">
			<h2 className="text-lg font-semibold">Ver depois</h2>
			<div className="flex gap-3 overflow-x-auto pb-2">
				{items.map(w => (
					<div key={w.imdb} className="relative w-32 shrink-0">
						<button
							type="button"
							onClick={() => onOpen(w)}
							className="flex w-full flex-col overflow-hidden rounded-xl border text-left transition hover:ring-2 hover:ring-accent"
						>
							<Poster src={w.poster} alt={w.title} />
							<span className="truncate p-2 text-xs font-medium">{w.title}</span>
						</button>
						<button
							type="button"
							aria-label={`Remover ${w.title} de Ver depois`}
							className="absolute top-1 right-1 rounded-full bg-black/70 px-2 text-xs text-white"
							onClick={() => toggleWatchlist(w)}
						>
							✕
						</button>
					</div>
				))}
			</div>
		</section>
	);
}

/** "+ Ver depois" / "✓ Na lista" toggle. */
export function WatchlistButton({ item }: { item: Omit<WatchlistEntry, "addedAt"> }) {
	const lib = useLibrary();
	const saved = lib.watchlist.some(w => w.imdb === item.imdb);
	return (
		<Button
			size="sm"
			variant={saved ? "secondary" : "outline"}
			aria-pressed={saved}
			onClick={() => toggleWatchlist(item)}
		>
			{saved ? "✓ Na lista" : "+ Ver depois"}
		</Button>
	);
}
