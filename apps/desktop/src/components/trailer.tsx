import { Button } from "@popcorntime/ui/components/button";
import { useState } from "react";
import { trailerUrl } from "@/tauri/yts";

/**
 * Trailer inside the movie/series window. YouTube refuses embeds from the
 * app's tauri:// origin (Error 153), so the backend serves a tiny embed page
 * on 127.0.0.1 and this iframes it.
 */
export function Trailer({ youtubeId }: { youtubeId?: string | null }) {
	const [src, setSrc] = useState<string | null>(null);
	const [error, setError] = useState(false);
	if (!youtubeId) return null;
	if (!src) {
		return (
			<span className="flex items-center gap-2">
				<Button
					size="sm"
					variant="outline"
					onClick={() => {
						setError(false);
						trailerUrl(youtubeId)
							.then(setSrc)
							.catch(() => setError(true));
					}}
				>
					🎬 Trailer
				</Button>
				{error && <span className="text-xs text-red-500">Trailer indisponível</span>}
			</span>
		);
	}
	return (
		<div className="flex w-full flex-col gap-1">
			<iframe
				title="Trailer"
				src={src}
				className="aspect-video w-full rounded-lg bg-black"
				allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
				allowFullScreen
			/>
			<Button size="sm" variant="ghost" className="self-end" onClick={() => setSrc(null)}>
				Fechar trailer
			</Button>
		</div>
	);
}
