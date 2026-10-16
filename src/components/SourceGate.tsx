import { useState } from "react";
import { Link } from "react-router-dom";
import { Compass, Download, Power, RefreshCw, Settings2 } from "lucide-react";
import { api, errMessage } from "../lib/api";
import { toast, useApp, useEvent } from "../lib/store";
import { Progress, Spinner } from "./ui";
import { ImportButtons } from "./ImportButton";
import type { SourceOverview } from "../lib/types";

import { sourceState } from "../lib/sources";
export { sourceState };

export function SourceGate({ overview, onChange, compact }: { overview: SourceOverview | null; onChange: () => void; compact?: boolean }) {
  const { settings, saveSettings, loadSettings } = useApp();
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEvent<{ done: number; total: number | null }>("runtime-progress", (p) => setProgress(p));
  const state = sourceState(overview);
  if (state === "ready") return null;

  async function run(label: string, fn: () => Promise<unknown>) {
    setBusy(label);
    setError(null);
    try {
      await fn();
      await loadSettings(); // the backend may have changed source settings (managed install)
      onChange();
    } catch (e) {
      setError(errMessage(e));
    } finally {
      setBusy(null);
      setProgress(null);
    }
  }

  const retry = (
    <button className="btn ghost" disabled={!!busy} onClick={() => run("בודק…", async () => onChange())}>
      <RefreshCw size={15} /> בדיקה חוזרת
    </button>
  );
  const advanced = (
    <Link className="btn ghost" to="/settings?tab=sources"><Settings2 size={15} /> הגדרות מתקדמות</Link>
  );

  let title = "";
  let body: React.ReactNode = null;
  let actions: React.ReactNode = null;
  switch (state) {
    case "disabled":
      title = "החיפוש באינטרנט כבוי";
      body = "הפעלת המקורות האונליין מאפשרת לחפש ולהוריד מנגה מתוך האפליקציה.";
      actions = (
        <button className="btn primary" disabled={!!busy}
          onClick={() => run("מפעיל…", () => saveSettings({ ...settings!, sources: { ...settings!.sources, suwayomi_enabled: true } }))}>
          <Power size={15} /> הפעלה
        </button>
      );
      break;
    case "not_installed":
      title = "צריך להתקין את מנוע המקורות";
      body = (
        <>
          החיפוש וההורדה עובדים דרך <bdi>Suwayomi</bdi>, שרת קוד פתוח שרץ על המחשב שלך בלבד. האפליקציה יכולה להתקין ולהפעיל אותו בשבילך:
          הורדה של כ-{overview?.runtime.download_size_mb ?? 330} MB מהגרסה הרשמית ({overview?.runtime.version}), עם בדיקת חתימת הקובץ (checksum).
          {overview?.runtime.supported === false && " אין גרסה מוכנה למערכת הזו; אפשר לחבר שרת קיים בהגדרות המתקדמות."}
        </>
      );
      actions = (
        <button className="btn primary" disabled={!!busy || overview?.runtime.supported === false}
          onClick={() => run("מתקין…", async () => { await api.setupSources(); toast("מנוע המקורות הותקן ופועל", "ok"); })}>
          <Download size={15} /> התקנה והפעלה
        </button>
      );
      break;
    case "stopped":
      title = "מנוע המקורות כבוי";
      body = overview?.runtime_error ?? "השרת המקומי לא פועל כרגע.";
      actions = (
        <button className="btn primary" disabled={!!busy} onClick={() => run("מפעיל… (עד דקה)", () => api.startSources())}>
          <Power size={15} /> הפעלה
        </button>
      );
      break;
    case "auth":
      title = "השרת דורש שם משתמש וסיסמה";
      body = "השרת שהוגדר דחה את פרטי ההתחברות. אפשר לעדכן אותם בהגדרות המתקדמות.";
      break;
    case "error":
      title = "אין חיבור למנוע המקורות";
      body = overview?.health?.message ?? "השרת לא ענה כמצופה.";
      break;
    case "local_only":
      title = "אין עדיין אתרי מנגה מותקנים";
      body = (
        <>
          מנוע המקורות פועל, אבל לא מותקנים בו תוספי מקורות. תוספים מגיעים מ"חנויות תוספים" שמפרסמים גורמים חיצוניים;
          חנות הקהילה Keiyoushi נרשמת כברירת מחדל. בוחרים אילו אתרים להתקין בהגדרות המתקדמות.
          עד אז אפשר לייבא מנגה מהמחשב ולקרוא ולתרגם אותה.
        </>
      );
      actions = <Link className="btn primary" to="/settings?tab=sources"><Compass size={15} /> הוספת מקורות</Link>;
      break;
  }

  return (
    <div className={`panel tone col ${compact ? "" : ""}`} style={{ gap: 12 }} role="status">
      <h3>{title}</h3>
      <p className="muted" style={{ maxWidth: 760 }}>{body}</p>
      {busy && (
        <div className="col" style={{ gap: 6, maxWidth: 420 }}>
          <Spinner label={busy} />
          {progress && <Progress value={progress.done} total={progress.total ?? progress.done * 2} />}
          {progress && <span className="small faint">{(progress.done / 1048576).toFixed(0)} MB{progress.total ? ` מתוך ${(progress.total / 1048576).toFixed(0)} MB` : ""}</span>}
        </div>
      )}
      {error && <div className="callout bad small" dir="auto">{error}</div>}
      <div className="row wrap">
        {actions}
        {retry}
        {state !== "local_only" && advanced}
      </div>
      {!compact && (state === "local_only" || state === "not_installed") && (
        <div className="col" style={{ gap: 8, marginTop: 4 }}>
          <span className="small faint">אפשר גם לייבא מנגה שכבר נמצאת במחשב:</span>
          <ImportButtons onDone={() => toast("יובא לספרייה", "ok")} />
        </div>
      )}
    </div>
  );
}
