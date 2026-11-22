import { Button } from "@popcorntime/ui/components/button";
import { useCallback, useEffect, useRef, useState } from "react";
import {
	openInVlc,
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
	magnet: string;
	title: string;
	imdbCode: string;
}

const OFFSET_STEP = 0.5;

function mb(bytes: number) {
	return `${(bytes / 1024 / 1024).toFixed(0)} MB`;
}

/** Shift every cue of a text track by `delta` seconds. */
function shiftCues(track: TextTrack | undefined, delta: number) {
	if (!track?.cues || delta === 0) return;
	for (const cue of Array.from(track.cues)) {
		cue.startTime = Math.max(0, cue.startTime + delta);
		cue.endTime = Math.max(0, cue.endTime + delta);
	}
}

/**
 * Full-window player: streams the torrent through the local engine, plays
 * mp4/m4v/mov/webm in a <video>, offers VLC for the rest, and overlays
 * OpenSubtitles tracks (with +/- sync and drag-and-drop .srt).
 */
export function Player({ request, onClose }: { request: PlayRequest; onClose: () => void }) {
	const videoRef = useRef<HTMLVideoElement>(null);
	const [info, setInfo] = useState<StreamInfo | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [videoError, setVideoError] = useState(false);
	const [status, setStatus] = useState<StreamStatus | null>(null);
	const [subs, setSubs] = useState<SubtitleTrack[] | null>(null);
	const [subId, setSubId] = useState<string>("");
	const [vtt, setVtt] = useState<string | null>(null);
	const [vttUrl, setVttUrl] = useState<string | null>(null);
	const [offset, setOffset] = useState(0);
	const [subError, setSubError] = useState<string | null>(null);

	// start streaming; always drop the torrent (and its cache) on close
	useEffect(() => {
		let cancelled = false;
		streamPlay(request.magnet)
			.then(res => {
				if (!cancelled) setInfo(res);
			})
			.catch(e => {
				if (!cancelled) setError(e instanceof Error ? e.message : String(e?.message ?? e));
			});
		return () => {
			cancelled = true;
			void streamStop();
		};
	}, [request.magnet]);

	// subtitles: listed once the stream exists so the file hash can rank them
	useEffect(() => {
		if (!info) return;
		let cancelled = false;
		subtitlesList(request.imdbCode)
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
	}, [info, request.imdbCode]);

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

	// turn VTT text into a <track> source
	useEffect(() => {
		if (!vtt) {
			setVttUrl(null);
			return;
		}
		const url = URL.createObjectURL(new Blob([vtt], { type: "text/vtt" }));
		setVttUrl(url);
		setOffset(0);
		return () => URL.revokeObjectURL(url);
	}, [vtt]);

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

	const nudge = useCallback((delta: number) => {
		shiftCues(videoRef.current?.textTracks[0], delta);
		setOffset(o => Math.round((o + delta) * 10) / 10);
	}, []);

	const showTrack = useCallback(() => {
		const track = videoRef.current?.textTracks[0];
		if (track) track.mode = "showing";
	}, []);

	const vlc = useCallback(() => {
		if (!info) return;
		openInVlc(info.url, vtt ?? undefined).catch(e =>
			setError(e instanceof Error ? e.message : String(e?.message ?? e))
		);
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

	const playsHere = info?.in_app && !videoError;
	const pct =
		status && status.total_bytes > 0 ? (status.progress_bytes / status.total_bytes) * 100 : 0;

	return (
		<div
			role="dialog"
			aria-label={`Playing ${request.title}`}
			className="fixed inset-0 z-[100] flex flex-col bg-black text-white"
			onDragOver={e => e.preventDefault()}
			onDrop={onDrop}
		>
			<div className="flex flex-wrap items-center gap-2 p-3 text-sm">
				<Button size="sm" variant="secondary" onClick={onClose}>
					✕ Close
				</Button>
				<span className="mr-auto truncate font-medium">{request.title}</span>
				{status && (
					<span className="text-white/70">
						{status.peers} peers · {(status.download_mbps / 8).toFixed(1)} MB/s ·{" "}
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
				{info && (
					<Button size="sm" variant="secondary" onClick={vlc}>
						Open in VLC
					</Button>
				)}
			</div>
			{subError && <p className="px-3 text-xs text-red-400">{subError}</p>}

			<div className="flex min-h-0 flex-1 items-center justify-center">
				{error && <p className="max-w-lg p-6 text-center text-red-400">{error}</p>}
				{!error && !info && <p className="text-white/70">Finding peers and reading the torrent…</p>}
				{info && playsHere && (
					// biome-ignore lint/a11y/useMediaCaption: captions come from the subtitle picker
					<video
						ref={videoRef}
						src={info.url}
						controls
						autoPlay
						className="h-full w-full"
						onError={() => setVideoError(true)}
						onLoadedMetadata={showTrack}
					>
						{vttUrl && (
							<track key={vttUrl} kind="subtitles" src={vttUrl} default onLoad={showTrack} />
						)}
					</video>
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
			</div>
		</div>
	);
}
