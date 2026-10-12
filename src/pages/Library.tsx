import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Heart, LayoutGrid, List, Plus, Search, X } from "lucide-react";
import { api } from "../lib/api";
import { toastError, useApp, useLoad } from "../lib/store";
import { ago } from "../lib/util";
import { Empty, ErrorBox, MangaCard, Modal } from "../components/ui";
import { ImportButtons } from "../components/ImportButton";
import type { MangaSummary } from "../lib/types";

type Sort = "title" | "recent_read" | "added" | "unread";
type Filter = "all" | "unread" | "translated" | "downloaded" | "local";

export default function LibraryPage({ favorites = false }: { favorites?: boolean }) {
  const nav = useNavigate();
  const { settings, saveSettings } = useApp();
  const { data, error, reload } = useLoad(() => api.listManga(), []);
  const cats = useLoad(() => api.categories(), []);
  const [q, setQ] = useState("");
  const [sort, setSort] = useState<Sort>("recent_read");
  const [filter, setFilter] = useState<Filter>("all");
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
    if (filter === "unread") list = list.filter((m) => m.read_count < m.chapter_count);
    if (filter === "translated") list = list.filter((m) => m.translated_chapters > 0);
    if (filter === "downloaded") list = list.filter((m) => m.source_kind === "source");
    if (filter === "local") list = list.filter((m) => m.source_kind === "local");
    const by = {
      title: (a: MangaSummary, b: MangaSummary) => a.title.localeCompare(b.title, undefined, { numeric: true }),
      recent_read: (a: MangaSummary, b: MangaSummary) => (b.last_read_at ?? "").localeCompare(a.last_read_at ?? "") || a.title.localeCompare(b.title),
      added: (a: MangaSummary, b: MangaSummary) => b.created_at.localeCompare(a.created_at),
      unread: (a: MangaSummary, b: MangaSummary) => (b.chapter_count - b.read_count) - (a.chapter_count - a.read_count),
    }[sort];
    return [...list].sort(by);
  }, [data, favorites, cat, q, filter, sort]);

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

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>{favorites ? "מועדפים" : "ספרייה"}</h1>
          <p className="muted">{items.length} כותרים</p>
        </div>
        {!favorites && <ImportButtons onDone={reload} />}
      </div>

      <div className="row wrap" style={{ marginBottom: 14 }}>
        <div className="search" style={{ width: 260 }}>
          <Search size={16} />
          <input className="input" placeholder="חיפוש לפי שם" value={q} onChange={(e) => setQ(e.target.value)} aria-label="חיפוש" />
        </div>
        <select className="select" style={{ width: 170 }} value={filter} onChange={(e) => setFilter(e.target.value as Filter)} aria-label="סינון">
          <option value="all">הכול</option>
          <option value="unread">עם פרקים שלא נקראו</option>
          <option value="translated">עם פרקים מתורגמים</option>
          <option value="downloaded">הורדו ממקור</option>
          <option value="local">יובאו מהמחשב</option>
        </select>
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
          <Empty title="הספרייה ריקה">ייבא תיקייה, CBZ או ZIP, או הורד פרקים מעיון במקורות.</Empty>
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
