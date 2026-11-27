import { describe, expect, it } from "vitest";
import { type Candidate, parseSize, pickBest, rankCandidates, scoreCandidate } from "@/lib/pick";

const c = (over: Partial<Candidate>): Candidate => ({
	hash: Math.random().toString(16).slice(2).padEnd(40, "0"),
	title: "Movie 2020 1080p WEB x264",
	quality: "1080p",
	seeds: 50,
	size: "2.1 GB",
	source: "Torrentio · 1337x",
	...over,
});

describe("parseSize", () => {
	it("reads the units the sources print", () => {
		expect(parseSize("1.85 GB")).toBeCloseTo(1.85 * 1024 ** 3);
		expect(parseSize("700 MB")).toBe(700 * 1024 ** 2);
		expect(parseSize("5,6 GiB")).toBeCloseTo(5.6 * 1024 ** 3);
		expect(parseSize("?")).toBeNull();
	});
});

describe("pickBest", () => {
	it("never auto-picks cams, remuxes, 3D, >8 GB files or dead torrents", () => {
		expect(scoreCandidate(c({ title: "Movie 2020 HDCAM x264" }))).toBeNull();
		expect(scoreCandidate(c({ title: "Movie 2020 1080p BluRay REMUX AVC" }))).toBeNull();
		expect(scoreCandidate(c({ quality: "3D", title: "Movie 3D" }))).toBeNull();
		expect(scoreCandidate(c({ size: "35.09 GB" }))).toBeNull();
		expect(scoreCandidate(c({ seeds: 0 }))).toBeNull();
	});

	it("prefers a healthy 1080p over a huge-seeded 4K and a Dolby-Vision-only file", () => {
		const best = pickBest([
			c({ hash: "a", quality: "4k", title: "Movie 2160p WEB HEVC", size: "7.9 GB", seeds: 400 }),
			c({ hash: "b", title: "Movie 1080p WEB DV HEVC", seeds: 300 }),
			c({ hash: "c", title: "Movie 1080p WEB x264", seeds: 120 }),
			c({ hash: "d", quality: "720p", title: "Movie 720p WEB x264", seeds: 150 }),
		]);
		expect(best?.hash).toBe("c");
	});

	it("gives YTS mp4s the edge at similar health", () => {
		const best = pickBest([
			c({ hash: "x", source: "Torrentio · RARBG", seeds: 90 }),
			c({ hash: "y", source: "YTS", seeds: 80, title: "Movie 1080p bluray x264" }),
		]);
		expect(best?.hash).toBe("y");
	});

	it("for episodes keeps only files that are that episode", () => {
		const ep = { season: 1, episode: 2 };
		const ranked = rankCandidates(
			[
				c({ hash: "pack-nohint", title: "Show S01 1080p", seeds: 500 }),
				c({
					hash: "pack-hint",
					title: "Show S01 1080p",
					fileName: "Show.S01E02.1080p.mkv",
					fileIdx: 1,
					seeds: 200,
				}),
				c({ hash: "single", title: "Show S01E02 1080p WEB x264", seeds: 100 }),
				c({ hash: "other", title: "Show S01E20 1080p WEB x264", seeds: 900 }),
			],
			ep
		);
		expect(ranked.map(r => r.hash)).not.toContain("pack-nohint");
		expect(ranked.map(r => r.hash)).not.toContain("other");
		expect(ranked[0]?.hash).toBe("pack-hint");
	});

	it("returns null when nothing qualifies", () => {
		expect(pickBest([c({ seeds: 0 })])).toBeNull();
	});
});
