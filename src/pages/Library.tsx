import { useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { BellRing, Heart, LayoutGrid, List, Plus, Search, X } from "lucide-react";
import { api } from "../lib/api";
import { toast, toastError, useApp, useLoad } from "../lib/store";
import { ago } from "../lib/util";
import { Empty, ErrorBox, MangaCard, Modal } from "../components/ui";
import { ImportButtons } from "../components/ImportButton";
import type { MangaSummary } from "../lib/types";

type Sort = "title" | "recent_read" | "added" | "unread";
type Tab = "all" | "reading" | "recent" | "downloaded" | "translated" | "favorites";
const TABS: [Tab, string][] = [
  ["all", "הכול"], ["reading", "קורא עכשיו"], ["recent", "נוספו לאחרונה"], ["downloaded", "הורדו"], ["translated", "תורגמו"], ["favorites", "מועדפים"],
];

export default function LibraryPage() {
  const nav = useNavigate();
  const [params, setParams] = useSearchParams();
  const tab = (params.get("tab") as Tab) || "all";
  const favorites = tab === "favorites";
  const { settings, saveSettings } = useApp();
  const { data, error, reload } = useLoad(() => api.listManga(), []);
  const cats = useLoad(() => api.categories(), []);
  const [q, setQ] = useState("");
  const [sort, setSort] = useState<Sort>("recent_read");
  const [cat, setCat] = useState<number | null>(null);
  const [newCat, setNewCat] = useState<string | null>(null);
  const view = settings?.library_view === "list" ? "list" : "grid";

  const items = useMemo(() => {
    let list: MangaSummary[] = data ?? [];
    if (favorites) list = list.filter((m) => m.favorite);
    if (cat !== null) list = list.filter((m) => m.categories.includes(cat));
    if (q.trim()) {
      const s = q.trim().toLowerCase();
      list = list.filter((m) => m.title.toLowerCase().includes(s));
    }
    if (tab === "reading") list = list.filter((m) => m.last_read_at && m.read_count < m.chapter_count);
    if (tab === "downloaded") list = list.filter((m) => m.downloaded_chapters > 0);
    if (tab === "translated") list = list.filter((m) => m.translated_pages > 0);
    if (tab === "recent") {
      const week = Date.now() - 14 * 86400000;
      list = list.filter((m) => new Date(m.created_at).getTime() > week);
    }
    const by = {
      title: (a: MangaSummary, b: MangaSummary) => a.title.localeCompare(b.title, undefined, { numeric: true }),
      recent_read: (a: MangaSummary, b: MangaSummary) => (b.last_read_at ?? "").localeCompare(a.last_read_at ?? "") || a.title.localeCompare(b.title),
      added: (a: MangaSummary, b: MangaSummary) => b.created_at.localeCompare(a.created_at),
      unread: (a: MangaSummary, b: MangaSummary) => (b.chapter_count - b.read_count) - (a.chapter_count - a.read_count),
    }[sort];
    return [...list].sort(tab === "recent" ? (a, b) => b.created_at.localeCompare(a.created_at) : by);
  }, [data, favorites, cat, q, tab, sort]);

  async function setView(v: "grid" | "list") {
    if (settings) await saveSettings({ ...settings, library_view: v }).catch(toastError);
  }

  async function addCategory() {
    if (!newCat?.trim()) return;
    try {
      await api.createCategory(newCat);
      setNewCat(null);
      cats.reload();
    } catch (e) {
      toastError(e);
    }
  }

  const [checking, setChecking] = useState(false);
  async function checkNew() {
    setChecking(true);
    try {
      const r = await api.checkNewChapters();
      if (!r.new.length) toast(`אין פרקים חדשים (${r.checked} סדרות נבדקו${r.failed.length ? `, ${r.failed.length} לא ענו` : ""})`, "ok");
      reload();
    } catch (e) {
      toastError(e);
    } finally {
      setChecking(false);
    }
  }

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>הספרייה שלי</h1>
          <p className="muted">{items.length} כותרים</p>
        </div>
        <div className="row wrap">
          <button className="btn ghost" disabled={checking} onClick={checkNew} title="בודק בכל הסדרות מהאינטרנט אם יצאו פרקים חדשים">
            <BellRing size={15} /> {checking ? "בודק…" : "בדיקת פרקים חדשים"}
          </button>
          <ImportButtons onDone={reload} />
        </div>
      </div>

      <div className="tabs" role="tablist">
        {TABS.map(([k, label]) => (
          <button key={k} role="tab" aria-selected={tab === k} className={tab === k ? "on" : ""} onClick={() => setParams(k === "all" ? {} : { tab: k })}>{label}</button>
        ))}
      </div>

      <div className="row wrap" style={{ marginBottom: 14 }}>
        <div className="search" style={{ width: 260 }}>
          <Search size={16} />
          <input className="input" placeholder="חיפוש לפי שם" value={q} onChange={(e) => setQ(e.target.value)} aria-label="חיפוש" />
        </div>
        <select className="select" style={{ width: 170 }} value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="מיון">
          <option value="recent_read">נקרא לאחרונה</option>
          <option value="title">שם</option>
          <option value="added">נוסף לאחרונה</option>
          <option value="unread">הכי הרבה שלא נקראו</option>
        </select>
        <div className="grow" />
        <div className="seg" role="group" aria-label="תצוגה">
          <button className={view === "grid" ? "on" : ""} onClick={() => setView("grid")} aria-label="רשת"><LayoutGrid size={15} /></button>
          <button className={view === "list" ? "on" : ""} onClick={() => setView("list")} aria-label="רשימה"><List size={15} /></button>
        </div>
      </div>

      <div className="row wrap" style={{ marginBottom: 22 }}>
        <button className={`btn sm ${cat === null ? "primary" : "ghost"}`} onClick={() => setCat(null)}>כל הקטגוריות</button>
        {(cats.data ?? []).map((c) => (
          <span key={c.id} className="row" style={{ gap: 2 }}>
            <button className={`btn sm ${cat === c.id ? "primary" : "ghost"}`} onClick={() => setCat(c.id)}>
              {c.name} <span className="faint">{c.count}</span>
            </button>
            {cat === c.id && (
              <button className="btn ghost icon sm" aria-label={`מחק את ${c.name}`}
                onClick={async () => { await api.deleteCategory(c.id).catch(toastError); setCat(null); cats.reload(); reload(); }}>
                <X size={14} />
              </button>
            )}
          </span>
        ))}
        <button className="btn ghost sm" onClick={() => setNewCat("")}><Plus size={14} /> קטגוריה</button>
      </div>

      {error && <ErrorBox error={error} onRetry={reload} />}
      {data && items.length === 0 && (
        favorites ? (
          <Empty title="אין מועדפים" icon={<Heart size={30} strokeWidth={1.5} />}>סמן מנגה כמועדפת מדף הפרטים שלה.</Empty>
        ) : data.length === 0 ? (
          <Empty title="הספרייה ריקה" action={<button className="btn primary" onClick={() => nav("/search")}>חיפוש מנגה</button>}>
            מנגה שתוריד תופיע כאן אוטומטית. אפשר גם לייבא תיקייה, CBZ או ZIP מהמחשב.
          </Empty>
        ) : (
          <Empty title="אין תוצאות">נסה חיפוש או סינון אחר.</Empty>
        )
      )}

      {view === "grid" ? (
        <div className="covers">{items.map((m) => <MangaCard key={m.id} m={m} />)}</div>
      ) : (
        items.length > 0 && (
          <div className="panel list" style={{ padding: 0 }}>
            {items.map((m) => (
              <div key={m.id} className="list-row clickable" onClick={() => nav(`/manga/${m.id}`)}>
                {m.favorite && <Heart size={14} fill="currentColor" className="muted" />}
                <span className="grow ellipsis" dir="auto">{m.title}</span>
                <span className="muted small nowrap">{m.read_count}/{m.chapter_count} נקראו</span>
                {m.translated_chapters > 0 && <span className="badge mark">{m.translated_chapters} מתורגמים</span>}
                <span className="faint small nowrap" style={{ width: 110, textAlign: "end" }}>{ago(m.last_read_at) || "לא נקרא"}</span>
              </div>
            ))}
          </div>
        )
      )}

      {newCat !== null && (
        <Modal title="קטגוריה חדשה" onClose={() => setNewCat(null)}
          actions={<><button className="btn ghost" onClick={() => setNewCat(null)}>ביטול</button><button className="btn primary" onClick={addCategory}>יצירה</button></>}>
          <input className="input" autoFocus value={newCat} onChange={(e) => setNewCat(e.target.value)} onKeyDown={(e) => e.key === "Enter" && addCategory()} placeholder="למשל: שונן, לקרוא בהמשך" />
        </Modal>
      )}
    </div>
  );
}
