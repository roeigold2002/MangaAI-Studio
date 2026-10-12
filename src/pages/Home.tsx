import { Link, useNavigate } from "react-router-dom";
import { Play } from "lucide-react";
import { api } from "../lib/api";
import { useEvent, useLoad, useThrottled } from "../lib/store";
import { ago, jobLabel } from "../lib/util";
import { Cover, Empty, ErrorBox, MangaCard, Progress } from "../components/ui";
import { ImportButtons } from "../components/ImportButton";

export default function HomePage() {
  const nav = useNavigate();
  const { data, error, reload } = useLoad(() => api.home(), []);
  const refresh = useThrottled(reload, 800);
  useEvent("job-updated", refresh);
  useEvent("download-updated", refresh);

  if (error) return <div className="page"><ErrorBox error={error} onRetry={reload} /></div>;
  if (!data) return null;

  const cont = data.continue_reading[0];
  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>בית</h1>
          <p className="muted">{data.library_count ? `${data.library_count} כותרים בספרייה` : "הספרייה ריקה"}</p>
        </div>
        <ImportButtons onDone={reload} />
      </div>

      {data.library_count === 0 && (
        <Empty title="עוד אין כאן מנגה">
          ייבא תיקייה עם תמונות, קובץ CBZ או ZIP מהמחשב. הקבצים המקוריים נשארים במקומם ולא משתנים.
        </Empty>
      )}

      {cont && (
        <div className="panel tone" style={{ display: "grid", gridTemplateColumns: "92px 1fr auto", gap: 20, alignItems: "center" }}>
          <div style={{ width: 92 }}><Cover mangaId={cont.manga_id} title={cont.manga_title} /></div>
          <div className="col" style={{ gap: 6, minWidth: 0 }}>
            <span className="muted small">המשך לקרוא</span>
            <h2 className="ellipsis" dir="auto">{cont.manga_title}</h2>
            <span className="muted" dir="auto">{cont.chapter_title} · עמוד {cont.page_idx + 1} מתוך {cont.page_count}</span>
            <div style={{ maxWidth: 360 }}><Progress value={cont.page_idx + 1} total={cont.page_count} /></div>
          </div>
          <button className="btn primary" onClick={() => nav(`/read/${cont.chapter_id}?page=${cont.page_idx}`)}>
            <Play size={16} /> המשך
          </button>
        </div>
      )}

      {data.active_jobs.length + data.active_downloads.length > 0 && (
        <div className="section">
          <div className="spread"><h2>בעבודה עכשיו</h2></div>
          <div className="panel list" style={{ padding: 0 }}>
            {data.active_jobs.map((j) => (
              <Link to="/queue" key={`j${j.id}`} className="list-row clickable">
                <span className="badge run">תרגום</span>
                <span className="grow ellipsis" dir="auto">{j.manga_title} · {j.chapter_title}</span>
                <span className="muted small">{jobLabel[j.status] ?? j.status}</span>
                <div style={{ width: 140 }}><Progress value={j.completed + j.failed} total={j.total} /></div>
              </Link>
            ))}
            {data.active_downloads.map((d) => (
              <Link to="/downloads" key={`d${d.id}`} className="list-row clickable">
                <span className="badge">הורדה</span>
                <span className="grow ellipsis" dir="auto">{d.manga_title} · {d.chapter_title}</span>
                <span className="muted small">{d.progress}/{d.total || "?"}</span>
                <div style={{ width: 140 }}><Progress value={d.progress} total={d.total} /></div>
              </Link>
            ))}
          </div>
        </div>
      )}

      {data.continue_reading.length > 1 && (
        <div className="section">
          <div className="spread"><h2>נקראו לאחרונה</h2><Link className="btn ghost sm" to="/history">כל ההיסטוריה</Link></div>
          <div className="covers compact">
            {data.continue_reading.slice(1).map((h) => (
              <div key={h.manga_id} className="cover-card" onClick={() => nav(`/read/${h.chapter_id}?page=${h.page_idx}`)}>
                <Cover mangaId={h.manga_id} title={h.manga_title} />
                <div>
                  <div className="cover-title" dir="auto">{h.manga_title}</div>
                  <div className="cover-sub" dir="auto">{h.chapter_title} · {ago(h.at)}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {data.recent_translations.length > 0 && (
        <div className="section">
          <div className="spread"><h2>תורגמו לאחרונה</h2></div>
          <div className="panel list" style={{ padding: 0 }}>
            {data.recent_translations.map((t) => (
              <div key={t.chapter_id} className="list-row clickable" onClick={() => nav(`/read/${t.chapter_id}`)}>
                <span className="grow ellipsis" dir="auto">{t.manga_title} · {t.chapter_title}</span>
                <span className={`badge ${t.pages_done === t.page_count ? "ok" : "run"}`}>{t.pages_done}/{t.page_count} עמודים</span>
                <span className="faint small">{ago(t.at)}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {data.recently_added.length > 0 && (
        <div className="section">
          <div className="spread"><h2>נוספו לאחרונה</h2><Link className="btn ghost sm" to="/library">לספרייה</Link></div>
          <div className="covers compact">
            {data.recently_added.map((m) => <MangaCard key={m.id} m={m} />)}
          </div>
        </div>
      )}
    </div>
  );
}
