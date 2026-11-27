import { Badge } from "@popcorntime/ui/components/badge";
import { Button } from "@popcorntime/ui/components/button";
import { Dialog, DialogContent, DialogTitle } from "@popcorntime/ui/components/dialog";
import { Input } from "@popcorntime/ui/components/input";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import useInfiniteScroll from "react-infinite-scroll-hook";
import placeholderImg from "@/assets/placeholder.svg";
import { ContinueRow, WatchlistButton, WatchlistRow } from "@/components/library-rows";
import { type NextUp, Player, type PlayRequest } from "@/components/player";
import { Trailer } from "@/components/trailer";
import {
	episodeKey,
	formatClock,
	getProgress,
	isWatched,
	lastEpisode,
	type ProgressEntry,
	resumePosition,
	useLibrary,
} from "@/lib/library";
import { episodeTag, pickBest } from "@/lib/pick";
import { ExtraSources, toCandidates } from "@/routes/movies";
import {
	type Episode,
	type ExtraTorrent,
	episodeSources,
	type SeriesDetail,
	type SeriesSummary,
	seriesCatalog,
	seriesMeta,
	ytsMagnetUrl,
} from "@/tauri/yts";

// Cinemeta `genre` values, labelled in PT-PT.
export const SERIES_GENRES = [
	{ value: "", label: "Todas" },
	{ value: "Action", label: "Ação" },
	{ value: "Adventure", label: "Aventura" },
	{ value: "Animation", label: "Animação" },
	{ value: "Biography", label: "Biografia" },
	{ value: "Comedy", label: "Comédia" },
	{ value: "Crime", label: "Crime" },
	{ value: "Documentary", label: "Documentário" },
	{ value: "Drama", label: "Drama" },
	{ value: "Family", label: "Família" },
	{ value: "Fantasy", label: "Fantasia" },
	{ value: "Sci-Fi", label: "Ficção científica" },
	{ value: "War", label: "Guerra" },
	{ value: "History", label: "História" },
	{ value: "Mystery", label: "Mistério" },
	{ value: "Reality-TV", label: "Reality" },
	{ value: "Romance", label: "Romance" },
	{ value: "Sport", label: "Desporto" },
	{ value: "Thriller", label: "Suspense" },
	{ value: "Talk-Show", label: "Talk-show" },
	{ value: "Horror", label: "Terror" },
	{ value: "Western", label: "Western" },
] as const;

const PAGE = 50;

/** Next aired episode after (season, episode), specials skipped. */
export function nextEpisode(episodes: Episode[], season: number, episode: number): Episode | null {
	return (
		episodes.find(
			e => e.season > 0 && (e.season > season || (e.season === season && e.episode > episode))
		) ?? null
	);
}

function aired(e: Episode): boolean {
	return !e.released || new Date(e.released).getTime() <= Date.now();
}

/** Resolve the best torrent for an episode and build the play request. */
export async function requestForEpisode(
	show: Pick<SeriesDetail, "id" | "name" | "poster">,
	ep: Episode
): Promise<PlayRequest> {
	const list = await episodeSources(show.id, show.name, ep.season, ep.episode);
	const best = pickBest(toCandidates(list), { season: ep.season, episode: ep.episode });
	if (!best) {
		throw new Error(`Sem fontes com seeds para ${episodeTag(ep.season, ep.episode)}.`);
	}
	const key = episodeKey(show.id, ep.season, ep.episode);
	return {
		key,
		kind: "episode",
		magnet: await ytsMagnetUrl(best.hash, best.title),
		hint: {
			file_idx: best.fileIdx ?? null,
			file_name: best.fileName ?? null,
			season: ep.season,
			episode: ep.episode,
		},
		title: `${show.name} · ${episodeTag(ep.season, ep.episode)} · ${ep.title}`,
		imdbCode: show.id,
		poster: show.poster,
		showTitle: show.name,
		season: ep.season,
		episode: ep.episode,
		episodeTitle: ep.title,
		startAt: resumePosition(getProgress(key)),
	};
}

function EpisodeRow({
	show,
	ep,
	onPlay,
}: {
	show: SeriesDetail;
	ep: Episode;
	onPlay: (req: PlayRequest) => void;
}) {
	useLibrary(); // re-render when progress changes
	const progress = getProgress(episodeKey(show.id, ep.season, ep.episode));
	const watched = isWatched(progress);
	const resumeAt = resumePosition(progress);
	const pct = progress?.duration ? Math.min(100, (progress.position / progress.duration) * 100) : 0;
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [sources, setSources] = useState<ExtraTorrent[] | null>(null);
	const [showSources, setShowSources] = useState(false);
	const [sourcesFailed, setSourcesFailed] = useState(false);
	const isAired = aired(ep);

	const playBest = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			onPlay(await requestForEpisode(show, ep));
		} catch (e) {
			setError(e instanceof Error ? e.message : String(e));
		} finally {
			setBusy(false);
		}
	}, [show, ep, onPlay]);

	const toggleSources = useCallback(() => {
		setShowSources(v => !v);
		if (sources !== null) return;
		episodeSources(show.id, show.name, ep.season, ep.episode)
			.then(setSources)
			.catch(() => setSourcesFailed(true));
	}, [sources, show, ep]);

	const key = episodeKey(show.id, ep.season, ep.episode);
	return (
		<div className="flex flex-col gap-2 rounded-lg border p-2">
			<div className="flex gap-3">
				<div className="relative w-36 shrink-0">
					<img
						src={ep.thumbnail || show.background || placeholderImg}
						alt=""
						loading="lazy"
						className="aspect-video w-full rounded object-cover"
						onError={e => {
							(e.target as HTMLImageElement).src = placeholderImg;
						}}
					/>
					{pct > 0 && (
						<div className="absolute right-0 bottom-0 left-0 h-1 bg-black/50">
							<div className="h-full bg-red-500" style={{ width: `${pct}%` }} />
						</div>
					)}
				</div>
				<div className="flex min-w-0 flex-1 flex-col gap-1">
					<span className="text-sm font-medium">
						{episodeTag(ep.season, ep.episode)} · {ep.title || `Episódio ${ep.episode}`}
						{watched && <span className="ml-2 text-xs text-green-600">✓ visto</span>}
					</span>
					{ep.released && (
						<span className="text-muted-foreground text-xs">
							{new Date(ep.released).toLocaleDateString("pt-PT")}
						</span>
					)}
					{ep.overview && (
						<p className="text-muted-foreground line-clamp-2 text-xs">{ep.overview}</p>
					)}
					<span className="mt-1 flex flex-wrap gap-2">
						<Button size="sm" onClick={() => void playBest()} disabled={busy || !isAired}>
							{busy
								? "A procurar…"
								: !isAired
									? "Ainda não saiu"
									: resumeAt > 0
										? `▶ Continuar (${formatClock(resumeAt)})`
										: "▶ Play"}
						</Button>
						<Button size="sm" variant="outline" onClick={toggleSources} disabled={!isAired}>
							{showSources ? "Esconder fontes" : "Fontes"}
						</Button>
					</span>
					{error && <span className="text-xs text-red-500">{error}</span>}
				</div>
			</div>
			{showSources && (
				<ExtraSources
					title="Fontes (Torrentio · Kickass)"
					items={sources}
					failed={sourcesFailed}
					knownHashes={[]}
					onPlay={(magnet, _t, hint) =>
						onPlay({
							key,
							kind: "episode",
							magnet,
							hint: { ...hint, season: ep.season, episode: ep.episode },
							title: `${show.name} · ${episodeTag(ep.season, ep.episode)} · ${ep.title}`,
							imdbCode: show.id,
							poster: show.poster,
							showTitle: show.name,
							season: ep.season,
							episode: ep.episode,
							episodeTitle: ep.title,
							startAt: resumeAt,
						})
					}
				/>
			)}
		</div>
	);
}

function SeriesDialog({
	summary,
	onPlay,
	onLoaded,
}: {
	summary: SeriesSummary;
	onPlay: (req: PlayRequest) => void;
	onLoaded: (d: SeriesDetail) => void;
}) {
	const [detail, setDetail] = useState<SeriesDetail | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [season, setSeason] = useState<number | null>(null);

	useEffect(() => {
		let cancelled = false;
		setDetail(null);
		setError(null);
		seriesMeta(summary.id)
			.then(d => {
				if (cancelled) return;
				setDetail(d);
				onLoaded(d);
				// open on the season you were watching, else the first one
				const last = lastEpisode(d.id);
				const first = d.episodes.find(e => e.season > 0)?.season ?? d.episodes[0]?.season ?? 1;
				setSeason(last?.season ?? first);
			})
			.catch(e => {
				if (!cancelled) setError(e instanceof Error ? e.message : String(e));
			});
		return () => {
			cancelled = true;
		};
	}, [summary.id, onLoaded]);

	const seasons = useMemo(
		() =>
			[...new Set((detail?.episodes ?? []).map(e => e.season))].sort((a, b) =>
				a === 0 ? 1 : b === 0 ? -1 : a - b
			),
		[detail]
	);
	const episodes = (detail?.episodes ?? []).filter(e => e.season === season);
	const shown = detail ?? {
		...summary,
		background: "",
		description: "",
		genres: [],
		runtime: "",
		trailer: "",
	};

	return (
		<>
			<DialogTitle className="sr-only">{shown.name}</DialogTitle>
			{shown.background && (
				<img
					src={shown.background}
					alt=""
					className="-mx-6 -mt-6 mb-2 h-44 w-[calc(100%+3rem)] object-cover"
				/>
			)}
			<div className="flex flex-col gap-3">
				<div>
					<h2 className="text-xl font-semibold">{shown.name}</h2>
					<p className="text-muted-foreground text-sm">
						{shown.year}
						{shown.rating && ` · ⭐ ${shown.rating}`}
						{shown.runtime && ` · ${shown.runtime}`}
					</p>
					<div className="mt-1 flex flex-wrap gap-1">
						{shown.genres.map(g => (
							<Badge key={g} variant="outline">
								{g}
							</Badge>
						))}
					</div>
				</div>
				<div className="flex flex-wrap items-center gap-2">
					<WatchlistButton
						item={{
							kind: "series",
							imdb: summary.id,
							title: shown.name,
							poster: shown.poster,
							year: shown.year,
						}}
					/>
					<Trailer youtubeId={shown.trailer} />
				</div>
				{shown.description && <p className="text-sm">{shown.description}</p>}
				{error && <p className="text-sm text-red-500">{error}</p>}
				{!detail && !error && (
					<p className="text-muted-foreground text-sm">A carregar episódios…</p>
				)}
				{seasons.length > 0 && (
					<fieldset className="m-0 flex flex-wrap gap-2 border-0 p-0" aria-label="Temporadas">
						{seasons.map(s => (
							<Button
								key={s}
								size="sm"
								variant={season === s ? "default" : "outline"}
								aria-pressed={season === s}
								onClick={() => setSeason(s)}
							>
								{s === 0 ? "Especiais" : `Temporada ${s}`}
							</Button>
						))}
					</fieldset>
				)}
				<div className="flex flex-col gap-2">
					{detail &&
						episodes.map(ep => (
							<EpisodeRow
								key={`${ep.season}-${ep.episode}`}
								show={detail}
								ep={ep}
								onPlay={onPlay}
							/>
						))}
				</div>
			</div>
		</>
	);
}

export function SeriesRoute() {
	const [query, setQuery] = useState("");
	const [applied, setApplied] = useState({ query: "", genre: "" });
	const [items, setItems] = useState<SeriesSummary[]>([]);
	const [hasMore, setHasMore] = useState(true);
	const [isLoading, setIsLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [selected, setSelected] = useState<SeriesSummary | null>(null);
	const [playing, setPlaying] = useState<PlayRequest | null>(null);
	const [nextError, setNextError] = useState<string | null>(null);
	// episode lists of shows opened this session (to roll on to the next one)
	const details = useRef(new Map<string, SeriesDetail>());
	const [detailsVersion, setDetailsVersion] = useState(0);
	const requestId = useRef(0);

	const loadPage = useCallback(async (skip: number, filters: typeof applied, append: boolean) => {
		const id = ++requestId.current;
		setIsLoading(true);
		setError(null);
		try {
			const res = await seriesCatalog(filters.genre || undefined, filters.query || undefined, skip);
			if (id !== requestId.current) return;
			setItems(prev => {
				if (!append) return res;
				const seen = new Set(prev.map(p => p.id));
				return [...prev, ...res.filter(r => !seen.has(r.id))];
			});
			// search returns one short page; listings come in pages of 50
			setHasMore(!filters.query && res.length >= PAGE);
		} catch (e) {
			if (id !== requestId.current) return;
			setError(e instanceof Error ? e.message : "Failed to load series");
		} finally {
			if (id === requestId.current) setIsLoading(false);
		}
	}, []);

	useEffect(() => {
		void loadPage(0, applied, false);
	}, [applied, loadPage]);

	const onLoadMore = useCallback(() => {
		if (!isLoading && hasMore) void loadPage(items.length, applied, true);
	}, [isLoading, hasMore, loadPage, items.length, applied]);

	const [sentryRef] = useInfiniteScroll({
		loading: isLoading,
		hasNextPage: hasMore,
		onLoadMore,
		rootMargin: "0px 0px 500px 0px",
	});

	const remember = useCallback((d: SeriesDetail) => {
		details.current.set(d.id, d);
		setDetailsVersion(v => v + 1);
	}, []);
	const onLoaded = remember;

	const startPlaying = useCallback((req: PlayRequest) => {
		setNextError(null);
		setSelected(null);
		setPlaying(req);
	}, []);

	// what comes after the current episode
	const next: NextUp | null = useMemo(() => {
		if (!playing || playing.kind !== "episode" || !playing.season || !playing.episode) return null;
		const show = details.current.get(playing.imdbCode);
		if (!show) return null;
		const ep = nextEpisode(show.episodes, playing.season, playing.episode);
		if (!ep || !aired(ep)) return null;
		return {
			label: `${episodeTag(ep.season, ep.episode)} · ${ep.title}`,
			start: () => {
				requestForEpisode(show, ep)
					.then(req => setPlaying({ ...req, startAt: 0 }))
					.catch(e => setNextError(e instanceof Error ? e.message : String(e)));
			},
		};
	}, [playing, detailsVersion]);

	const resume = useCallback(
		(p: ProgressEntry) => {
			// make sure "next episode" works after resuming from the row
			if (!details.current.has(p.imdb)) {
				seriesMeta(p.imdb)
					.then(remember)
					.catch(() => {});
			}
			setPlaying({
				key: p.key,
				kind: "episode",
				magnet: p.magnet,
				hint: p.hint,
				title: `${p.showTitle ?? p.title}${p.season && p.episode ? ` · ${episodeTag(p.season, p.episode)}` : ""}${p.episodeTitle ? ` · ${p.episodeTitle}` : ""}`,
				imdbCode: p.imdb,
				poster: p.poster,
				showTitle: p.showTitle,
				season: p.season,
				episode: p.episode,
				episodeTitle: p.episodeTitle,
				startAt: resumePosition(p),
			});
		},
		[remember]
	);

	return (
		<div className="flex flex-col gap-4 p-4">
			<div className="flex flex-wrap items-center gap-2">
				<h1 className="mr-auto text-2xl font-semibold">Séries</h1>
				<span className="text-muted-foreground text-sm">Cinemeta · Torrentio · Kickass</span>
			</div>

			<ContinueRow kind="episode" onResume={resume} />
			<WatchlistRow
				kind="series"
				onOpen={w =>
					setSelected({
						id: w.imdb,
						name: w.title,
						poster: w.poster ?? "",
						year: w.year ?? "",
						rating: "",
					})
				}
			/>

			<form
				className="flex flex-wrap gap-2"
				onSubmit={e => {
					e.preventDefault();
					setApplied(a => ({ ...a, query }));
				}}
			>
				<Input
					className="max-w-xs"
					placeholder="Procurar série…"
					value={query}
					onChange={e => setQuery(e.target.value)}
				/>
				<Button type="submit" disabled={isLoading}>
					{isLoading ? "Loading…" : "Search"}
				</Button>
			</form>

			<fieldset className="m-0 flex flex-wrap gap-2 border-0 p-0" aria-label="Genres">
				{SERIES_GENRES.map(g => (
					<Button
						key={g.value || "all"}
						type="button"
						size="sm"
						variant={applied.genre === g.value ? "default" : "outline"}
						aria-pressed={applied.genre === g.value}
						onClick={() => setApplied(a => ({ ...a, genre: g.value }))}
					>
						{g.label}
					</Button>
				))}
			</fieldset>

			{error && <p className="text-sm text-red-500">{error}</p>}

			<div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
				{items.map(m => (
					<button
						key={m.id}
						type="button"
						onClick={() => setSelected(m)}
						className="group flex flex-col overflow-hidden rounded-xl border text-left transition hover:ring-2 hover:ring-accent"
					>
						<img
							src={m.poster || placeholderImg}
							alt={m.name}
							loading="lazy"
							className="aspect-[2/3] w-full object-cover"
							onError={e => {
								(e.target as HTMLImageElement).src = placeholderImg;
							}}
						/>
						<div className="flex flex-col gap-1 p-2">
							<span className="truncate text-sm font-medium">{m.name}</span>
							<span className="text-muted-foreground text-xs">
								{m.year}
								{m.rating && ` · ⭐ ${m.rating}`}
							</span>
						</div>
					</button>
				))}
			</div>

			{items.length === 0 && !isLoading && !error && (
				<p className="text-muted-foreground text-sm">Nenhuma série encontrada.</p>
			)}

			{(hasMore || isLoading) && (
				<div ref={sentryRef} className="flex justify-center py-4">
					<span className="text-muted-foreground text-sm">
						{isLoading ? "Loading more…" : "Scroll for more"}
					</span>
				</div>
			)}

			<Dialog
				open={selected !== null}
				onOpenChange={open => {
					if (!open) setSelected(null);
				}}
			>
				<DialogContent className="max-h-[85vh] grid-cols-[minmax(0,1fr)] overflow-x-hidden overflow-y-auto sm:max-w-3xl">
					{selected && (
						<SeriesDialog
							key={selected.id}
							summary={selected}
							onPlay={startPlaying}
							onLoaded={onLoaded}
						/>
					)}
				</DialogContent>
			</Dialog>

			{playing && (
				<Player
					request={playing}
					next={next}
					onClose={() => {
						const show = details.current.get(playing.imdbCode);
						setPlaying(null);
						if (show)
							setSelected({
								id: show.id,
								name: show.name,
								poster: show.poster,
								year: show.year,
								rating: show.rating,
							});
					}}
				/>
			)}
			{nextError && playing && (
				<p className="fixed right-6 bottom-6 z-[110] rounded bg-black/90 px-4 py-2 text-sm text-red-400">
					{nextError}
				</p>
			)}
		</div>
	);
}
