import { Badge } from "@popcorntime/ui/components/badge";
import { Button, buttonVariants } from "@popcorntime/ui/components/button";
import { Dialog, DialogContent, DialogTitle } from "@popcorntime/ui/components/dialog";
import { Input } from "@popcorntime/ui/components/input";
import { cn } from "@popcorntime/ui/lib/utils";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import useInfiniteScroll from "react-infinite-scroll-hook";
import placeholderImg from "@/assets/placeholder.svg";
import { type YtsMovie, mergeYtsMovie, ytsListMovies, ytsMagnetUrl, ytsMovieDetails } from "@/tauri/yts";

const SORTS = [
	{ value: "seeds", label: "Seeds" },
	{ value: "peers", label: "Peers" },
	{ value: "rating", label: "Rating" },
	{ value: "year", label: "Year" },
	{ value: "date_added", label: "Latest" },
	{ value: "download_count", label: "Downloads" },
] as const;

const QUALITIES = ["", "480p", "720p", "1080p", "1080p.x265", "2160p", "3D"] as const;

const PAGE_SIZE = 20;

/** Poster with fallback chain: medium → small → large → placeholder. */
function CoverImg({ movie }: { movie: YtsMovie }) {
	const candidates = [
		movie.medium_cover_image,
		movie.small_cover_image,
		movie.large_cover_image,
	].filter(Boolean);
	const [index, setIndex] = useState(0);
	const src = index < candidates.length ? candidates[index] : placeholderImg;

	return (
		<img
			src={src}
			alt={movie.title}
			loading="lazy"
			className="aspect-[2/3] w-full object-cover"
			onError={() => setIndex(i => i + 1)}
		/>
	);
}

function TorrentRow({
	torrent,
	movieTitle,
}: {
	torrent: YtsMovie["torrents"][number];
	movieTitle: string;
}) {
	const [copied, setCopied] = useState(false);
	const [openError, setOpenError] = useState<string | null>(null);

	const copyMagnet = useCallback(async () => {
		try {
			const magnet = await ytsMagnetUrl(torrent.hash, movieTitle);
			await navigator.clipboard.writeText(magnet);
			setCopied(true);
			setTimeout(() => setCopied(false), 2000);
		} catch {
			setOpenError("Failed to build magnet link");
		}
	}, [torrent.hash, movieTitle]);

	const openMagnet = useCallback(async () => {
		try {
			const magnet = await ytsMagnetUrl(torrent.hash, movieTitle);
			await openUrl(magnet);
		} catch {
			setOpenError("No torrent app handled the magnet link — copy it instead");
		}
	}, [torrent.hash, movieTitle]);

	return (
		<div className="flex flex-wrap items-center gap-2 rounded-lg border p-2 text-sm">
			<Badge variant="secondary">{torrent.quality}</Badge>
			<span className="text-muted-foreground">{torrent.type}</span>
			<span>{torrent.size}</span>
			<span className="text-muted-foreground">
				{torrent.seeds} seeds / {torrent.peers} peers
			</span>
			<span className="ml-auto flex gap-2">
				<Button size="sm" variant="outline" onClick={copyMagnet}>
					{copied ? "Copied!" : "Copy magnet"}
				</Button>
				<Button size="sm" onClick={openMagnet}>
					Open
				</Button>
			</span>
			{openError && <span className="w-full text-xs text-red-500">{openError}</span>}
		</div>
	);
}

function MovieDialog({ movie }: { movie: YtsMovie }) {
	const [details, setDetails] = useState<YtsMovie | null>(null);

	useEffect(() => {
		let cancelled = false;
		setDetails(null);
		ytsMovieDetails({ movieId: movie.id, withCast: true })
			.then(res => {
				if (!cancelled && res.status === "ok") setDetails(res.data.movie);
			})
			.catch(console.error);
		return () => {
			cancelled = true;
		};
	}, [movie.id]);

	const shown = useMemo(() => mergeYtsMovie(movie, details), [movie, details]);
	// Plot fallback chain: many fresh uploads ship an empty synopsis,
	// but description/summary usually carry the text.
	const plot =
		shown.synopsis || shown.description_full || shown.description_intro || shown.summary;
	const imdbUrl = shown.imdb_code ? `https://www.imdb.com/title/${shown.imdb_code}/` : null;
	const screenshots = [
		shown.medium_screenshot_image1,
		shown.medium_screenshot_image2,
		shown.medium_screenshot_image3,
	].filter(Boolean);
	const topCast = (shown.cast ?? []).slice(0, 6);

	return (
		<>
			<DialogTitle className="sr-only">{shown.title_long || shown.title}</DialogTitle>
			{shown.background_image && (
				<img
					src={shown.background_image}
					alt=""
					className="-mx-6 -mt-6 mb-2 h-40 w-[calc(100%+3rem)] object-cover"
				/>
			)}
			<div className="flex flex-col gap-3">
				<div>
					<h2 className="text-xl font-semibold">{shown.title_long || shown.title}</h2>
					<p className="text-muted-foreground text-sm">
						{shown.year} · ⭐ {shown.rating} · {shown.runtime} min · {shown.mpa_rating}
					</p>
					<div className="mt-1 flex flex-wrap gap-1">
						{shown.genres.map(g => (
							<Badge key={g} variant="outline">
								{g}
							</Badge>
						))}
					</div>
				</div>
				{plot && <p className="text-sm">{plot}</p>}
				<div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
					<span title="IMDb rating">⭐ {shown.rating}</span>
					{imdbUrl && (
						<button
							type="button"
							className="text-blue-500 underline"
							onClick={() => void openUrl(imdbUrl)}
						>
							IMDb: {shown.imdb_code}
						</button>
					)}
					{shown.like_count > 0 && <span>👍 {shown.like_count.toLocaleString()}</span>}
					{shown.download_count > 0 && <span>⬇ {shown.download_count.toLocaleString()}</span>}
				</div>
				{screenshots.length > 0 && (
					<div className="flex gap-2 overflow-x-auto">
						{screenshots.map(src => (
							<img
								key={src}
								src={src}
								alt=""
								loading="lazy"
								className="h-24 rounded-lg object-cover"
								onError={e => {
									(e.target as HTMLImageElement).style.display = "none";
								}}
							/>
						))}
					</div>
				)}
				{topCast.length > 0 && (
					<div className="flex flex-col gap-1">
						<span className="text-sm font-medium">Cast</span>
						<div className="flex gap-3 overflow-x-auto">
							{topCast.map(c => (
								<div key={`${c.name}-${c.character_name}`} className="flex w-20 shrink-0 flex-col items-center gap-1 text-center">
									{c.url_small_image ? (
										<img
											src={c.url_small_image}
											alt={c.name}
											loading="lazy"
											className="h-20 w-20 rounded-full object-cover"
											onError={e => {
												(e.target as HTMLImageElement).style.display = "none";
											}}
										/>
									) : (
										<div className="bg-muted flex h-20 w-20 items-center justify-center rounded-full text-lg">
											{c.name.charAt(0)}
										</div>
									)}
									<span className="text-xs font-medium">{c.name}</span>
									{c.character_name && (
										<span className="text-muted-foreground text-[11px]">{c.character_name}</span>
									)}
								</div>
							))}
						</div>
					</div>
				)}
				<div className="flex flex-col gap-2">
					{shown.torrents.map(t => (
						<TorrentRow key={t.hash} torrent={t} movieTitle={shown.title_long || shown.title} />
					))}
					{shown.torrents.length === 0 && (
						<p className="text-muted-foreground text-sm">No torrents listed for this movie.</p>
					)}
				</div>
			</div>
		</>
	);
}

export function MoviesRoute() {
	const [query, setQuery] = useState("");
	const [sortBy, setSortBy] = useState<string>("seeds");
	const [quality, setQuality] = useState<string>("");
	const [movies, setMovies] = useState<YtsMovie[]>([]);
	const [movieCount, setMovieCount] = useState(0);
	const [page, setPage] = useState(1);
	const [isLoading, setIsLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [selected, setSelected] = useState<YtsMovie | null>(null);
	// Filters actually applied to the accumulated list (set on Search).
	const [applied, setApplied] = useState({ query: "", sortBy: "seeds", quality: "" });
	const requestId = useRef(0);

	const loadPage = useCallback(
		async (pageNumber: number, filters: typeof applied, append: boolean) => {
			const id = ++requestId.current;
			setIsLoading(true);
			setError(null);
			try {
				const res = await ytsListMovies({
					limit: PAGE_SIZE,
					page: pageNumber,
					queryTerm: filters.query || undefined,
					sortBy: filters.sortBy,
					orderBy: "desc",
					quality: filters.quality || undefined,
				});
				if (id !== requestId.current) return;
				if (res.status !== "ok") throw new Error(res.status_message);
				setMovies(prev =>
					append ? [...prev, ...(res.data.movies ?? [])] : (res.data.movies ?? [])
				);
				setMovieCount(res.data.movie_count ?? 0);
				setPage(res.data.page_number || pageNumber);
			} catch (e) {
				if (id !== requestId.current) return;
				setError(e instanceof Error ? e.message : "Failed to load movies");
			} finally {
				if (id === requestId.current) setIsLoading(false);
			}
		},
		[]
	);

	// Fresh search whenever applied filters change.
	useEffect(() => {
		setSelected(null);
		void loadPage(1, applied, false);
	}, [applied, loadPage]);

	const hasNextPage = movies.length < movieCount;

	const onLoadMore = useCallback(() => {
		if (!isLoading && hasNextPage) {
			void loadPage(page + 1, applied, true);
		}
	}, [isLoading, hasNextPage, loadPage, page, applied]);

	const [sentryRef] = useInfiniteScroll({
		loading: isLoading,
		hasNextPage,
		onLoadMore,
		rootMargin: "0px 0px 500px 0px",
	});

	return (
		<div className="flex flex-col gap-4 p-4">
			<div className="flex flex-wrap items-center gap-2">
				<h1 className="mr-auto text-2xl font-semibold">YTS Movies</h1>
				<span className="text-muted-foreground text-sm">
					{movieCount > 0
						? `${movies.length.toLocaleString()} / ${movieCount.toLocaleString()} movies`
						: "no account needed"}
				</span>
			</div>

			<form
				className="flex flex-wrap gap-2"
				onSubmit={e => {
					e.preventDefault();
					setApplied({ query, sortBy, quality });
				}}
			>
				<Input
					className="max-w-xs"
					placeholder="Search title, actor, director, IMDb id…"
					value={query}
					onChange={e => setQuery(e.target.value)}
				/>
				<select
					className={cn(buttonVariants({ variant: "outline", size: "default" }), "cursor-pointer")}
					value={sortBy}
					onChange={e => setSortBy(e.target.value)}
					aria-label="Sort by"
				>
					{SORTS.map(s => (
						<option key={s.value} value={s.value}>
							{s.label}
						</option>
					))}
				</select>
				<select
					className={cn(buttonVariants({ variant: "outline", size: "default" }), "cursor-pointer")}
					value={quality}
					onChange={e => setQuality(e.target.value)}
					aria-label="Quality"
				>
					{QUALITIES.map(q => (
						<option key={q} value={q}>
							{q || "All qualities"}
						</option>
					))}
				</select>
				<Button type="submit" disabled={isLoading}>
					{isLoading ? "Loading…" : "Search"}
				</Button>
			</form>

			{error && <p className="text-sm text-red-500">{error}</p>}

			<div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
				{movies.map(m => (
					<button
						key={m.id}
						type="button"
						onClick={() => setSelected(prev => (prev?.id === m.id ? null : m))}
						className="group flex flex-col overflow-hidden rounded-xl border text-left transition hover:ring-2 hover:ring-accent"
					>
						<CoverImg movie={m} />
						<div className="flex flex-col gap-1 p-2">
							<span className="truncate text-sm font-medium">{m.title}</span>
							<span className="text-muted-foreground text-xs">
								{m.year} · ⭐ {m.rating}
							</span>
						</div>
					</button>
				))}
			</div>

			{movies.length === 0 && !isLoading && !error && (
				<p className="text-muted-foreground text-sm">No movies found.</p>
			)}

			{/* Infinite-scroll sentinel: appends the next page as it nears the viewport. */}
			{(hasNextPage || isLoading) && (
				<div ref={sentryRef} className="flex justify-center py-4">
					<span className="text-muted-foreground text-sm">
						{isLoading ? "Loading more…" : "Scroll for more"}
					</span>
				</div>
			)}

			{/* Suspended window: one click opens, clicking the same cover (or X/backdrop) closes. */}
			<Dialog
				open={selected !== null}
				onOpenChange={open => {
					if (!open) setSelected(null);
				}}
			>
				<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
					{selected && <MovieDialog key={selected.id} movie={selected} />}
				</DialogContent>
			</Dialog>
		</div>
	);
}
