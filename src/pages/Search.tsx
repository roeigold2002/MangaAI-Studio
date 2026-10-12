import { useEffect, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { Search as SearchIcon, X } from "lucide-react";
import { api, errCode, errMessage } from "../lib/api";
import { useLoad } from "../lib/store";
import { remoteUrl } from "../lib/util";
import { Empty, Spinner } from "../components/ui";
import { SourceGate, sourceState } from "../components/SourceGate";
import type { SearchGroup, SearchOutcome } from "../lib/types";

let generation = 0;

export default function SearchPage() {
  const [params, setParams] = useSearchParams();
  const [q, setQ] = useState(params.get("q") ?? "");
  const [result, setResult] = useState<SearchOutcome | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ code: string | null; message: string } | null>(null);
  const overview = useLoad(() => api.sourceOverview(), []);
  const state = sourceState(overview.data);
  const canSearch = state === "ready" || state === "local_only";
  const inputRef = useRef<HTMLInputElement>(null);
  const [nonce, setNonce] = useState(0);

  // debounced search; a newer query cancels the older one (frontend and backend)
  useEffect(() => {
    const query = q.trim();
    setParams(query ? { q: query } : {}, { replace: true });
    if (!query || !canSearch) {
      setResult(null);
      setBusy(false);
      return;
    }
    const gen = ++generation;
    setBusy(true);
    setError(null);
    const t = window.setTimeout(async () => {
      try {
        const r = await api.searchAll(query, gen);
        if (gen === generation && !r.cancelled) setResult(r);
      } catch (e) {
        if (gen === generation) setError({ code: errCode(e), message: errMessage(e) });
      } finally {
        if (gen === generation) setBusy(false);
      }
    }, 450);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [q, canSearch, nonce]);

  useEffect(() => inputRef.current?.focus(), []);

  const retry = () => setNonce((n) => n + 1);

  return (
    <div className="page">
      <div className="page-head"><h1>חיפוש מנגה</h1></div>
      <div className="search hero-search">
        <SearchIcon size={20} />
        <input ref={inputRef} className="input" value={q} onChange={(e) => setQ(e.target.value)} dir="auto"
          placeholder="שם המנגה, למשל One Piece או 葬送のフリーレン" aria-label="חיפוש מנגה" />
        {q && <button className="btn ghost icon sm clear" onClick={() => setQ("")} aria-label="ניקוי"><X size={16} /></button>}
      </div>

      {!overview.loading && !canSearch && <div style={{ marginTop: 18 }}><SourceGate overview={overview.data} onChange={overview.reload} /></div>}
      {state === "local_only" && q.trim() && (
        <p className="small faint" style={{ marginTop: 10 }}>
          אין מקורות אונליין מותקנים, ולכן החיפוש כולל רק את התיקייה המקומית של מנוע המקורות. <a href="#/settings?tab=sources" style={{ textDecoration: "underline" }}>הוספת מקורות</a>
        </p>
      )}

      <div style={{ marginTop: 22 }}>
        {busy && <Spinner label="מחפש בכל המקורות…" />}
        {error && (
          <div className="callout bad col">
            <span dir="auto">{error.code === "no_sources" ? "אין מקורות מותקנים במנוע המקורות." : error.message}</span>
            <div><button className="btn sm" onClick={retry}>חיפוש חוזר</button></div>
          </div>
        )}
        {!busy && result && result.groups.length === 0 && (
          <Empty title="לא נמצאו תוצאות">
            חיפשנו ב-{result.sources_searched} מקורות ולא נמצאה מנגה בשם הזה. נסה כתיב אחר, שם באנגלית או את השם המקורי.
          </Empty>
        )}
        {result && result.failures.length > 0 && (
          <div className="callout small" style={{ marginBottom: 16 }}>
            {result.failures.length} מקורות לא ענו: {result.failures.map((f) => `${f.source_name} (${f.message})`).join(" · ")}
            <button className="btn ghost sm" onClick={retry} style={{ marginInlineStart: 8 }}>נסה שוב</button>
          </div>
        )}
        {result && result.groups.length > 0 && (
          <>
            <p className="muted small" style={{ marginBottom: 14 }}>{result.groups.length} תוצאות מ-{result.sources_searched} מקורות</p>
            <ResultGrid groups={result.groups} />
          </>
        )}
        {!q.trim() && canSearch && <Empty title="מה בא לך לקרוא?">הקלד שם של מנגה. החיפוש רץ בכל המקורות המותקנים בבת אחת.</Empty>}
      </div>
    </div>
  );
}

function ResultGrid({ groups }: { groups: SearchGroup[] }) {
  const nav = useNavigate();
  const [counts, setCounts] = useState<Record<string, number | "err">>({});
  // chapter counts for the first results, two at a time (each one asks the source)
  useEffect(() => {
    let alive = true;
    const refs = groups.slice(0, 12).map((g) => g.entries[0].manga.manga_ref);
    let i = 0;
    const worker = async () => {
      while (alive && i < refs.length) {
        const ref = refs[i++];
        try {
          const n = await api.remoteChapterCount(ref);
          if (alive) setCounts((c) => ({ ...c, [ref]: n }));
        } catch {
          if (alive) setCounts((c) => ({ ...c, [ref]: "err" }));
        }
      }
    };
    worker();
    worker();
    return () => { alive = false; };
  }, [groups]);

  return (
    <div className="covers">
      {groups.map((g, i) => {
        const first = g.entries[0];
        const n = counts[first.manga.manga_ref];
        return (
          <div key={`${first.source_id}-${first.manga.manga_ref}-${i}`} className="cover-card" role="link" tabIndex={0}
            onClick={() => nav(`/title/${encodeURIComponent(first.manga.manga_ref)}`, { state: { group: g } })}
            onKeyDown={(e) => e.key === "Enter" && nav(`/title/${encodeURIComponent(first.manga.manga_ref)}`, { state: { group: g } })}>
            <div className="cover">
              {g.thumbnail_url ? <img src={remoteUrl(g.thumbnail_url)} loading="lazy" alt="" /> : <div className="fallback tone">{g.title}</div>}
              <div className="flags">
                {g.entries.length > 1 && <span className="badge">{g.entries.length} מקורות</span>}
              </div>
            </div>
            <div>
              <div className="cover-title" dir="auto">{g.title}</div>
              <div className="cover-sub" dir="auto">
                {typeof n === "number" ? `${n} פרקים` : n === "err" ? "" : i < 12 ? "סופר פרקים…" : ""}
              </div>
              <div className="cover-sub" dir="auto">{g.entries.map((e) => e.source_name).join(", ")}</div>
              {g.description && <div className="cover-desc" dir="auto">{g.description}</div>}
            </div>
          </div>
        );
      })}
    </div>
  );
}
