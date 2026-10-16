import { useMemo, useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router-dom";
import { ArrowRight, BookOpen, Download, Library, Play } from "lucide-react";
import { api } from "../lib/api";
import { toast, toastError, useEvent, useLoad, useThrottled } from "../lib/store";
import { bytes, chaptersInRange, chapterStats, langName, publicationStatus, remoteUrl } from "../lib/util";
import { ErrorBox, Modal, Spinner } from "../components/ui";
import type { DownloadPlan, RemoteChapter, SearchGroup } from "../lib/types";

/** What a bulk download left out: "114 גרסאות כפולות דולגו · …" */
function planText(p: DownloadPlan): string {
  const parts = [];
  if (p.duplicates) parts.push(`${p.duplicates} גרסאות כפולות דולגו`);
  if (p.placeholders) parts.push(`${p.placeholders} פרקים של עמוד אחד שמפנה לאתר המו״ל דולגו`);
  if (p.already) parts.push(`${p.already} כבר בספרייה או בהורדה`);
  return parts.join(" · ");
}

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
  const [confirm, setConfirm] = useState<{ list: RemoteChapter[]; plan: DownloadPlan } | null>(null);
  /** Preferred scanlation group: undefined = as stored for this manga, null = automatic. */
  const [pref, setPref] = useState<string | null | undefined>(undefined);
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
  const stats = useMemo(() => chapterStats(chapters, data?.downloads ?? {}), [chapters, data]);
  const preferred = pref === undefined ? data?.preferred_scanlator ?? null : pref;
  const m = data?.manga ?? group?.entries[0].manga;

  /** `bestOnly`: download all / a range, one version per chapter; otherwise exactly the chosen chapters. */
  async function download(list: RemoteChapter[], bestOnly = false) {
    if (!data || !list.length) return;
    try {
      const r = await api.enqueueDownloads(data.manga, list, { bestOnly, preferred });
      const summary = r.plan ? planText(r.plan) : "";
      toast(
        (r.added ? `${r.added} פרקים נכנסו להורדה. הם יופיעו ב"הספרייה שלי" כשיסתיימו.` : "הפרקים האלה כבר הורדו או בהורדה") +
          (summary ? ` (${summary})` : ""),
        "ok",
      );
      setSel(new Set());
      reload();
    } catch (e) {
      toastError(e);
    }
  }

  /** Download all / a range: one version per chapter, without one-page placeholders. */
  async function askAll(list: RemoteChapter[]) {
    if (!data || !list.length) return;
    try {
      const plan = await api.planDownloads(data.manga, list, preferred);
      if (!plan.chapters) return toast(`אין פרקים חדשים להורדה. ${planText(plan)}`, "ok");
      if (plan.chapters > 20) setConfirm({ list, plan });
      else download(list, true);
    } catch (e) {
      toastError(e);
    }
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
    if (range.from === "" || range.to === "" || Number.isNaN(from) || Number.isNaN(to) || from > to) return toast("בחר טווח תקין: מפרק מספר … עד פרק מספר …", "bad");
    askAll(chaptersInRange(chapters, from, to));
  }

  const doneCount = stats.distinct - stats.missing;
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
              {data && <span>{stats.distinct} פרקים</span>}
              {data && stats.versions > stats.distinct && (
                <span title="המקור מציג כמה גרסאות לאותו פרק (קבוצות תרגום שונות). בהורדת כל הפרקים יורדת גרסה אחת לכל פרק.">
                  {stats.versions} גרסאות
                </span>
              )}
              {doneCount > 0 && <span>{doneCount} הורדו</span>}
              {localManga && translatedPages > 0 && <span>{translatedPages} עמודים תורגמו</span>}
            </div>
            {m.genres.length > 0 && <div className="row wrap">{m.genres.slice(0, 8).map((g) => <span key={g} className="badge" dir="auto">{g}</span>)}</div>}
            {m.description && <p className="muted clamp-4" dir="auto" style={{ maxWidth: 760 }}>{m.description}</p>}
            <div className="row wrap" style={{ marginTop: 4 }}>
              <button className="btn primary" disabled={!data || !chapters.length || waitingFor !== null} onClick={readNow}>
                {waitingFor !== null ? <Spinner label="מוריד את הפרק הראשון…" /> : <><Play size={16} /> קריאה עכשיו</>}
              </button>
              <button className="btn" disabled={!stats.missing} onClick={() => askAll(chapters)}>
                <Download size={16} /> הורדת כל הפרקים{stats.missing && stats.missing < stats.distinct ? ` (${stats.missing} שנותרו)` : ""}
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
            {stats.groups.length > 1 && (
              <>
                <span className="small muted">קבוצת תרגום מועדפת:</span>
                <select className="select" style={{ width: "auto", height: 28 }} value={preferred ?? ""} aria-label="קבוצת תרגום מועדפת"
                  onChange={(e) => setPref(e.target.value || null)}>
                  <option value="">אוטומטי</option>
                  {stats.groups.map(([g, n]) => <option key={g} value={g}>{g} ({n})</option>)}
                </select>
              </>
            )}
            {sel.size > 0 && (
              <button className="btn primary sm" onClick={() => download(chapters.filter((c) => sel.has(c.chapter_ref)))}>
                <Download size={14} /> הורדת פרק{sel.size > 1 ? `ים (${sel.size})` : ""}
              </button>
            )}
            <span className="small muted">טווח:</span>
            <input className="input" style={{ width: 70, height: 28 }} type="number" min={0} placeholder="מ-" value={range.from} onChange={(e) => setRange({ ...range, from: e.target.value })} aria-label="ממספר פרק" />
            <input className="input" style={{ width: 70, height: 28 }} type="number" min={0} placeholder="עד" value={range.to} onChange={(e) => setRange({ ...range, to: e.target.value })} aria-label="עד מספר פרק" />
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
                  {c.page_count === 1 && <span className="badge" title="לפעמים זה רק עמוד שמפנה לאתר המו״ל">עמוד אחד</span>}
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
        <Modal title={`להוריד ${confirm.plan.chapters} פרקים?`} onClose={() => setConfirm(null)}
          actions={<>
            <button className="btn ghost" onClick={() => setConfirm(null)}>ביטול</button>
            <button className="btn primary" onClick={() => { download(confirm.list, true); setConfirm(null); }}>הורדה</button>
          </>}>
          <p>
            גרסה אחת לכל פרק{confirm.plan.preferred ? <>, עדיפות לקבוצה <b dir="auto">{confirm.plan.preferred}</b></> : ""}.{" "}
            <span className="muted">{planText(confirm.plan)}</span>
          </p>
          <p className="muted">
            {hint.data
              ? `לפי הפרקים שכבר הורדו, צפוי לתפוס בערך ${bytes(hint.data * confirm.plan.chapters)} בדיסק.`
              : "עוד אין הורדות קודמות, אז אי אפשר להעריך בצורה אמינה כמה מקום זה יתפוס."}{" "}
            ההורדה רצה ברקע ואפשר לעצור אותה במסך ההורדות.
          </p>
        </Modal>
      )}
    </div>
  );
}

