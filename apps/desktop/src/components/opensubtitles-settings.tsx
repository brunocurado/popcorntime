import { Button } from "@popcorntime/ui/components/button";
import { Input } from "@popcorntime/ui/components/input";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

interface Status {
	configured: boolean;
	username: string | null;
	remaining: number | null;
}

function errorText(e: unknown) {
	if (e instanceof Error) return e.message;
	if (e && typeof e === "object" && "message" in e)
		return String((e as { message: unknown }).message);
	return String(e);
}

/**
 * OpenSubtitles.com account, used only when the free source has no
 * Portuguese subtitle. Values go straight to the macOS Keychain (checked
 * against OpenSubtitles first) and are never shown again.
 */
export function OpenSubtitlesSettings() {
	const [status, setStatus] = useState<Status | null>(null);
	const [apiKey, setApiKey] = useState("");
	const [username, setUsername] = useState("");
	const [password, setPassword] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		invoke<Status>("opensubtitles_status")
			.then(s => setStatus(s ?? { configured: false, username: null, remaining: null }))
			.catch(() => setStatus({ configured: false, username: null, remaining: null }));
	}, []);

	const save = useCallback(async () => {
		setBusy(true);
		setError(null);
		try {
			const s = await invoke<Status>("opensubtitles_save", {
				apiKey,
				username: username.trim() || null,
				password: password || null,
			});
			setStatus(s);
			setApiKey("");
			setPassword("");
		} catch (e) {
			setError(errorText(e));
		} finally {
			setBusy(false);
		}
	}, [apiKey, username, password]);

	const clear = useCallback(async () => {
		await invoke("opensubtitles_clear").catch(() => {});
		setStatus({ configured: false, username: null, remaining: null });
	}, []);

	return (
		<section
			aria-label="OpenSubtitles"
			className="flex flex-col gap-2 rounded-lg border p-3 shadow-sm"
		>
			<div className="space-y-0.5">
				<div>Legendas · OpenSubtitles.com</div>
				<p className="text-muted-foreground text-sm">
					Reserva para quando a fonte grátis não tem legenda em português (filmes muito recentes).
					Fica guardado no Porta-chaves do Mac.
				</p>
			</div>
			{status?.configured ? (
				<div className="flex flex-wrap items-center gap-2 text-sm">
					<span>
						✓ Ligado{status.username ? ` como ${status.username}` : " (só chave de API)"}
						{status.remaining != null && ` · ${status.remaining} downloads hoje`}
					</span>
					<Button
						type="button"
						size="sm"
						variant="outline"
						className="ml-auto"
						onClick={() => void clear()}
					>
						Desligar
					</Button>
				</div>
			) : (
				<div className="flex flex-col gap-2">
					<Input
						aria-label="Chave de API"
						placeholder="Chave de API (opensubtitles.com → API Consumers)"
						value={apiKey}
						autoComplete="off"
						onChange={e => setApiKey(e.target.value)}
					/>
					<Input
						aria-label="Utilizador"
						placeholder="Utilizador (opcional, dá 20 downloads/dia)"
						value={username}
						autoComplete="username"
						onChange={e => setUsername(e.target.value)}
					/>
					<Input
						aria-label="Password"
						type="password"
						placeholder="Password"
						value={password}
						autoComplete="current-password"
						onChange={e => setPassword(e.target.value)}
					/>
					<Button
						type="button"
						size="sm"
						className="self-end"
						disabled={busy || !apiKey.trim() || !!username.trim() !== !!password}
						onClick={() => void save()}
					>
						{busy ? "A verificar…" : "Verificar e guardar"}
					</Button>
				</div>
			)}
			{error && <p className="text-sm text-red-500">{error}</p>}
		</section>
	);
}
