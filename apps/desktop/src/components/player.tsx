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
	subtitleVtt,
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

	// subtitles: listed once the stream exists so the file hash can rank them
	useEffect(() => {
		if (!info) return;
		let cancelled = false;
		subtitlesList(request.imdbCode, request.season, request.episode)
			.then(list => {
				if (cancelled) return;
				setSubs(list);
				if (list[0]) setSubId(list[0].id);
			})
			.catch(() => {
				if (!cancelled) setSubs([]);
			});
		return () => {
			cancelled = true;
		};
	}, [info, request.imdbCode, request.season, request.episode]);

	// download the chosen subtitle
	useEffect(() => {
		if (!subs || !subId) return;
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

	// a new subtitle starts in sync
	useEffect(() => {
		setOffset(0);
	}, [vtt]);

	// (re)build cues whenever the text or the sync offset changes; remuxed
	// streams start at time_offset, so cues shift back by it
	useEffect(() => {
		applyCues(videoRef.current, trackRef, vtt, offset - (info?.remuxed ? info.time_offset : 0));
	}, [vtt, offset, info]);

	// live download stats
	useEffect(() => {
		if (!info) return;
		const id = setInterval(() => {
			streamStatus()
				.then(setStatus)
				.catch(() => {});
		}, 1000);
		return () => clearInterval(id);
	}, [info]);

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

			<div className="relative flex min-h-0 flex-1 items-center justify-center">
				{error && <p className="max-w-lg p-6 text-center text-red-400">{error}</p>}
				{!error && !info && (
					<p className="text-white/70">A ligar aos peers e a preparar o vídeo…</p>
				)}
				{info && playsHere && (
					// biome-ignore lint/a11y/useMediaCaption: captions come from the subtitle picker
					<video
						ref={videoRef}
						src={info.play_url ?? undefined}
						controls
						autoPlay
						className="h-full w-full"
						onDoubleClick={e => {
							e.preventDefault();
							toggleFullscreen();
						}}
						onError={() => setVideoError(true)}
						onLoadedMetadata={onLoadedMetadata}
						onEnded={onEnded}
					/>
				)}
				{info && !playsHere && (
					<div className="flex max-w-lg flex-col items-center gap-3 p-6 text-center">
						<p>
							{videoError
								? "The built-in player can't decode this file (codec)."
								: `${info.file_name} is not a format the built-in player supports.`}
						</p>
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
