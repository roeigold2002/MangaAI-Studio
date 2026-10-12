import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { ArrowRight, Compass, Download, Search } from "lucide-react";
import { api, errMessage } from "../lib/api";
import { toast, toastError, useApp, useLoad } from "../lib/store";
import { remoteUrl } from "../lib/util";
import { Empty, ErrorBox, Spinner } from "../components/ui";
import type { RemoteChapter, RemoteManga, SourceInfo } from "../lib/types";

export default function BrowsePage() {
  const settings = useApp((s) => s.settings)!;
  const status = useLoad(() => (settings.sources.suwayomi_enabled ? api.sourceStatus() : Promise.reject({ code: "sources_disabled", message: "" })), [settings.sources.suwayomi_enabled, settings.sources.suwayomi_url]);
  const [source, setSource] = useState<SourceInfo | null>(null);
  const [manga, setManga] = useState<RemoteManga | null>(null);

  if (!settings.sources.suwayomi_enabled) {
    return (
      <div className="page">
        <div className="page-head"><h1>עיון במקורות</h1></div>
        <Empty title="מקורות אונליין כבויים" icon={<Compass size={30} strokeWidth={1.5} />}
          action={<Link className="btn primary" to="/settings">להגדרות המקורות</Link>}>
          החיפוש וההורדה עובדים דרך Suwayomi-Server, שרת קוד פתוח שמריץ הרחבות מקורות של Mihon/Tachiyomi על המחשב שלך.
          מתקינים אותו, מוסיפים בו הרחבות, ומפעילים את החיבור בהגדרות.
        </Empty>
      </div>
    );
  }
  if (status.loading && !status.data) return <div className="page"><Spinner label="מתחבר ל-Suwayomi…" /></div>;
  if (status.error) {
    return (
      <div className="page">
        <div className="page-head"><h1>עיון במקורות</h1></div>
        <ErrorBox
          error={
            status.errorCode === "source_offline"
              ? "אין חיבור ל-Suwayomi. ודא שהשרת פועל ושהכתובת נכונה."
              : status.errorCode === "source_auth"
                ? "Suwayomi דחה את שם המשתמש או הסיסמה."
                : status.error
          }
          onRetry={status.reload}
        />
        <p className="muted small" style={{ marginTop: 10 }}>כתובת מוגדרת: <span className="ltr">{settings.sources.suwayomi_url}</span>. אפשר לשנות אותה ב<Link to="/settings" style={{ textDecoration: "underline" }}>הגדרות</Link>.</p>
      </div>
    );
  }
  if (manga) return <RemoteMangaView manga={manga} onBack={() => setManga(null)} />;
  if (source) return <SourceView source={source} onBack={() => setSource(null)} onOpen={setManga} />;
  return <SourceList server={status.data ?? ""} onPick={setSource} />;
}

function SourceList({ server, onPick }: { server: string; onPick: (s: SourceInfo) => void }) {
  const { data, error, reload } = useLoad(() => api.sources(), []);
  const [lang, setLang] = useState("all");
  const langs = useMemo(() => [...new Set((data ?? []).map((s) => s.lang))].sort(), [data]);
  const list = (data ?? []).filter((s) => lang === "all" || s.lang === lang);
  return (
    <div className="page">
      <div className="page-head">
        <div><h1>עיון במקורות</h1><p className="muted ltr" style={{ textAlign: "start" }}>{server}</p></div>
        <select className="select" style={{ width: 160 }} value={lang} onChange={(e) => setLang(e.target.value)} aria-label="שפה">
          <option value="all">כל השפות</option>
          {langs.map((l) => <option key={l} value={l}>{l}</option>)}
        </select>
      </div>
      {error && <ErrorBox error={error} onRetry={reload} />}
      {data && list.length === 0 && (
        <Empty title="אין מקורות מותקנים">הוסף מאגר הרחבות והתקן הרחבות בממשק של Suwayomi, ואז רענן כאן.</Empty>
      )}
      <div className="panel" style={{ padding: 0 }}>
        {list.map((s) => (
          <div key={s.id} className="list-row clickable" onClick={() => onPick(s)}>
            {s.icon_url ? <img src={remoteUrl(s.icon_url)} width={28} height={28} style={{ borderRadius: 6 }} alt="" /> : <Compass size={22} />}
            <span className="grow" dir="auto">{s.name}</span>
            {s.is_nsfw && <span className="badge warn">18+</span>}
            <span className="badge">{s.lang}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function SourceView({ source, onBack, onOpen }: { source: SourceInfo; onBack: () => void; onOpen: (m: RemoteManga) => void }) {
  const [kind, setKind] = useState<"popular" | "latest" | "search">("popular");
  const [q, setQ] = useState("");
  const [query, setQuery] = useState<string | null>(null);
  const [page, setPage] = useState(1);
  const [items, setItems] = useState<RemoteManga[]>([]);
  const [more, setMore] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setBusy(true);
    setError(null);
    api.browse(source.id, kind, kind === "search" ? query : null, page)
      .then((r) => {
        if (!alive) return;
        setItems((prev) => (page === 1 ? r.mangas : [...prev, ...r.mangas]));
        setMore(r.has_next_page);
      })
      .catch((e) => alive && setError(errMessage(e)))
      .finally(() => alive && setBusy(false));
    return () => { alive = false; };
  }, [source.id, kind, query, page]);

  return (
    <div className="page">
      <div className="page-head">
        <div className="row">
          <button className="btn icon ghost" onClick={onBack} aria-label="חזרה"><ArrowRight size={18} /></button>
          <h1 dir="auto">{source.name}</h1>
        </div>
        <form className="row" onSubmit={(e) => { e.preventDefault(); setKind("search"); setQuery(q); setPage(1); }}>
          <div className="search" style={{ width: 260 }}>
            <Search size={16} />
            <input className="input" placeholder="חיפוש במקור" value={q} onChange={(e) => setQ(e.target.value)} dir="auto" />
          </div>
          <div className="seg">
            <button type="button" className={kind === "popular" ? "on" : ""} onClick={() => { setKind("popular"); setPage(1); }}>פופולרי</button>
            {source.supports_latest && <button type="button" className={kind === "latest" ? "on" : ""} onClick={() => { setKind("latest"); setPage(1); }}>עדכונים אחרונים</button>}
          </div>
        </form>
      </div>
      {error && <ErrorBox error={error} />}
      {!busy && !error && items.length === 0 && <Empty title="אין תוצאות" />}
      <div className="covers">
        {items.map((m) => (
          <div key={m.manga_ref} className="cover-card" onClick={() => onOpen(m)}>
            <div className="cover">
              {m.thumbnail_url ? <img src={remoteUrl(m.thumbnail_url)} loading="lazy" alt="" /> : <div className="fallback">{m.title}</div>}
            </div>
            <div className="cover-title" dir="auto">{m.title}</div>
          </div>
        ))}
      </div>
      <div className="row" style={{ justifyContent: "center", marginTop: 24 }}>
        {busy ? <Spinner label="טוען…" /> : more && <button className="btn" onClick={() => setPage(page + 1)}>עוד תוצאות</button>}
      </div>
    </div>
  );
}

function RemoteMangaView({ manga, onBack }: { manga: RemoteManga; onBack: () => void }) {
  const nav = useNavigate();
  const { data, error, reload } = useLoad(() => api.remoteManga(manga.manga_ref), [manga.manga_ref]);
  const [sel, setSel] = useState<Set<string>>(new Set());

  async function download(chapters: RemoteChapter[]) {
    if (!data) return;
    try {
      const r = await api.enqueueDownloads(data.manga, chapters);
      toast(`${r.added} פרקים נוספו להורדה${r.skipped ? `, ${r.skipped} כבר קיימים` : ""}`, "ok");
      setSel(new Set());
      reload();
    } catch (e) {
      toastError(e);
    }
  }

  const m = data?.manga ?? manga;
  return (
    <div className="page">
      <div className="row" style={{ marginBottom: 16 }}>
        <button className="btn icon ghost" onClick={onBack} aria-label="חזרה"><ArrowRight size={18} /></button>
      </div>
      <div className="detail-hero">
        <div className="cover">{m.thumbnail_url ? <img src={remoteUrl(m.thumbnail_url)} alt="" /> : <div className="fallback">{m.title}</div>}</div>
        <div className="col">
          <h1 dir="auto">{m.title}</h1>
          <div className="meta-line">{m.author && <span dir="auto">{m.author}</span>}{m.status && <span>{m.status}</span>}</div>
          {m.description && <p className="muted" dir="auto" style={{ maxWidth: 760 }}>{m.description}</p>}
          <div className="row wrap">
            {data && <button className="btn primary" onClick={() => download(data.chapters)}><Download size={16} /> הורדת כל הפרקים</button>}
            {sel.size > 0 && data && <button className="btn" onClick={() => download(data.chapters.filter((c) => sel.has(c.chapter_ref)))}>הורדת {sel.size} נבחרים</button>}
            {data?.local_manga_id && <button className="btn ghost" onClick={() => nav(`/manga/${data.local_manga_id}`)}>בספרייה</button>}
          </div>
          <p className="small faint">ההורדה מתבצעת דרך ההרחבה שמותקנת ב-Suwayomi. האחריות לעמידה בתנאי המקור ובחוק היא של המשתמש.</p>
        </div>
      </div>
      <div className="section">
        {error && <ErrorBox error={error} onRetry={reload} />}
        {!data && !error && <Spinner label="טוען פרקים…" />}
        {data && (
          <div className="panel" style={{ padding: 0 }}>
            {data.chapters.map((c) => {
              const st = data.downloads[c.chapter_ref];
              return (
                <label key={c.chapter_ref} className="list-row clickable">
                  <input type="checkbox" checked={sel.has(c.chapter_ref)} disabled={st === "done"} style={{ accentColor: "var(--marker)" }}
                    onChange={(e) => { const s = new Set(sel); if (e.target.checked) s.add(c.chapter_ref); else s.delete(c.chapter_ref); setSel(s); }} />
                  <span className="grow" dir="auto">{c.title}</span>
                  {c.scanlator && <span className="faint small" dir="auto">{c.scanlator}</span>}
                  {c.upload_date ? <span className="faint small">{new Date(c.upload_date).toLocaleDateString("he-IL")}</span> : null}
                  {st && <span className={`badge ${st === "done" ? "ok" : st === "failed" ? "bad" : "run"}`}>{st === "done" ? "הורד" : st === "failed" ? "נכשל" : "בהורדה"}</span>}
                </label>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
