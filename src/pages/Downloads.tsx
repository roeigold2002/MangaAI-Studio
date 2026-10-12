import { Link } from "react-router-dom";
import { RotateCcw, X } from "lucide-react";
import { api } from "../lib/api";
import { toastError, useEvent, useLoad, useThrottled } from "../lib/store";
import { ago, jobLabel } from "../lib/util";
import { Empty, ErrorBox, Progress } from "../components/ui";

export default function DownloadsPage() {
  const { data, error, reload } = useLoad(() => api.downloads(), []);
  const refresh = useThrottled(reload, 400);
  useEvent("download-updated", refresh);
  const finished = (data ?? []).filter((d) => d.status === "done" || d.status === "cancelled").length;

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>הורדות</h1>
          <p className="muted">פרקים שהורדו נשמרים בספרייה המקומית ונשארים זמינים גם בלי חיבור.</p>
        </div>
        {finished > 0 && <button className="btn ghost" onClick={() => api.clearFinishedDownloads().then(reload)}>ניקוי הורדות שהסתיימו</button>}
      </div>
      {error && <ErrorBox error={error} onRetry={reload} />}
      {data && data.length === 0 && (
        <Empty title="אין הורדות" action={<Link className="btn" to="/browse">לעיון במקורות</Link>}>
          פרקים שתבחר להוריד מעיון במקורות יופיעו כאן עם ההתקדמות שלהם.
        </Empty>
      )}
      {data && data.length > 0 && (
        <div className="panel" style={{ padding: 0 }}>
          {data.map((d) => (
            <div key={d.id} className="list-row">
              <div className="col grow" style={{ gap: 5 }}>
                <span className="ellipsis" dir="auto">
                  {d.manga_id ? <Link to={`/manga/${d.manga_id}`}>{d.manga_title}</Link> : d.manga_title} · {d.chapter_title}
                </span>
                <div style={{ maxWidth: 460 }}><Progress value={d.progress} total={d.total} ok={d.status === "done"} /></div>
                <div className="row small muted">
                  <span className={`badge ${d.status === "done" ? "ok" : d.status === "failed" ? "bad" : d.status === "running" ? "run" : ""}`}>{jobLabel[d.status] ?? d.status}</span>
                  <span>{d.progress}/{d.total || "?"} עמודים</span>
                  <span className="faint">{ago(d.updated_at)}</span>
                </div>
                {d.error && <span className="small" style={{ color: "var(--bad)" }} dir="auto">{d.error}</span>}
              </div>
              {(d.status === "failed" || d.status === "cancelled") && (
                <button className="btn sm" onClick={() => api.downloadAction(d.id, "retry").then(reload).catch(toastError)}><RotateCcw size={14} /> ניסיון חוזר</button>
              )}
              {(d.status === "queued" || d.status === "running") && (
                <button className="btn ghost sm" onClick={() => api.downloadAction(d.id, "cancel").then(reload).catch(toastError)}><X size={14} /> ביטול</button>
              )}
              {d.chapter_id && <Link className="btn ghost sm" to={`/read/${d.chapter_id}`}>קריאה</Link>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
