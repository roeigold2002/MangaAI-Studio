import { convertFileSrc } from "@tauri-apps/api/core";

export type Platform = "mac" | "windows" | "linux";
export function detectPlatform(ua: string): Platform {
  if (/Windows/i.test(ua)) return "windows";
  if (/Mac OS X|Macintosh/i.test(ua)) return "mac";
  return "linux";
}
export const platform: Platform = detectPlatform(typeof navigator === "undefined" ? "" : navigator.userAgent);

/** Where API keys and passwords are kept on this OS, in the UI's words. */
export function credentialStoreName(p: Platform = platform): string {
  if (p === "windows") return "מנהל האישורים של Windows";
  if (p === "mac") return "ה-Keychain של macOS";
  return "מאגר הסיסמאות של המערכת";
}

// Base of the mangaimg:// protocol for this platform (mangaimg://localhost/ on macOS,
// http://mangaimg.localhost/ on Windows).
let base: string | null = null;
export function imgUrl(path: string): string {
  if (base === null) base = convertFileSrc("", "mangaimg");
  return base.replace(/\/?$/, "/") + path.replace(/^\//, "");
}

export const pageUrl = (id: number) => imgUrl(`page/${id}`);
export const translatedUrl = (id: number, v: string | null) => imgUrl(`page/${id}/translated?v=${v ?? ""}`);
export const previousUrl = (id: number) => imgUrl(`page/${id}/previous?t=${Date.now()}`);
export const cleanUrl = (id: number, v: string | null) => imgUrl(`page/${id}/clean?v=${v ?? ""}`);
export const thumbUrl = (id: number) => imgUrl(`thumb/page/${id}`);
export const coverUrl = (mangaId: number, v?: string) => imgUrl(`cover/${mangaId}?v=${v ?? ""}`);
export const remoteUrl = (url: string) => imgUrl(`remote/${hex(url)}`);

function hex(s: string): string {
  return Array.from(new TextEncoder().encode(s), (b) => b.toString(16).padStart(2, "0")).join("");
}

const rtf = new Intl.RelativeTimeFormat("he", { numeric: "auto" });
export function ago(iso: string | null | undefined): string {
  if (!iso) return "";
  const diff = (new Date(iso).getTime() - Date.now()) / 1000;
  const abs = Math.abs(diff);
  if (abs < 60) return rtf.format(Math.round(diff), "second");
  if (abs < 3600) return rtf.format(Math.round(diff / 60), "minute");
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), "hour");
  if (abs < 86400 * 30) return rtf.format(Math.round(diff / 86400), "day");
  return new Date(iso).toLocaleDateString("he-IL");
}

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export function usd(n: number | null | undefined): string {
  if (n === null || n === undefined) return "לא ידוע";
  if (n === 0) return "$0";
  if (n < 0.01) return "פחות מ-$0.01";
  return `$${n.toFixed(2)}`;
}

/** Visual page order for a two-page spread. Wide pages always stand alone. */
export function spreads(pages: { width: number; height: number }[], coverAlone: boolean): number[][] {
  const out: number[][] = [];
  let i = 0;
  if (coverAlone && pages.length > 0) {
    out.push([0]);
    i = 1;
  }
  while (i < pages.length) {
    const wide = (p: { width: number; height: number }) => p.width > p.height * 1.1;
    if (wide(pages[i]) || i + 1 >= pages.length || wide(pages[i + 1])) {
      out.push([i]);
      i += 1;
    } else {
      out.push([i, i + 1]);
      i += 2;
    }
  }
  return out;
}

/** Arrow key → step, depending on page direction (RTL: left arrow goes forward). */
export function arrowStep(key: string, dir: "rtl" | "ltr"): number {
  if (key === "ArrowLeft") return dir === "rtl" ? 1 : -1;
  if (key === "ArrowRight") return dir === "rtl" ? -1 : 1;
  return 0;
}

export const statusLabel: Record<string, string> = {
  none: "לא תורגם",
  analyzed: "זוהה טקסט",
  translated: "תורגם, ממתין לעימוד",
  done: "תורגם",
  failed: "נכשל",
  running: "בעבודה",
  queued: "בתור",
  paused: "מושהה",
  cancelled: "בוטל",
};

/** Publication status from Suwayomi/Mihon ("ONGOING", …) in Hebrew; unknown → "". */
export function publicationStatus(s: string | null | undefined): string {
  return ({ ONGOING: "בהמשכים", COMPLETED: "הסתיים", CANCELLED: "בוטל", ON_HIATUS: "בהפסקה", LICENSED: "ברישיון", PUBLISHING_FINISHED: "הסתיים" } as Record<string, string>)[s ?? ""] ?? "";
}

export const LANG_NAMES: Record<string, string> = {
  ja: "יפנית", en: "אנגלית", ko: "קוריאנית", zh: "סינית", "zh-Hans": "סינית מפושטת", "zh-Hant": "סינית מסורתית",
  he: "עברית", es: "ספרדית", "es-419": "ספרדית (אמ״ל)", fr: "צרפתית", de: "גרמנית", it: "איטלקית", pt: "פורטוגזית",
  "pt-BR": "פורטוגזית (ברזיל)", ru: "רוסית", ar: "ערבית", id: "אינדונזית", vi: "וייטנאמית", th: "תאית", tr: "טורקית", pl: "פולנית",
  all: "כל השפות",
};
export const langName = (code: string) => LANG_NAMES[code] ?? code;

export const jobLabel: Record<string, string> = {
  queued: "בתור", running: "בעבודה", paused: "מושהה", done: "הושלם", failed: "הסתיים עם שגיאות", cancelled: "בוטל", skipped: "דולג",
};

/** Strip a prefix shared by all chapter titles ("Series Name v01", "Series Name v02" → "v01", "v02"). */
export function shortTitles(titles: string[]): string[] {
  if (titles.length < 2) return titles;
  let p = titles[0];
  for (const t of titles) while (p && !t.startsWith(p)) p = p.slice(0, -1);
  const cut = Math.max(p.lastIndexOf(" "), p.lastIndexOf("_"), p.lastIndexOf("·"), p.lastIndexOf("-"));
  if (cut < 4) return titles;
  const out = titles.map((t) => t.slice(cut + 1).trim());
  return out.some((t) => !t) ? titles : out;
}

export function chapterTranslationBadge(t: { total: number; done: number; failed: number; in_progress: number }) {
  if (t.total > 0 && t.done === t.total) return { cls: "ok", text: "מתורגם" };
  if (t.failed > 0) return { cls: "bad", text: `${t.failed} נכשלו` };
  if (t.done > 0 || t.in_progress > 0) return { cls: "run", text: `${t.done}/${t.total}` };
  return null;
}

/** Versions of the same chapter (one per scanlation group) share this key; 104 and 104.5 differ. */
export function chapterKey(c: { number: number | null; title: string }): string {
  return c.number != null ? `n:${Math.round(c.number * 1000)}` : `t:${c.title.trim().toLowerCase()}`;
}

/** Chapters numbered from..to (inclusive, with to's decimals: 1–3 includes 3.5). Unnumbered sources use list positions. */
export function chaptersInRange<T extends { number: number | null }>(chapters: T[], from: number, to: number): T[] {
  if (!chapters.some((c) => c.number != null)) return chapters.slice(from - 1, to);
  return chapters.filter((c) => c.number != null && c.number >= from && c.number < to + 1);
}

/** Distinct chapters vs. listed versions, chapters not held in any version, and the scanlation groups. */
export function chapterStats(chapters: { number: number | null; title: string; chapter_ref: string; scanlator: string | null }[], downloads: Record<string, string>) {
  const held = new Set<string>();
  const all = new Set<string>();
  const groups = new Map<string, number>();
  for (const c of chapters) {
    const k = chapterKey(c);
    all.add(k);
    if (["done", "queued", "running"].includes(downloads[c.chapter_ref])) held.add(k);
    const g = c.scanlator?.trim();
    if (g) groups.set(g, (groups.get(g) ?? 0) + 1);
  }
  return {
    distinct: all.size,
    versions: chapters.length,
    missing: [...all].filter((k) => !held.has(k)).length,
    groups: [...groups.entries()].sort((a, b) => b[1] - a[1]),
  };
}
