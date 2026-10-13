import { useMemo, useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router-dom";
import { ArrowRight, BookOpen, Download, Library, Play } from "lucide-react";
import { api } from "../lib/api";
import { toast, toastError, useEvent, useLoad, useThrottled } from "../lib/store";
import { bytes, langName, publicationStatus, remoteUrl } from "../lib/util";
import { ErrorBox, Modal, Spinner } from "../components/ui";
import type { RemoteChapter, SearchGroup } from "../lib/types";

/** Details of a search result: chapters, download status, and the download actions. */
export default function TitlePage() {
  const mangaRef = decodeURIComponent(useParams().ref ?? "");
  const nav = useNavigate();
  const group = (useLocation().state as { group?: SearchGroup } | null)?.group;
  const { data, error, reload } = useLoad(() => api.remoteManga(mangaRef), [mangaRef]);
  const local = useLoad(() => (data?.local_manga_id ? api.mangaDetail(data.local_manga_id) : Promise.resolve(null)), [data?.local_manga_id]);
  const hint = useLoad(() => api.downloadSizeHint(), []);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [range, setRange] = useState({ from: "", to: "" });
  const [confirm, setConfirm] = useState<RemoteChapter[] | null>(null);
  const [waitingFor, setWaitingFor] = useState<number | null>(null);
  const refresh = useThrottled(() => { reload(); local.reload(); }, 600);

  useEvent<{ id: number; status: string; chapter_id: number | null; error: string | null }>("download-updated", (d) => {
    refresh();
    if (waitingFor === d.id && d.status === "done" && d.chapter_id) {
      setWaitingFor(null);
      nav(`/read/${d.chapter_id}?page=0`);
    }
    if (waitingFor === d.id && d.status === "failed") {
      setWaitingFor(null);
      toast(`ההורדה נכשלה: ${d.error ?? ""}`, "bad");
    }
  });

  const chapters = data?.chapters ?? [];
  const pending = useMemo(() => chapters.filter((c) => data?.downloads[c.chapter_ref] !== "done"), [chapters, data]);
  const m = data?.manga ?? group?.entries[0].manga;

  async function download(list: RemoteChapter[]) {
    if (!data || !list.length) return;
    try {
      const r = await api.enqueueDownloads(data.manga, list);
      toast(r.added ? `${r.added} פרקים נכנסו להורדה. הם יופיעו ב"הספרייה שלי" כשיסתיימו.` : "הפרקים האלה כבר הורדו או בהורדה", "ok");
      setSel(new Set());
      reload();
    } catch (e) {
      toastError(e);
    }
  }

  function askAll(list: RemoteChapter[]) {
    if (list.length > 20) setConfirm(list);
    else download(list);
  }

  async function readNow() {
    if (!data || !chapters.length) return;
    const first = chapters.find((c) => data.local_chapters[c.chapter_ref]) ?? chapters[0];
    const localId = data.local_chapters[first.chapter_ref];
    if (localId) return nav(`/read/${localId}?page=0`);
    try {
      const id = await api.downloadNow(data.manga, first);
      setWaitingFor(id);
    } catch (e) {
      toastError(e);
    }
  }

  function downloadRange() {
    const from = Number(range.from), to = Number(range.to);
    if (!from || !to || from > to) return toast("בחר טווח תקין: מפרק מספר … עד פרק מספר …", "bad");
    const list = chapters.slice(from - 1, to).filter((c) => data?.downloads[c.chapter_ref] !== "done");
    askAll(list);
  }

  const doneCount = chapters.length - pending.length;
  const localManga = local.data;
  const translatedPages = localManga?.chapters.reduce((s, c) => s + c.translation.done, 0) ?? 0;

  return (
    <div className="page">
      <div className="row" style={{ marginBottom: 14 }}>
        <button className="btn ghost" onClick={() => nav(-1)}><ArrowRight size={16} /> חזרה לתוצאות</button>
      </div>
      {error && /no chapters/i.test(error) ? (
        <div className="callout col" style={{ gap: 8 }}>
          <span>
            אין במקור הזה פרקים שאפשר להוריד לכותר הזה. ב-MangaDex זה קורה בעיקר בסדרות עם רישיון רשמי (למשל Dragon Ball):
            הפרקים הוסרו או מפנים לאתר של המו״ל, ולכן אין עמודים להורדה. לפעמים יש פרקים בשפה אחרת (כל שפה היא מקור נפרד),
            ולפעמים צריך תוסף מקור אחר מחנות התוספים.
          </span>
          {group && group.entries.length > 1 && (
            <div className="row wrap">
              <span className="small muted">לנסות בשפה אחרת:</span>
              {group.entries.filter((e) => e.manga.manga_ref !== mangaRef).map((e) => (
                <button key={e.source_id + e.manga.manga_ref} className="btn sm"
                  onClick={() => nav(`/title/${encodeURIComponent(e.manga.manga_ref)}`, { state: { group }, replace: true })}>
                  {e.source_name} · {langName(e.lang)}
                </button>
              ))}
            </div>
          )}
        </div>
      ) : error ? <ErrorBox error={error} onRetry={reload} /> : null}
      {m && (
        <div className="detail-hero">
          <div className="cover">{m.thumbnail_url ? <img src={remoteUrl(m.thumbnail_url)} alt="" /> : <div className="fallback tone">{m.title}</div>}</div>
          <div className="col" style={{ gap: 12 }}>
            <h1 dir="auto">{m.title}</h1>
            {group && group.entries.some((e) => e.manga.title !== m.title) && (
              <div className="muted" dir="auto">{[...new Set(group.entries.map((e) => e.manga.title).filter((t) => t !== m.title))].join(" / ")}</div>
            )}
            <div className="meta-line">
              {m.author && <span dir="auto">{m.author}</span>}
              {publicationStatus(m.status) && <span>{publicationStatus(m.status)}</span>}
              {data && <span>{chapters.length} פרקים</span>}
              {doneCount > 0 && <span>{doneCount} הורדו</span>}
              {localManga && translatedPages > 0 && <span>{translatedPages} עמודים תורגמו</span>}
            </div>
            {m.genres.length > 0 && <div className="row wrap">{m.genres.slice(0, 8).map((g) => <span key={g} className="badge" dir="auto">{g}</span>)}</div>}
            {m.description && <p className="muted clamp-4" dir="auto" style={{ maxWidth: 760 }}>{m.description}</p>}
            <div className="row wrap" style={{ marginTop: 4 }}>
              <button className="btn primary" disabled={!data || !chapters.length || waitingFor !== null} onClick={readNow}>
                {waitingFor !== null ? <Spinner label="מוריד את הפרק הראשון…" /> : <><Play size={16} /> קריאה עכשיו</>}
              </button>
              <button className="btn" disabled={!pending.length} onClick={() => askAll(pending)}>
                <Download size={16} /> הורדת כל הפרקים{pending.length && pending.length < chapters.length ? ` (${pending.length} שנותרו)` : ""}
              </button>
              {data?.local_manga_id && (
                <button className="btn ghost" onClick={() => nav(`/manga/${data.local_manga_id}`)}><Library size={16} /> בספרייה שלי</button>
              )}
            </div>
            {group && group.entries.length > 1 && (
              <div className="row wrap small">
                <span className="muted">זמין גם ב:</span>
                {group.entries.map((e) => (
                  <button key={e.source_id + e.manga.manga_ref} className={`btn sm ${e.manga.manga_ref === mangaRef ? "primary" : "ghost"}`}
                    onClick={() => nav(`/title/${encodeURIComponent(e.manga.manga_ref)}`, { state: { group }, replace: true })}>
                    {e.source_name} · {langName(e.lang)}
                  </button>
                ))}
              </div>
            )}
            {group && group.entries.length === 1 && <span className="small faint">מקור: {group.entries[0].source_name} · {langName(group.entries[0].lang)}</span>}
          </div>
        </div>
      )}

      <div className="section">
        <div className="spread">
          <h2>פרקים</h2>
          <div className="row wrap">
            {sel.size > 0 && (
              <button className="btn primary sm" onClick={() => download(chapters.filter((c) => sel.has(c.chapter_ref)))}>
                <Download size={14} /> הורדת פרק{sel.size > 1 ? `ים (${sel.size})` : ""}
              </button>
            )}
            <span className="small muted">טווח:</span>
            <input className="input" style={{ width: 70, height: 28 }} type="number" min={1} placeholder="מ-" value={range.from} onChange={(e) => setRange({ ...range, from: e.target.value })} aria-label="מפרק" />
            <input className="input" style={{ width: 70, height: 28 }} type="number" min={1} placeholder="עד" value={range.to} onChange={(e) => setRange({ ...range, to: e.target.value })} aria-label="עד פרק" />
            <button className="btn sm" onClick={downloadRange}>הורדת הטווח</button>
          </div>
        </div>
        {!data && !error && <Spinner label="טוען פרקים מהמקור…" />}
        {data && chapters.length === 0 && <p className="muted">המקור לא החזיר פרקים לכותר הזה.</p>}
        {data && chapters.length > 0 && (
          <div className="panel" style={{ padding: 0 }}>
            {chapters.map((c, i) => {
              const st = data.downloads[c.chapter_ref];
              const localId = data.local_chapters[c.chapter_ref];
              return (
                <div key={c.chapter_ref} className="list-row">
                  <input type="checkbox" disabled={st === "done"} checked={sel.has(c.chapter_ref)} style={{ accentColor: "var(--marker)" }}
                    aria-label={`בחירת ${c.title}`}
                    onChange={(e) => { const s = new Set(sel); if (e.target.checked) s.add(c.chapter_ref); else s.delete(c.chapter_ref); setSel(s); }} />
                  <span className="faint small" style={{ width: 34 }}>{i + 1}</span>
                  <span className="grow" dir="auto">{c.title}</span>
                  {c.scanlator && <span className="faint small" dir="auto">{c.scanlator}</span>}
                  {st === "done" && <span className="badge ok">הורד</span>}
                  {(st === "queued" || st === "running") && <span className="badge run">בהורדה</span>}
                  {st === "failed" && <span className="badge bad">נכשל</span>}
                  {localId ? (
                    <button className="btn ghost sm" onClick={() => nav(`/read/${localId}?page=0`)}><BookOpen size={14} /> קריאה</button>
                  ) : (
                    st !== "queued" && st !== "running" && (
                      <button className="btn ghost sm" onClick={() => download([c])}><Download size={14} /> הורדת פרק</button>
                    )
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>

      {confirm && (
        <Modal title={`להוריד ${confirm.length} פרקים?`} onClose={() => setConfirm(null)}
          actions={<>
            <button className="btn ghost" onClick={() => setConfirm(null)}>ביטול</button>
            <button className="btn primary" onClick={() => { download(confirm); setConfirm(null); }}>הורדה</button>
          </>}>
          <p className="muted">
            {hint.data
              ? `לפי הפרקים שכבר הורדו, צפוי לתפוס בערך ${bytes(hint.data * confirm.length)} בדיסק.`
              : "עוד אין הורדות קודמות, אז אי אפשר להעריך בצורה אמינה כמה מקום זה יתפוס."}{" "}
            ההורדה רצה ברקע ואפשר לעצור אותה במסך ההורדות.
          </p>
        </Modal>
      )}
    </div>
  );
}

