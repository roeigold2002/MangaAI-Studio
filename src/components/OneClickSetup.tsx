import { useState } from "react";
import { Check, CircleAlert, Loader2, Sparkles } from "lucide-react";
import { api } from "../lib/api";
import { toast, toastError, useEvent, useLoad } from "../lib/store";
import type { SetupNeeds, SetupStep } from "../lib/types";

const STEPS: { key: SetupStep["step"]; label: string; size: string }[] = [
  { key: "engine", label: "מנוע ה-AI (זיהוי בועות, OCR, ניקוי ועימוד עברית)", size: "כ-1GB" },
  { key: "models", label: "מודלים: זיהוי, OCR ליפנית ולאנגלית, ניקוי טקסט", size: "כ-2GB" },
  { key: "sources", label: "מנוע המקורות עם MangaDex ו-MangaFire", size: "כ-330MB" },
];

function done(n: SetupNeeds, key: SetupStep["step"]) {
  return key === "engine" ? n.engine : key === "models" ? n.models : n.sources;
}

/** Home: everything a new install needs, behind one button, with live progress. */
export function OneClickSetup() {
  const needs = useLoad(() => api.setupNeeds(), []);
  const [gpuChoice, setGpuChoice] = useState<boolean | null>(null);
  const [state, setState] = useState<Record<string, SetupStep>>({});
  const [running, setRunning] = useState(false);
  const [bytes, setBytes] = useState<{ done: number; total: number | null } | null>(null);

  useEvent<SetupStep>("setup-progress", (s) => setState((cur) => ({ ...cur, [s.step]: s })));
  useEvent<{ done: number; total: number | null }>("runtime-progress", (p) => setBytes(p));

  const n = needs.data;
  if (!n || (n.engine && n.models && n.sources && !running && !Object.keys(state).length)) return null;
  const gpu = n.gpu_offered && (gpuChoice ?? true);

  async function start() {
    setRunning(true);
    setState({});
    try {
      const results = await api.setupEverything(gpu);
      const failed = results.filter((r) => r.status === "failed");
      if (failed.length) toast(`ההכנה הסתיימה, אבל ${failed.length} שלבים נכשלו. אפשר ללחוץ שוב כדי לנסות אותם מחדש.`, "bad");
      else toast("הכל מוכן: אפשר לחפש מנגה, להוריד ולתרגם.", "ok");
    } catch (e) {
      toastError(e);
    } finally {
      setRunning(false);
      setBytes(null);
      needs.reload();
    }
  }

  return (
    <div className="panel tone col" style={{ gap: 12, marginBottom: 22 }}>
      <div className="spread">
        <h2><Sparkles size={18} style={{ verticalAlign: "-3px" }} /> הכנה בלחיצה אחת</h2>
      </div>
      <p className="muted">
        לחיצה אחת מתקינה את כל מה שהאפליקציה צריכה. זה לוקח כמה דקות, ואפשר להמשיך להשתמש בה בזמן הזה. צריך חיבור לאינטרנט רק עכשיו.
      </p>
      <div className="col" style={{ gap: 8 }}>
        {STEPS.map((st) => {
          const s = state[st.key];
          const already = done(n, st.key) && !s;
          const status = s?.status ?? (already ? "done" : running ? "waiting" : "todo");
          return (
            <div key={st.key} className="row small">
              {status === "running" ? <Loader2 size={16} className="spin" /> :
                status === "done" || status === "skipped" ? <Check size={16} style={{ color: "var(--ok)" }} /> :
                status === "failed" ? <CircleAlert size={16} style={{ color: "var(--bad)" }} /> :
                <span className="dot" />}
              <span className="grow">{st.label} <span className="faint">({st.size})</span></span>
              {status === "running" && st.key === "sources" && bytes?.total ? (
                <span className="faint ltr">{Math.round((bytes.done / bytes.total) * 100)}%</span>
              ) : null}
              {status === "failed" && <span className="small" style={{ color: "var(--bad)" }} dir="auto">{s?.detail}</span>}
            </div>
          );
        })}
      </div>
      {n.gpu_offered && !n.engine && (
        <label className="check small">
          <input type="checkbox" checked={gpu} disabled={running} onChange={(e) => setGpuChoice(e.target.checked)} />
          <span>להשתמש בכרטיס המסך {n.gpu_name?.replace(/^NVIDIA\s+/i, "")} (עוד כ-1.5GB להורדה, עיבוד מהיר פי כמה)</span>
        </label>
      )}
      <div className="row">
        <button className="btn primary" disabled={running} onClick={start}>
          {running ? <><Loader2 size={15} className="spin" /> מכין…</> : <><Sparkles size={15} /> הכנה בלחיצה אחת</>}
        </button>
      </div>
    </div>
  );
}
