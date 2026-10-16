import { useState } from "react";
import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { toast, toastError, useApp } from "../lib/store";
import { ago } from "../lib/util";
import type { MergeReport } from "../lib/types";

function describe(r: MergeReport): string {
  const parts = [];
  if (r.chapters_updated) parts.push(`${r.chapters_updated} פרקים עודכנו`);
  if (r.series_added.length) parts.push(`נוספו סדרות: ${r.series_added.join(", ")}`);
  if (r.glossary_updated) parts.push(`${r.glossary_updated} מונחים במילון`);
  if (r.memory_added) parts.push(`${r.memory_added} תרגומים בזיכרון`);
  if (r.favorites_changed) parts.push(`${r.favorites_changed} מועדפים`);
  return parts.length ? parts.join(" · ") : "הכל כבר מסונכרן";
}

/** Settings → Data: sync folder shared between computers, and backup / restore. */
export function BackupSync() {
  const { settings, saveSettings } = useApp();
  const [busy, setBusy] = useState<string | null>(null);
  const [withChapters, setWithChapters] = useState(false);
  if (!settings) return null;
  const folder = settings.sync?.folder ?? "";

  async function run<T>(label: string, fn: () => Promise<T>, done: (v: T) => void) {
    setBusy(label);
    try {
      done(await fn());
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(null);
    }
  }

  async function chooseFolder() {
    const dir = await openDialog({ directory: true, title: "תיקייה משותפת לסנכרון (OneDrive, Dropbox, Google Drive…)" });
    if (typeof dir !== "string") return;
    await saveSettings({ ...settings!, sync: { ...settings!.sync, folder: dir } }).catch(toastError);
    run("מסנכרן…", api.syncNow, (r) => toast(`סונכרן: ${describe(r)}`, "ok"));
  }

  async function backup() {
    const stamp = new Date().toISOString().slice(0, 10);
    const path = await saveDialog({ defaultPath: `mangaai-backup-${stamp}.zip`, filters: [{ name: "ZIP", extensions: ["zip"] }] });
    if (!path) return;
    run("יוצר גיבוי…", () => api.createBackup(path, withChapters), (r) =>
      toast(`הגיבוי נשמר (${(r.bytes / 1e6).toFixed(1)} MB, ${r.files} קבצים)`, "ok"));
  }

  async function restore() {
    const path = await openDialog({ filters: [{ name: "ZIP", extensions: ["zip"] }], title: "בחירת קובץ גיבוי" });
    if (typeof path !== "string") return;
    const ok = await ask("הספרייה תוחלף בזו שבגיבוי בהפעלה הבאה של האפליקציה. הספרייה הנוכחית נשמרת בצד. להמשיך?", { title: "שחזור מגיבוי", kind: "warning" });
    if (!ok) return;
    run("בודק את הגיבוי…", () => api.restoreBackup(path), (n) =>
      toast(`הגיבוי מוכן${n ? ` (${n} קבצי פרקים חולצו)` : ""}. סגרו ופתחו את האפליקציה כדי להשלים את השחזור.`, "ok"));
  }

  return (
    <div className="panel">
      <div className="panel-head"><h2>גיבוי וסנכרון</h2>{busy && <span className="small muted">{busy}</span>}</div>
      <div className="col">
        <p className="small muted">
          סנכרון בין מחשבים דרך תיקייה משותפת (OneDrive, Dropbox, Google Drive או תיקיית רשת): ההתקדמות בקריאה, פרקים שנקראו,
          מועדפים וקטגוריות, המילון וזיכרון התרגום, ורשימת הסדרות מהאינטרנט. הסנכרון רץ בפתיחה, כל רבע שעה ובסגירה.
          העמודים עצמם לא עוברים: כל מחשב מוריד אותם בעצמו.
        </p>
        <div className="spread">
          <span className="small ltr" style={{ textAlign: "start" }}>{folder || "לא נבחרה תיקייה"}</span>
          <div className="row">
            <button className="btn sm" disabled={!!busy} onClick={chooseFolder}>{folder ? "החלפת תיקייה" : "בחירת תיקייה"}</button>
            {folder && <button className="btn sm" disabled={!!busy} onClick={() => run("מסנכרן…", api.syncNow, (r) => toast(`סונכרן: ${describe(r)}`, "ok"))}>סנכרון עכשיו</button>}
            {folder && <button className="btn sm ghost" disabled={!!busy} onClick={() => saveSettings({ ...settings!, sync: { ...settings!.sync, folder: "" } }).catch(toastError)}>כיבוי</button>}
          </div>
        </div>
        {settings.sync?.last_sync_at && <span className="small faint">סנכרון אחרון: {ago(settings.sync.last_sync_at)}</span>}
        <div className="spread">
          <span className="small muted">גיבוי לקובץ ZIP: כל הספרייה, ההתקדמות, המילונים והתרגומים (בלי מפתחות API).</span>
          <div className="row">
            <label className="check small"><input type="checkbox" checked={withChapters} onChange={(e) => setWithChapters(e.target.checked)} /><span>כולל הפרקים שהורדו</span></label>
            <button className="btn sm" disabled={!!busy} onClick={backup}>גיבוי</button>
            <button className="btn sm ghost" disabled={!!busy} onClick={restore}>שחזור מגיבוי</button>
          </div>
        </div>
      </div>
    </div>
  );
}
