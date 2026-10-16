import { useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { Check, CopyX, Heart, Languages, Play, RefreshCw, Trash2 } from "lucide-react";
import { api } from "../lib/api";
import { toast, toastError, useEvent, useLoad, useThrottled } from "../lib/store";
import { bytes, chapterTranslationBadge, publicationStatus, shortTitles } from "../lib/util";
import { Cover, Empty, ErrorBox, Field, Modal, Spinner } from "../components/ui";
import { TranslateFlow } from "../components/TranslateFlow";
import type { ChapterInfo, CleanupPlan, GlossaryEntry, MangaDetail } from "../lib/types";

export default function MangaPage() {
  const id = Number(useParams().id);
  const nav = useNavigate();
  const { data: m, error, reload } = useLoad(() => api.mangaDetail(id), [id]);
  const [tab, setTab] = useState<"chapters" | "glossary" | "memory" | "options">("chapters");
  const [sel, setSel] = useState<Set<number>>(new Set());
  const [translate, setTranslate] = useState<number[] | null>(null);
  const [flowOpen, setFlowOpen] = useState(false);
  const [cleanup, setCleanup] = useState(false);
  const refresh = useThrottled(reload, 1000);
  useEvent<{ chapter_id: number }>("page-updated", (p) => {
    if (m?.chapters.some((c) => c.id === p.chapter_id)) refresh();
  });
  useEvent("download-updated", refresh);

  if (error) return <div className="page"><ErrorBox error={error} onRetry={reload} /></div>;
  if (!m) return null;

  const next = m.chapters.find((c) => !c.read) ?? m.chapters[0];
  const resume = m.chapters.filter((c) => c.last_read_at).sort((a, b) => (b.last_read_at ?? "").localeCompare(a.last_read_at ?? ""))[0];
  const target = resume && !resume.read ? resume : next;
  const selected = [...sel];

  async function toggleFav() {
    await api.updateManga(id, { favorite: !m!.favorite }).catch(toastError);
    reload();
  }

  async function markRead(read: boolean) {
    await api.setChaptersRead(selected, read).catch(toastError);
    setSel(new Set());
    reload();
  }

  async function rescan() {
    try {
      const r = await api.rescanManga(id);
      toast(`${r.chapters_added} פרקים חדשים, ${r.chapters_updated} עודכנו`, "ok");
      r.warnings.slice(0, 3).forEach((w) => toast(w, "bad"));
      reload();
    } catch (e) {
      toastError(e);
    }
  }

  return (
    <div className="page">
      <div className="detail-hero">
        <Cover mangaId={m.id} title={m.title} />
        <div className="col" style={{ gap: 12 }}>
          <h1 dir="auto">{m.title}</h1>
          {m.alt_titles.length > 0 && <div className="muted" dir="auto">{m.alt_titles.join(" / ")}</div>}
          <div className="meta-line">
            {m.author && <span dir="auto">כתיבה: {m.author}</span>}
            {m.artist && m.artist !== m.author && <span dir="auto">איור: {m.artist}</span>}
            <span>{m.chapters.length} פרקים</span>
            <span>{m.source_kind === "source" ? "הורד ממקור" : "יובא מהמחשב"}</span>
            {publicationStatus(m.status) && <span>{publicationStatus(m.status)}</span>}
          </div>
          {m.genres.length > 0 && <div className="row wrap">{m.genres.map((g) => <span key={g} className="badge" dir="auto">{g}</span>)}</div>}
          {m.description ? <p className="muted" dir="auto" style={{ maxWidth: 760 }}>{m.description}</p> : null}
          <TranslationSummary m={m} />
          <div className="row wrap" style={{ marginTop: 6 }}>
            <button className="btn primary" onClick={() => setFlowOpen(true)} disabled={!m.chapters.length}>
              <Languages size={16} /> תרגם לעברית
            </button>
            {target && (
              <button className="btn" onClick={() => nav(`/read/${target.id}?page=${target.read ? 0 : target.last_page}`)}>
                <Play size={16} /> {resume ? "המשך קריאה" : "קריאה עכשיו"}
              </button>
            )}
            <button className="btn" onClick={toggleFav} aria-pressed={m.favorite}>
              <Heart size={16} fill={m.favorite ? "currentColor" : "none"} /> {m.favorite ? "במועדפים" : "הוסף למועדפים"}
            </button>
            {m.import_root && <button className="btn ghost" onClick={rescan}><RefreshCw size={16} /> סריקה מחדש</button>}
            {m.source_kind === "source" && (
              <button className="btn ghost" onClick={() => setCleanup(true)}><CopyX size={16} /> ניקוי פרקים כפולים</button>
            )}
          </div>
        </div>
      </div>

      <div className="section">
        <div className="tabs" role="tablist">
          {(
            [["chapters", "פרקים"], ["glossary", "מילון מונחים"], ["memory", "זיכרון תרגום"], ["options", "אפשרויות"]] as const
          ).map(([k, label]) => (
            <button key={k} role="tab" aria-selected={tab === k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}>{label}</button>
          ))}
        </div>
        {tab === "chapters" && (
          <Chapters m={m} sel={sel} setSel={setSel} onTranslate={setTranslate} onMarkRead={markRead} />
        )}
        {tab === "glossary" && <Glossary mangaId={m.id} />}
        {tab === "memory" && <Memory mangaId={m.id} />}
        {tab === "options" && <Options m={m} onSaved={reload} />}
      </div>

      {translate && <TranslateFlow chapters={m.chapters} preselect={translate} onClose={() => { setTranslate(null); setSel(new Set()); }} />}
      {flowOpen && <TranslateFlow chapters={m.chapters} onClose={() => setFlowOpen(false)} />}
      {cleanup && <DuplicateCleanup mangaId={m.id} onClose={() => setCleanup(false)} onDone={reload} />}
    </div>
  );
}

/** Remove extra versions of the same chapter (and one-page placeholders), after showing exactly what goes. */
function DuplicateCleanup({ mangaId, onClose, onDone }: { mangaId: number; onClose: () => void; onDone: () => void }) {
  const [placeholders, setPlaceholders] = useState(true);
  const [busy, setBusy] = useState(false);
  const { data: plan, error, loading } = useLoad(() => api.duplicateCleanupPlan(mangaId, placeholders), [mangaId, placeholders]);

  async function apply(p: CleanupPlan) {
    setBusy(true);
    try {
      const chapterIds = p.remove.flatMap((e) => (e.chapter_id != null ? [e.chapter_id] : []));
      const downloadIds = p.remove.flatMap((e) => (e.download_id != null ? [e.download_id] : []));
      const r = await api.cleanUpDuplicates(mangaId, placeholders, chapterIds, downloadIds);
      const parts = [`${r.chapters_removed} פרקים הוסרו (${bytes(r.bytes_freed)} התפנו)`];
      if (r.downloads_cancelled) parts.push(`${r.downloads_cancelled} הורדות כפולות בוטלו`);
      toast(parts.join(" · "), "ok");
      onDone();
      onClose();
    } catch (e) {
      toastError(e);
      setBusy(false);
    }
  }

  const n = plan?.remove.length ?? 0;
  return (
    <Modal title={plan && n ? `להסיר ${n} פרקים כפולים?` : "ניקוי פרקים כפולים"} onClose={onClose}
      actions={<>
        <button className="btn ghost" onClick={onClose}>{plan && !n ? "סגירה" : "ביטול"}</button>
        {plan && n > 0 && (
          <button className="btn danger" disabled={busy || loading} onClick={() => apply(plan)}>
            {busy ? <Spinner label="מוחק…" /> : <><Trash2 size={14} /> מחיקה</>}
          </button>
        )}
      </>}>
      {error && <ErrorBox error={error} />}
      {!plan && !error && <Spinner label="מחפש כפילויות…" />}
      {plan && (
        <>
          <p className="muted">
            {n
              ? <>נשאר עותק אחד לכל פרק ({plan.kept} פרקים נשארים{plan.preferred ? <>, עדיפות לקבוצה <b dir="auto">{plan.preferred}</b></> : ""}).
                  {" "}העותקים הכפולים יימחקו מהספרייה ומהדיסק{plan.bytes ? ` (${bytes(plan.bytes)})` : ""}. פרקים שייבאת בעצמך ופרקים עם תרגום לא נמחקים.</>
              : "לא נמצאו פרקים כפולים להסרה."}
          </p>
          <label className="check">
            <input type="checkbox" checked={placeholders} onChange={(e) => setPlaceholders(e.target.checked)} />
            <span className="small">להסיר גם פרקים של עמוד אחד שרק מפנים לאתר המו״ל</span>
          </label>
          {n > 0 && (
            <div className="panel" style={{ padding: 0, maxHeight: 280, overflow: "auto" }}>
              {plan.remove.map((e) => (
                <div key={`${e.chapter_id}-${e.download_id}`} className="list-row small">
                  <span className="grow" dir="auto">{e.title}</span>
                  {e.scanlator && <span className="faint" dir="auto">{e.scanlator}</span>}
                  {e.pages != null && <span className="faint">{e.pages} עמ׳</span>}
                  <span className="badge">
                    {e.chapter_id == null ? "יבוטל מתור ההורדות" : e.reason === "placeholder" ? "עמוד מפנה" : "עותק נוסף"}
                  </span>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </Modal>
  );
}

function Chapters({ m, sel, setSel, onTranslate, onMarkRead }: {
  m: MangaDetail; sel: Set<number>; setSel: (s: Set<number>) => void; onTranslate: (ids: number[]) => void; onMarkRead: (r: boolean) => void;
}) {
  const nav = useNavigate();
  const [desc, setDesc] = useState(false);
  const list = useMemo(() => (desc ? [...m.chapters].reverse() : m.chapters), [m.chapters, desc]);
  const short = useMemo(() => {
    const s = shortTitles(m.chapters.map((c) => c.title));
    return new Map(m.chapters.map((c, i) => [c.id, s[i]]));
  }, [m.chapters]);
  if (!m.chapters.length) return <Empty title="אין פרקים">הקבצים של המנגה הזו לא נמצאו. נסה סריקה מחדש.</Empty>;
  const toggle = (id: number) => {
    const s = new Set(sel);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    setSel(s);
  };
  return (
    <>
      <div className="spread" style={{ marginBottom: 10 }}>
        <div className="row">
          <label className="check">
            <input type="checkbox" checked={sel.size === m.chapters.length} onChange={(e) => setSel(e.target.checked ? new Set(m.chapters.map((c) => c.id)) : new Set())} />
            <span className="muted small">{sel.size ? `${sel.size} נבחרו` : "בחר הכול"}</span>
          </label>
          {sel.size > 0 && (
            <>
              <button className="btn sm" onClick={() => onTranslate([...sel])}><Languages size={14} /> תרגום הנבחרים</button>
              <button className="btn sm ghost" onClick={() => onMarkRead(true)}><Check size={14} /> סמן כנקרא</button>
              <button className="btn sm ghost" onClick={() => onMarkRead(false)}>סמן כלא נקרא</button>
            </>
          )}
        </div>
        <button className="btn ghost sm" onClick={() => setDesc(!desc)}>{desc ? "מהחדש לישן" : "מהישן לחדש"}</button>
      </div>
      <div className="panel" style={{ padding: 0 }}>
        {list.map((c: ChapterInfo) => {
          const tb = chapterTranslationBadge(c.translation);
          return (
            <div key={c.id} className={`chapter-row ${c.read ? "read" : ""}`}>
              <input type="checkbox" checked={sel.has(c.id)} onChange={() => toggle(c.id)} aria-label={`בחר ${c.title}`} style={{ accentColor: "var(--marker)" }} />
              <div className="col" style={{ gap: 2, minWidth: 0 }}>
                <span className="ch-title ellipsis" dir="auto" role="link" tabIndex={0}
                  onClick={() => nav(`/read/${c.id}?page=${c.read ? 0 : c.last_page}`)}
                  onKeyDown={(e) => e.key === "Enter" && nav(`/read/${c.id}`)}>
                  {short.get(c.id) ?? c.title}
                </span>
                <span className="faint small">
                  {c.page_count} עמודים
                  {!c.read && c.last_page > 0 ? ` · נעצר בעמוד ${c.last_page + 1}` : ""}
                  {!c.available ? " · הקבצים חסרים" : ""}
                </span>
              </div>
              <div className="row" style={{ gap: 6 }}>
                {c.downloaded && <span className="badge">הורד</span>}
                {tb && <span className={`badge ${tb.cls}`}>{tb.text}</span>}
                {c.read && <span className="badge">נקרא</span>}
              </div>
              <button className="btn ghost icon sm" title="תרגום הפרק" aria-label="תרגום הפרק" onClick={() => onTranslate([c.id])}>
                <Languages size={15} />
              </button>
            </div>
          );
        })}
      </div>
    </>
  );
}

const KINDS: Record<string, string> = {
  character: "דמות", term: "מונח", place: "מקום", attack: "מתקפה / כוח", organization: "ארגון", honorific: "תואר כבוד", other: "אחר",
};

function Glossary({ mangaId }: { mangaId: number }) {
  const { data, reload } = useLoad(() => api.glossary(mangaId), [mangaId]);
  const [edit, setEdit] = useState<Partial<GlossaryEntry> | null>(null);
  const suggested = (data ?? []).filter((g) => g.status === "suggested");
  const confirmed = (data ?? []).filter((g) => g.status === "confirmed");

  async function save(e: Partial<GlossaryEntry>, status = "confirmed") {
    try {
      await api.upsertGlossary(mangaId, { source: e.source ?? "", target: e.target ?? "", kind: e.kind ?? "term", gender: e.gender ?? null, notes: e.notes ?? "", status });
      setEdit(null);
      reload();
    } catch (err) {
      toastError(err);
    }
  }

  return (
    <div className="col" style={{ gap: 18 }}>
      <div className="spread">
        <p className="muted" style={{ maxWidth: 640 }}>
          המונחים המאושרים נשלחים עם כל בקשת תרגום, כדי ששמות ומונחים ייכתבו תמיד אותו דבר. מגדר של דמות עוזר להטיות נכונות בעברית.
        </p>
        <button className="btn primary" onClick={() => setEdit({ kind: "character" })}>הוספת מונח</button>
      </div>
      {suggested.length > 0 && (
        <div className="panel">
          <div className="panel-head"><h3>הצעות מהמתרגם ({suggested.length})</h3><span className="faint small">לא בשימוש עד שתאשר</span></div>
          {suggested.map((g) => (
            <div key={g.id} className="list-row">
              <span className="grow" dir="auto"><bdi>{g.source}</bdi> ← <bdi>{g.target}</bdi> <span className="faint small">({KINDS[g.kind] ?? g.kind})</span></span>
              <button className="btn sm" onClick={() => save(g)}>אישור</button>
              <button className="btn sm ghost" onClick={() => setEdit(g)}>עריכה</button>
              <button className="btn sm ghost danger" onClick={() => api.deleteGlossary(g.id).then(reload)}>דחייה</button>
            </div>
          ))}
        </div>
      )}
      {confirmed.length === 0 ? (
        <Empty title="המילון ריק">הוסף שמות דמויות ומונחים חוזרים, או אשר הצעות שיופיעו אחרי תרגום.</Empty>
      ) : (
        <div className="panel" style={{ padding: 0 }}>
          {confirmed.map((g) => (
            <div key={g.id} className="list-row clickable" onClick={() => setEdit(g)}>
              <bdi className="grow" style={{ flexBasis: 0 }}>{g.source}</bdi>
              <bdi className="grow" style={{ flexBasis: 0, fontWeight: 500 }}>{g.target}</bdi>
              <span className="badge">{KINDS[g.kind] ?? g.kind}</span>
              {g.gender && <span className="badge">{g.gender === "m" ? "זכר" : g.gender === "f" ? "נקבה" : "ניטרלי"}</span>}
            </div>
          ))}
        </div>
      )}
      {edit && (
        <Modal title={edit.id ? "עריכת מונח" : "מונח חדש"} onClose={() => setEdit(null)}
          actions={<>
            {edit.id && <button className="btn ghost danger" onClick={() => api.deleteGlossary(edit.id!).then(() => { setEdit(null); reload(); })}><Trash2 size={14} /> מחיקה</button>}
            <div className="grow" />
            <button className="btn ghost" onClick={() => setEdit(null)}>ביטול</button>
            <button className="btn primary" onClick={() => save(edit)}>שמירה</button>
          </>}>
          <div className="grid-form">
            <Field label="במקור"><input className="input" dir="auto" value={edit.source ?? ""} onChange={(e) => setEdit({ ...edit, source: e.target.value })} disabled={!!edit.id} /></Field>
            <Field label="בעברית"><input className="input" dir="rtl" value={edit.target ?? ""} onChange={(e) => setEdit({ ...edit, target: e.target.value })} /></Field>
            <Field label="סוג">
              <select className="select" value={edit.kind ?? "term"} onChange={(e) => setEdit({ ...edit, kind: e.target.value })}>
                {Object.entries(KINDS).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
              </select>
            </Field>
            <Field label="מגדר (לדמויות)">
              <select className="select" value={edit.gender ?? ""} onChange={(e) => setEdit({ ...edit, gender: e.target.value || null })}>
                <option value="">לא צוין</option><option value="m">זכר</option><option value="f">נקבה</option><option value="n">ניטרלי</option>
              </select>
            </Field>
          </div>
          <Field label="הערות למתרגם" hint="למשל: מדבר בגסות, אחותו של הגיבור, משתמש בלשון רשמית">
            <textarea className="textarea" rows={2} value={edit.notes ?? ""} onChange={(e) => setEdit({ ...edit, notes: e.target.value })} />
          </Field>
        </Modal>
      )}
    </div>
  );
}

function Memory({ mangaId }: { mangaId: number }) {
  const { data, reload } = useLoad(() => api.memory(mangaId), [mangaId]);
  if (!data) return null;
  if (!data.length) return <Empty title="זיכרון התרגום ריק">כל תיקון שתאשר בעורך נשמר כאן, ושורות זהות בעתיד יתורגמו אותו דבר בלי לשלוח אותן שוב.</Empty>;
  return (
    <div className="panel" style={{ padding: 0 }}>
      {data.map((t) => (
        <div key={t.id} className="list-row">
          <bdi className="grow" style={{ flexBasis: 0 }}>{t.source_text}</bdi>
          <bdi className="grow" style={{ flexBasis: 0 }}>{t.target_text}</bdi>
          <button className="btn ghost icon sm" aria-label="מחיקה" onClick={() => api.deleteMemory(t.id).then(reload)}><Trash2 size={14} /></button>
        </div>
      ))}
    </div>
  );
}

function Options({ m, onSaved }: { m: MangaDetail; onSaved: () => void }) {
  const nav = useNavigate();
  const cats = useLoad(() => api.categories(), []);
  const [form, setForm] = useState({
    title: m.title, reading_dir: m.reading_dir ?? "", reading_mode: m.reading_mode ?? "", source_lang: m.source_lang,
    translation_notes: m.translation_notes, categories: m.categories,
  });
  const [confirmRemove, setConfirmRemove] = useState(false);

  async function save() {
    try {
      await api.updateManga(m.id, {
        title: form.title,
        reading_dir: form.reading_dir || null,
        reading_mode: form.reading_mode || null,
        source_lang: form.source_lang,
        translation_notes: form.translation_notes,
        categories: form.categories,
      });
      toast("נשמר", "ok");
      onSaved();
    } catch (e) {
      toastError(e);
    }
  }

  return (
    <div className="col" style={{ gap: 18, maxWidth: 760 }}>
      <div className="grid-form">
        <Field label="שם"><input className="input" dir="auto" value={form.title} onChange={(e) => setForm({ ...form, title: e.target.value })} /></Field>
        <Field label="כיוון דפדוף" hint="מנגה יפנית: מימין לשמאל">
          <select className="select" value={form.reading_dir} onChange={(e) => setForm({ ...form, reading_dir: e.target.value })}>
            <option value="">לפי ההגדרה הכללית</option><option value="rtl">מימין לשמאל</option><option value="ltr">משמאל לימין</option>
          </select>
        </Field>
        <Field label="מצב קריאה">
          <select className="select" value={form.reading_mode} onChange={(e) => setForm({ ...form, reading_mode: e.target.value })}>
            <option value="">לפי ההגדרה הכללית</option><option value="single">עמוד בודד</option><option value="double">כפולה</option><option value="vertical">גלילה אנכית (וובטון)</option>
          </select>
        </Field>
        <Field label="שפת המקור">
          <select className="select" value={form.source_lang} onChange={(e) => setForm({ ...form, source_lang: e.target.value })}>
            <option value="ja">יפנית</option>
            <option value="en">אנגלית</option>
            <option value="ko">קוריאנית (המתרגם קורא מהתמונה)</option>
            <option value="zh">סינית (המתרגם קורא מהתמונה)</option>
          </select>
        </Field>
      </div>
      <Field label="הערות סגנון למתרגם" hint="נשלחות עם כל בקשה. למשל: לשמור על -סאן/-קון, טון קליל וסלנג עכשווי, הגיבורה מדברת בגוף ראשון נקבה.">
        <textarea className="textarea" rows={4} dir="rtl" value={form.translation_notes} onChange={(e) => setForm({ ...form, translation_notes: e.target.value })} />
      </Field>
      {(cats.data?.length ?? 0) > 0 && (
        <Field label="קטגוריות">
          <div className="row wrap">
            {cats.data!.map((c) => (
              <label key={c.id} className="check">
                <input type="checkbox" checked={form.categories.includes(c.id)}
                  onChange={(e) => setForm({ ...form, categories: e.target.checked ? [...form.categories, c.id] : form.categories.filter((x) => x !== c.id) })} />
                {c.name}
              </label>
            ))}
          </div>
        </Field>
      )}
      <div className="row"><button className="btn primary" onClick={save}>שמירה</button></div>
      <div className="callout bad col">
        <strong>הסרה מהספרייה</strong>
        <span className="small">
          {m.source_kind === "local"
            ? "הקבצים המקוריים במחשב לא יימחקו. רק הרשומה בספרייה, ההתקדמות והתרגומים."
            : "הרשומה, ההתקדמות והתרגומים יוסרו. אפשר גם למחוק את העמודים שהורדו."}
        </span>
        <div><button className="btn danger sm" onClick={() => setConfirmRemove(true)}><Trash2 size={14} /> הסרה</button></div>
      </div>
      {confirmRemove && (
        <Modal title={`להסיר את "${m.title}"?`} onClose={() => setConfirmRemove(false)}
          actions={<>
            <button className="btn ghost" onClick={() => setConfirmRemove(false)}>ביטול</button>
            {m.source_kind === "source" && (
              <button className="btn danger" onClick={() => api.removeManga(m.id, true).then(() => nav("/library")).catch(toastError)}>הסרה ומחיקת ההורדות</button>
            )}
            <button className="btn primary" onClick={() => api.removeManga(m.id, false).then(() => nav("/library")).catch(toastError)}>הסרה מהספרייה</button>
          </>}>
          <p className="muted">אי אפשר לבטל את הפעולה.</p>
        </Modal>
      )}
    </div>
  );
}

/** Honest summary of translation state across chapters (partial results shown as partial). */
function TranslationSummary({ m }: { m: MangaDetail }) {
  const pages = m.chapters.reduce((s, c) => s + c.page_count, 0);
  const done = m.chapters.reduce((s, c) => s + c.translation.done, 0);
  const failed = m.chapters.reduce((s, c) => s + c.translation.failed, 0);
  const fullChapters = m.chapters.filter((c) => c.page_count > 0 && c.translation.done === c.page_count).length;
  const read = m.chapters.filter((c) => c.read).length;
  if (!m.chapters.length) return null;
  return (
    <div className="row wrap small" style={{ gap: 14 }}>
      <span className="muted">קריאה: {read}/{m.chapters.length} פרקים</span>
      <span className="muted">
        עברית: {done === 0 ? "עוד לא תורגם" : `${fullChapters} פרקים מלאים · ${done}/${pages} עמודים`}
      </span>
      {failed > 0 && <span style={{ color: "var(--bad)" }}>{failed} עמודים נכשלו (אפשר לנסות שוב מהקורא או מתור התרגום)</span>}
    </div>
  );
}
