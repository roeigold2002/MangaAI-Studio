import { useNavigate } from "react-router-dom";
import { Pause, Play, RotateCcw, X } from "lucide-react";
import { api } from "../lib/api";
import { toastError, useEvent, useLoad, useThrottled } from "../lib/store";
import { ago, jobLabel, usd } from "../lib/util";
import { Empty, ErrorBox, Progress } from "../components/ui";
import type { Job } from "../lib/types";

export default function QueuePage() {
  const nav = useNavigate();
  const { data, error, reload } = useLoad(() => api.jobs(true), []);
  const usage = useLoad(() => api.usage(), []);
  const refresh = useThrottled(() => { reload(); usage.reload(); }, 500);
  useEvent("job-updated", refresh);

  const act = (j: Job, a: "pause" | "resume" | "cancel" | "retry_failed") => api.jobAction(j.id, a).then(reload).catch(toastError);
  const active = (data ?? []).filter((j) => ["running", "queued", "paused"].includes(j.status));
  const finished = (data ?? []).filter((j) => !["running", "queued", "paused"].includes(j.status));
  const total = (usage.data ?? []).reduce((s, u) => s + (u.cost_usd ?? 0), 0);

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>תור תרגום</h1>
          <p className="muted">העבודה נשמרת ברקע וממשיכה גם אחרי סגירה ופתיחה של האפליקציה.</p>
        </div>
        {finished.length > 0 && <button className="btn ghost" onClick={() => api.clearFinishedJobs().then(reload)}>ניקוי משימות שהסתיימו</button>}
      </div>
      {error && <ErrorBox error={error} onRetry={reload} />}
      {data && data.length === 0 && <Empty title="התור ריק">בחר פרקים בדף של מנגה ולחץ על תרגום, או תרגם עמוד ישירות מהקורא.</Empty>}

      {active.length > 0 && <JobList jobs={active} act={act} onOpen={(j) => nav(`/read/${j.chapter_id}`)} />}
      {finished.length > 0 && (
        <div className="section">
          <div className="spread"><h2>הסתיימו</h2></div>
          <JobList jobs={finished} act={act} onOpen={(j) => nav(`/read/${j.chapter_id}`)} />
        </div>
      )}

      {(usage.data?.length ?? 0) > 0 && (
        <div className="section">
          <div className="spread"><h2>שימוש בספקים</h2><span className="muted">סה״כ מדווח {usd(total)}</span></div>
          <div className="panel" style={{ padding: 0 }}>
            {usage.data!.map((u) => (
              <div key={`${u.provider}-${u.model}`} className="list-row">
                <span className="grow ltr" style={{ textAlign: "start" }}>{u.provider} · {u.model}</span>
                <span className="muted small">{u.requests} בקשות</span>
                <span className="muted small ltr">{(u.input_tokens / 1000).toFixed(1)}k in / {(u.output_tokens / 1000).toFixed(1)}k out</span>
                <span style={{ width: 90, textAlign: "end" }}>{usd(u.cost_usd)}</span>
              </div>
            ))}
          </div>
          <p className="small faint" style={{ marginTop: 8 }}>
            עלות מחושבת לפי מספר הטוקנים שהספק דיווח ולפי המחירים שהוגדרו לדגם. Claude Code מדווח עלות משוערת של המנוי ולא חיוב בפועל.
          </p>
        </div>
      )}
    </div>
  );
}

function JobList({ jobs, act, onOpen }: { jobs: Job[]; act: (j: Job, a: "pause" | "resume" | "cancel" | "retry_failed") => void; onOpen: (j: Job) => void }) {
  return (
    <div className="panel" style={{ padding: 0 }}>
      {jobs.map((j) => (
        <div key={j.id} className="list-row" style={{ alignItems: "flex-start" }}>
          <div className="col grow" style={{ gap: 6 }}>
            <div className="row">
              <span className="ellipsis clickable" dir="auto" style={{ cursor: "pointer", fontWeight: 500 }} onClick={() => onOpen(j)}>
                {j.manga_title} · {j.chapter_title}
              </span>
              {j.page_ids && <span className="badge">{j.page_ids.length} עמודים נבחרים</span>}
              {j.mode === "all" && <span className="badge">תרגום מחדש</span>}
            </div>
            <div style={{ maxWidth: 520 }}><Progress value={j.completed + j.failed} total={j.total} ok={j.status === "done"} /></div>
            <div className="row small muted wrap">
              <span className={`badge ${j.status === "done" ? "ok" : j.status === "failed" ? "bad" : j.status === "running" ? "run" : ""}`}>
                {jobLabel[j.status] ?? j.status}
              </span>
              <span>{j.completed}/{j.total} עמודים</span>
              {j.failed > 0 && <span style={{ color: "var(--bad)" }}>{j.failed} נכשלו</span>}
              {j.model && <span className="ltr">{j.model}</span>}
              {j.cost_usd > 0 && <span>{usd(j.cost_usd)}</span>}
              <span className="faint">{ago(j.updated_at)}</span>
            </div>
            {j.error && <span className="small" style={{ color: "var(--bad)" }} dir="auto">{j.error}</span>}
          </div>
          <div className="row" style={{ gap: 4 }}>
            {(j.status === "running" || j.status === "queued") && <button className="btn sm ghost" onClick={() => act(j, "pause")}><Pause size={14} /> השהיה</button>}
            {j.status === "paused" && <button className="btn sm" onClick={() => act(j, "resume")}><Play size={14} /> המשך</button>}
            {j.failed > 0 && !["running", "queued"].includes(j.status) && <button className="btn sm" onClick={() => act(j, "retry_failed")}><RotateCcw size={14} /> ניסיון חוזר לנכשלים</button>}
            {j.status === "cancelled" && <button className="btn sm ghost" onClick={() => act(j, "resume")}><RotateCcw size={14} /> הפעלה מחדש</button>}
            {["running", "queued", "paused", "failed"].includes(j.status) && <button className="btn ghost icon sm" onClick={() => act(j, "cancel")} aria-label="ביטול"><X size={14} /></button>}
          </div>
        </div>
      ))}
    </div>
  );
}
