import { Button } from "@popcorntime/ui/components/button";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { formatClock, type ProgressKind, saveProgress } from "@/lib/library";
import {
	type FileHint,
	openInVlc,
	parseVtt,
	type StreamInfo,
	type StreamStatus,
	type SubtitleTrack,
	srtToVtt,
	streamPlay,
	streamStatus,
	streamStop,
	subtitlesList,
	subtitleTranslateCancel,
	subtitleTranslateProgress,
	subtitleTranslateStart,
	subtitleVtt,
	type TranslationProgress,
} from "@/tauri/yts";

export interface PlayRequest {
	/** library key: `movie:tt…` or `ep:tt…:s:e` */
	key: string;
	kind: ProgressKind;
	magnet: string;
	/** what the toolbar shows */
	title: string;
	imdbCode: string;
	hint?: FileHint;
	poster?: string;
	showTitle?: string;
	season?: number;
	episode?: number;
	episodeTitle?: string;
	/** resume position in seconds */
	startAt?: number;
}

export interface NextUp {
	label: string;
	start: () => void;
}

const OFFSET_STEP = 0.5;
const HIDE_CONTROLS_MS = 3000;
const SAVE_EVERY_MS = 5000;
const NEXT_COUNTDOWN = 10;
/** Virtual track: the best English subtitle, translated to PT on the Mac. */
export const TRANSLATED_ID = "tr-por";
/** Start once the first batch is in (≈ the first 1-2 minutes of film)… */
const LEAD_CUES = 25;
/** …or this much film is covered from the start point. */
const LEAD_SECONDS = 120;
/** Never hold the film longer than this waiting for subtitles. */
const MAX_WAIT_MS = 120_000;
const TRANSLATE_POLL_MS = 2000;

/**
 * WKWebView ships with the HTML Fullscreen API disabled, so the <video>
 * fullscreen button does nothing. Fullscreen is the native window instead:
 * the player already covers the whole window.
 */
async function setWindowFullscreen(on: boolean) {
	try {
		await getCurrentWindow().setFullscreen(on);
	} catch {
		// not running inside Tauri (tests) — ignore
	}
}

function mb(bytes: number) {
	return `${(bytes / 1024 / 1024).toFixed(0)} MB`;
}

function errorText(e: unknown) {
	if (e instanceof Error) return e.message;
	if (e && typeof e === "object" && "message" in e)
		return String((e as { message: unknown }).message);
	return String(e);
}

/** Replace the cues of the player's subtitle track (offset in seconds). */
function applyCues(
	video: HTMLVideoElement | null,
	trackRef: { current: TextTrack | null },
	vtt: string | null,
	offset: number
) {
	if (!video || typeof video.addTextTrack !== "function" || typeof VTTCue === "undefined") return;
	if (!trackRef.current) trackRef.current = video.addTextTrack("subtitles", "Subtitles");
	const track = trackRef.current;
	for (const cue of Array.from(track.cues ?? [])) track.removeCue(cue);
	if (!vtt) {
		track.mode = "disabled";
		return;
	}
	for (const c of parseVtt(vtt)) {
		const start = Math.max(0, c.start + offset);
		const end = Math.max(start, c.end + offset);
		track.addCue(new VTTCue(start, end, c.text));
	}
	track.mode = "showing";
}

/**
 * Full-window player: streams the torrent through the local engine, plays
 * mp4/mov/webm directly and mkv/avi through the ffmpeg HLS remux, offers VLC
 * as a fallback, overlays OpenSubtitles tracks, remembers where you stopped
 * and rolls on to the next episode.
 */
export function Player({
	request,
	onClose,
	next,
}: {
	request: PlayRequest;
	onClose: () => void;
	next?: NextUp | null;
}) {
	const videoRef = useRef<HTMLVideoElement>(null);
	const trackRef = useRef<TextTrack | null>(null);
	const [info, setInfo] = useState<StreamInfo | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [videoError, setVideoError] = useState(false);
	const [status, setStatus] = useState<StreamStatus | null>(null);
	const [subs, setSubs] = useState<SubtitleTrack[] | null>(null);
	const [subId, setSubId] = useState<string>("");
	const [vtt, setVtt] = useState<string | null>(null);
	const [offset, setOffset] = useState(0);
	const [subError, setSubError] = useState<string | null>(null);
	const [fullscreen, setFullscreen] = useState(false);
	const [idle, setIdle] = useState(false);
	const [resumedAt, setResumedAt] = useState(0);
	const [countdown, setCountdown] = useState<number | null>(null);
	const [translation, setTranslation] = useState<TranslationProgress | null>(null);
	const [waitedOut, setWaitedOut] = useState(false);
	const [skipWait, setSkipWait] = useState(false);
	const [translateNote, setTranslateNote] = useState<string | null>(null);
	// bumping this restarts the stream (used by "start over" on remuxed files)
	const [restart, setRestart] = useState<{ key: string; n: number }>({ key: "", n: 0 });
	/** where this stream starts: the resume point, or 0 after "start over" */
	const startAt = restart.key === request.key ? 0 : (request.startAt ?? 0);
	const idleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
	const infoRef = useRef<StreamInfo | null>(null);
	infoRef.current = info;
	const requestRef = useRef(request);
	requestRef.current = request;

	const fullscreenRef = useRef(false);
	const toggleFullscreen = useCallback(() => {
		const nextFs = !fullscreenRef.current;
		fullscreenRef.current = nextFs;
		setFullscreen(nextFs);
		void setWindowFullscreen(nextFs);
	}, []);

	/** Film position: remuxed streams start at `time_offset`. */
	const position = useCallback(() => {
		const v = videoRef.current;
		const i = infoRef.current;
		if (!v || !i) return 0;
		return v.currentTime + (i.remuxed ? i.time_offset : 0);
	}, []);

	const duration = useCallback(() => {
		const v = videoRef.current;
		const i = infoRef.current;
		if (i?.duration && i.duration > 0) return i.duration;
		return v && Number.isFinite(v.duration) ? v.duration : 0;
	}, []);

	const persist = useCallback(
		(pos?: number) => {
			const r = requestRef.current;
			const p = pos ?? position();
			if (!infoRef.current || p <= 0) return;
			saveProgress({
				key: r.key,
				kind: r.kind,
				imdb: r.imdbCode,
				title: r.title,
				poster: r.poster,
				showTitle: r.showTitle,
				season: r.season,
				episode: r.episode,
				episodeTitle: r.episodeTitle,
				magnet: r.magnet,
				hint: r.hint,
				position: p,
				duration: duration(),
			});
		},
		[position, duration]
	);

	const close = useCallback(() => {
		persist();
		fullscreenRef.current = false;
		void setWindowFullscreen(false);
		onClose();
	}, [onClose, persist]);

	// leave fullscreen if the player unmounts any other way
	useEffect(() => () => void setWindowFullscreen(false), []);

	// keyboard: F toggles fullscreen, Esc leaves it (or closes), space pauses
	useEffect(() => {
		const onKey = (e: KeyboardEvent) => {
			if (e.target instanceof HTMLSelectElement || e.target instanceof HTMLInputElement) return;
			if (e.key === "f" || e.key === "F") {
				e.preventDefault();
				toggleFullscreen();
			} else if (e.key === "Escape") {
				e.preventDefault();
				if (fullscreen) toggleFullscreen();
				else close();
			} else if (e.key === " " && videoRef.current) {
				e.preventDefault();
				if (videoRef.current.paused) void videoRef.current.play();
				else videoRef.current.pause();
			}
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [fullscreen, toggleFullscreen, close]);

	// in fullscreen the toolbar and cursor hide after a few idle seconds
	const wake = useCallback(() => {
		setIdle(false);
		if (idleTimer.current) clearTimeout(idleTimer.current);
		idleTimer.current = setTimeout(() => setIdle(true), HIDE_CONTROLS_MS);
	}, []);
	useEffect(() => {
		wake();
		return () => {
			if (idleTimer.current) clearTimeout(idleTimer.current);
		};
	}, [wake]);
	const hideChrome = fullscreen && idle && countdown === null;

	// start streaming; drop the torrent (and its cache) when this stream ends
	useEffect(() => {
		let cancelled = false;
		let startedId: number | null = null;
		setInfo(null);
		setError(null);
		setVideoError(false);
		setStatus(null);
		setSubs(null);
		setSubId("");
		setVtt(null);
		setCountdown(null);
		setResumedAt(0);
		setTranslation(null);
		setWaitedOut(false);
		setSkipWait(false);
		trackRef.current = null;
		streamPlay(request.magnet, request.hint, startAt)
			.then(res => {
				startedId = res.torrent_id;
				if (cancelled) {
					void streamStop(res.torrent_id);
					return;
				}
				setInfo(res);
				if (res.remuxed && res.time_offset > 0) setResumedAt(res.time_offset);
			})
			.catch(e => {
				if (!cancelled) setError(errorText(e));
			});
		return () => {
			cancelled = true;
			persist();
			// only stops if nothing newer started meanwhile
			if (startedId !== null) void streamStop(startedId);
		};
	}, [request.magnet, request.key, restart.n]);

	// native files: jump to the saved position once metadata is in
	const onLoadedMetadata = useCallback(() => {
		const v = videoRef.current;
		const i = infoRef.current;
		if (v && i && !i.remuxed && startAt > 0 && startAt < (duration() || Infinity) - 5) {
			v.currentTime = startAt;
			setResumedAt(startAt);
		}
		applyCues(videoRef.current, trackRef, vtt, offset - (i?.remuxed ? i.time_offset : 0));
	}, [startAt, duration, vtt, offset]);

	// hide the "resumed" banner after a while
	useEffect(() => {
		if (!resumedAt) return;
		const t = setTimeout(() => setResumedAt(0), 8000);
		return () => clearTimeout(t);
	}, [resumedAt]);

	const startOver = useCallback(() => {
		setResumedAt(0);
		if (info?.remuxed) setRestart(r => ({ key: request.key, n: r.n + 1 }));
		else if (videoRef.current) videoRef.current.currentTime = 0;
	}, [info, request.key]);

	// save progress while playing
	useEffect(() => {
		if (!info) return;
		const id = setInterval(() => {
			if (videoRef.current && !videoRef.current.paused) persist();
		}, SAVE_EVERY_MS);
		return () => clearInterval(id);
	}, [info, persist]);

	// subtitles: listed right away so a translation can run while the film buffers
	useEffect(() => {
		let cancelled = false;
		setSubs(null);
		setSubId("");
		subtitlesList(request.imdbCode, request.season, request.episode)
			.then(list => {
				if (cancelled) return;
				const hasPt = list.some(t => t.lang === "por" || t.lang === "pob");
				const english = list.find(t => t.lang === "eng");
				const tracks =
					!hasPt && english
						? [
								{
									id: TRANSLATED_ID,
									lang: "por",
									label: "Português (traduzido do inglês)",
									url: english.url,
								},
								...list,
							]
						: list;
				setSubs(tracks);
				if (tracks[0]) setSubId(tracks[0].id);
			})
			.catch(() => {
				if (!cancelled) setSubs([]);
			});
		return () => {
			cancelled = true;
		};
	}, [request.imdbCode, request.season, request.episode, request.key]);

	// on-device translation of the English track, from the start point onward
	useEffect(() => {
		if (subId !== TRANSLATED_ID || !subs) return;
		const source = subs.find(s => s.id === TRANSLATED_ID);
		if (!source) return;
		let cancelled = false;
		let timer: ReturnType<typeof setTimeout> | null = null;
		setTranslation(null);
		setTranslateNote(null);
		const fallBackToEnglish = (msg: string) => {
			setTranslateNote(`Tradução indisponível — a usar inglês. ${msg}`);
			const english = subs.find(s => s.lang === "eng");
			setSubId(english?.id ?? "");
		};
		subtitleTranslateStart(source.url, startAt)
			.then(jobId => {
				const poll = () => {
					subtitleTranslateProgress(jobId)
						.then(p => {
							if (cancelled || !p) return;
							setTranslation(p);
							if (p.done > 0) setVtt(p.vtt);
							if (p.error) {
								if (p.done === 0) fallBackToEnglish(p.error);
								return;
							}
							if (!p.finished) timer = setTimeout(poll, TRANSLATE_POLL_MS);
						})
						.catch(() => {
							if (!cancelled) timer = setTimeout(poll, TRANSLATE_POLL_MS);
						});
				};
				if (!cancelled) poll();
			})
			.catch(e => {
				if (!cancelled) fallBackToEnglish(errorText(e));
			});
		return () => {
			cancelled = true;
			if (timer) clearTimeout(timer);
			void subtitleTranslateCancel();
		};
	}, [subId, subs, startAt]);

	// don't hold the film forever waiting for subtitles
	useEffect(() => {
		const t = setTimeout(() => setWaitedOut(true), MAX_WAIT_MS);
		return () => clearTimeout(t);
	}, [request.key]);

	// download the chosen subtitle (the translated one comes from its job)
	useEffect(() => {
		if (!subs || !subId || subId === TRANSLATED_ID) return;
		const track = subs.find(s => s.id === subId);
		if (!track) return;
		let cancelled = false;
		setSubError(null);
		subtitleVtt(track.url)
			.then(text => {
				if (!cancelled) setVtt(text);
			})
			.catch(() => {
				if (!cancelled) setSubError("Could not load that subtitle");
			});
		return () => {
			cancelled = true;
		};
	}, [subs, subId]);

	// a new subtitle starts in sync (translation updates keep the offset)
	useEffect(() => {
		if (subId) setOffset(0);
	}, [subId]);

	// (re)build cues whenever the text or the sync offset changes; remuxed
	// streams start at time_offset, so cues shift back by it
	useEffect(() => {
		applyCues(videoRef.current, trackRef, vtt, offset - (info?.remuxed ? info.time_offset : 0));
	}, [vtt, offset, info]);

	// live download stats, also while the stream is still being prepared
	useEffect(() => {
		if (error) return;
		const id = setInterval(() => {
			streamStatus()
				.then(setStatus)
				.catch(() => {});
		}, 1000);
		return () => clearInterval(id);
	}, [error]);

	// next-episode countdown
	useEffect(() => {
		if (countdown === null || !next) return;
		if (countdown <= 0) {
			setCountdown(null);
			next.start();
			return;
		}
		const t = setTimeout(() => setCountdown(c => (c === null ? null : c - 1)), 1000);
		return () => clearTimeout(t);
	}, [countdown, next]);

	const onEnded = useCallback(() => {
		const d = duration();
		persist(d > 0 ? d : position());
		if (next) setCountdown(NEXT_COUNTDOWN);
	}, [duration, persist, position, next]);

	const nudge = useCallback((delta: number) => {
		setOffset(o => Math.round((o + delta) * 10) / 10);
	}, []);

	const vlc = useCallback(() => {
		if (!info) return;
		openInVlc(info.url, vtt ?? undefined).catch(e => setError(errorText(e)));
	}, [info, vtt]);

	const onDrop = useCallback((e: React.DragEvent) => {
		e.preventDefault();
		const file = e.dataTransfer.files[0];
		if (!file || !/\.(srt|vtt)$/i.test(file.name)) return;
		void file.text().then(text => {
			setSubId("");
			setVtt(srtToVtt(text));
		});
	}, []);

	const playsHere = info?.in_app && info.play_url && !videoError;

	// Hold playback until the subtitle is ready: a translation needs its first
	// batch (≈ the first minutes); meanwhile the film keeps buffering.
	const translationLead =
		translation !== null &&
		(translation.finished ||
			translation.error !== null ||
			translation.done >= Math.min(LEAD_CUES, translation.total) ||
			translation.ready_until >= startAt + LEAD_SECONDS);
	const subtitlesReady =
		subs !== null &&
		(subs.length === 0 ||
			subId === "" ||
			(subId === TRANSLATED_ID ? translationLead : vtt !== null || subError !== null));
	const gateOpen = !!playsHere && (subtitlesReady || waitedOut || skipWait);
	const gateOpenRef = useRef(false);
	useEffect(() => {
		const v = videoRef.current;
		if (gateOpen && !gateOpenRef.current && v) {
			gateOpenRef.current = true;
			try {
				// jsdom (tests) returns undefined here
				v.play()?.catch(() => {});
			} catch {
				// autoplay refused: the controls are there
			}
		}
		if (!gateOpen) gateOpenRef.current = false;
	}, [gateOpen]);
	const pct =
		status && status.total_bytes > 0 ? (status.progress_bytes / status.total_bytes) * 100 : 0;

	return (
		<div
			role="dialog"
			aria-label={`Playing ${request.title}`}
			className="fixed inset-0 z-[100] flex flex-col bg-black text-white"
			onDragOver={e => e.preventDefault()}
			onDrop={onDrop}
			onMouseMove={wake}
			style={hideChrome ? { cursor: "none" } : undefined}
		>
			<div
				className="flex flex-wrap items-center gap-2 p-3 text-sm transition-opacity duration-300"
				style={hideChrome ? { opacity: 0, pointerEvents: "none" } : undefined}
			>
				<Button size="sm" variant="secondary" onClick={close}>
					✕ Close
				</Button>
				<span className="mr-auto truncate font-medium">{request.title}</span>
				{info?.remuxed && (
					<span
						className="rounded bg-white/10 px-2 py-0.5 text-xs text-white/70"
						title={info.file_name}
					>
						mkv → conversão ao vivo
					</span>
				)}
				{status && (
					<span className="text-white/70">
						{status.peers} peers · {status.download_mbps.toFixed(1)} MB/s ·{" "}
						{mb(status.progress_bytes)} ({pct.toFixed(1)}%)
					</span>
				)}
				{subs && subs.length > 0 && (
					<select
						aria-label="Subtitles"
						className="rounded bg-white/10 px-2 py-1"
						value={subId}
						onChange={e => {
							setSubId(e.target.value);
							if (!e.target.value) setVtt(null);
						}}
					>
						<option value="">No subtitles</option>
						{subs.map(s => (
							<option key={s.id} value={s.id}>
								{s.label}
							</option>
						))}
					</select>
				)}
				{subId === TRANSLATED_ID &&
					translation &&
					!translation.finished &&
					!translation.error &&
					gateOpen && (
						<span className="text-xs text-white/60">
							tradução {translation.done}/{translation.total}
						</span>
					)}
				{subs && subs.length === 0 && (
					<span className="text-white/60">No subtitles found — drop a .srt</span>
				)}
				{vtt && (
					<span className="flex items-center gap-1">
						<Button size="sm" variant="secondary" onClick={() => nudge(-OFFSET_STEP)}>
							−0.5s
						</Button>
						<span className="w-14 text-center tabular-nums">
							{offset > 0 ? `+${offset}` : offset}s
						</span>
						<Button size="sm" variant="secondary" onClick={() => nudge(OFFSET_STEP)}>
							+0.5s
						</Button>
					</span>
				)}
				{next && (
					<Button size="sm" variant="secondary" onClick={next.start} title={next.label}>
						⏭ Próximo
					</Button>
				)}
				{info && (
					<Button size="sm" variant="secondary" onClick={vlc}>
						Open in VLC
					</Button>
				)}
				<Button
					size="sm"
					variant="secondary"
					onClick={toggleFullscreen}
					aria-pressed={fullscreen}
					title="Fullscreen (F)"
				>
					{fullscreen ? "⤡ Exit fullscreen" : "⤢ Fullscreen"}
				</Button>
			</div>
			{subError && <p className="px-3 text-xs text-red-400">{subError}</p>}
			{translateNote && <p className="px-3 text-xs text-amber-300">{translateNote}</p>}

			<div className="relative flex min-h-0 flex-1 items-center justify-center">
				{error && <p className="max-w-lg p-6 text-center text-red-400">{error}</p>}
				{!error && !info && (
					<div className="flex flex-col items-center gap-1 text-center">
						<p className="text-white/70">A ligar aos peers e a preparar o vídeo…</p>
						{status && (
							<p className="text-xs text-white/50">
								{status.peers} peers · {status.download_mbps.toFixed(1)} MB/s
							</p>
						)}
					</div>
				)}
				{info && playsHere && (
					// biome-ignore lint/a11y/useMediaCaption: captions come from the subtitle picker
					<video
						ref={videoRef}
						src={info.play_url ?? undefined}
						controls={gateOpen}
						preload="auto"
						className="h-full w-full"
						style={gateOpen ? undefined : { visibility: "hidden" }}
						onDoubleClick={e => {
							e.preventDefault();
							toggleFullscreen();
						}}
						onError={() => setVideoError(true)}
						onLoadedMetadata={onLoadedMetadata}
						onEnded={onEnded}
					/>
				)}
				{info && playsHere && !gateOpen && (
					<div className="absolute inset-0 flex flex-col items-center justify-center gap-3 text-center">
						<p className="text-lg">A preparar o filme com legenda em português…</p>
						<ul className="flex flex-col gap-1 text-sm text-white/80">
							<li>
								✓ Vídeo pronto
								{status && ` · ${mb(status.progress_bytes)} em cache · ${status.peers} peers`}
							</li>
							<li>
								{subId === TRANSLATED_ID
									? translation
										? `⏳ A traduzir a legenda: ${Math.min(translation.done, LEAD_CUES)}/${Math.min(LEAD_CUES, translation.total)} falas`
										: "⏳ A traduzir a legenda…"
									: subs === null
										? "⏳ A procurar legendas…"
										: "⏳ A descarregar a legenda…"}
							</li>
						</ul>
						{subId === TRANSLATED_ID && translation && (
							<div className="h-1 w-64 overflow-hidden rounded bg-white/20">
								<div
									className="h-full bg-white transition-all"
									style={{
										width: `${Math.min(100, (translation.done / Math.max(1, Math.min(LEAD_CUES, translation.total))) * 100)}%`,
									}}
								/>
							</div>
						)}
						<Button size="sm" variant="secondary" onClick={() => setSkipWait(true)}>
							Começar já
						</Button>
					</div>
				)}
				{info && !playsHere && (
					<div className="flex max-w-lg flex-col items-center gap-3 p-6 text-center">
						<p>
							{videoError
								? "The built-in player can't decode this file (codec)."
								: info.remux_error
									? `Não deu para converter ${info.file_name}: ${info.remux_error}`
									: `${info.file_name} is not a format the built-in player supports.`}
						</p>
						{info.remux_error && (
							<p className="text-white/70">
								Fecha e escolhe outra fonte em “Fontes”, ou tenta no VLC.
							</p>
						)}
						<p className="text-white/70">
							It keeps streaming here — VLC plays it, with the chosen subtitle.
						</p>
						<Button onClick={vlc}>Open in VLC</Button>
					</div>
				)}
				{resumedAt > 0 && (
					<div className="absolute top-4 left-1/2 flex -translate-x-1/2 items-center gap-3 rounded-full bg-black/80 px-4 py-2 text-sm">
						<span>Retomado em {formatClock(resumedAt)}</span>
						<Button size="sm" variant="secondary" onClick={startOver}>
							⟲ Recomeçar
						</Button>
					</div>
				)}
				{countdown !== null && next && (
					<div className="absolute right-6 bottom-20 flex flex-col gap-2 rounded-xl bg-black/85 p-4 text-sm">
						<span className="text-white/70">A seguir, em {countdown}s</span>
						<span className="font-medium">{next.label}</span>
						<span className="flex gap-2">
							<Button
								size="sm"
								onClick={() => {
									setCountdown(null);
									next.start();
								}}
							>
								▶ Ver agora
							</Button>
							<Button size="sm" variant="secondary" onClick={() => setCountdown(null)}>
								Cancelar
							</Button>
						</span>
					</div>
				)}
			</div>
		</div>
	);
}
