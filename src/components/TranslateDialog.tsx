import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, errCode, errMessage } from "../lib/api";
import { toast, useApp } from "../lib/store";
import { usd } from "../lib/util";
import { Modal, Spinner } from "./ui";
import type { Estimate } from "../lib/types";

/** Estimate → (confirm if needed) → enqueue. Shows what leaves the computer. */
export function TranslateDialog({
  chapterIds, pageIds, mode = "missing", priority = 0, onClose, onQueued,
}: {
  chapterIds: number[]; pageIds?: number[]; mode?: "missing" | "failed" | "all"; priority?: number;
  onClose: () => void; onQueued?: () => void;
}) {
  const nav = useNavigate();
  const settings = useApp((s) => s.settings);
  const [est, setEst] = useState<Estimate | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [redo, setRedo] = useState(mode === "all");

  useEffect(() => {
    api.estimate(chapterIds).then(setEst).catch((e) => setError(errMessage(e)));
  }, [chapterIds]);

  async function go() {
    setBusy(true);
    try {
      await api.enqueue(chapterIds, { pageIds, mode: redo ? "all" : mode, priority });
      toast(pageIds ? "העמוד נוסף לתור התרגום" : `${chapterIds.length} פרקים נוספו לתור התרגום`, "ok");
      onQueued?.();
      onClose();
    } catch (e) {
      const code = errCode(e);
      if (code === "cloud_notice" || code === "local_only" || code === "no_provider") {
        setError(errMessage(e));
      } else {
        setError(errMessage(e));
      }
    } finally {
      setBusy(false);
    }
  }

  const pages = pageIds ? pageIds.length : est?.pages_to_translate ?? 0;
  const needsNotice = est && !est.local && !settings?.cloud_notice_accepted;

  return (
    <Modal
      title={pageIds ? "תרגום עמוד" : chapterIds.length > 1 ? `תרגום ${chapterIds.length} פרקים` : "תרגום פרק"}
      onClose={onClose}
      actions={
        <>
          <button className="btn ghost" onClick={onClose}>ביטול</button>
          {needsNotice ? (
            <button className="btn primary" onClick={() => nav("/translation")}>להגדרות התרגום</button>
          ) : (
            <button className="btn primary" disabled={!est || busy} onClick={go}>
              {est?.needs_confirmation ? "אישור ותרגום" : "תרגם"}
            </button>
          )}
        </>
      }
    >
      {!est && !error && <Spinner label="מחשב הערכה…" />}
      {error && <div className="callout bad">{error}</div>}
      {est && (
        <div className="col">
          <div className="spread"><span className="muted">ספק</span><span className="ltr">{est.provider} · {est.model || "ברירת מחדל"}</span></div>
          <div className="spread"><span className="muted">עמודים לתרגום</span><span>{redo && !pageIds ? est.pages_total : pages} מתוך {est.pages_total}</span></div>
          {!redo && est.pages_to_translate < est.pages_total && (
            <span className="small faint">עמודים שכבר תורגמו באותן הגדרות לא יישלחו שוב.</span>
          )}
          <label className="check small">
            <input type="checkbox" checked={redo} onChange={(e) => setRedo(e.target.checked)} />
            <span>לתרגם מחדש גם עמודים שכבר תורגמו (תיקונים שאישרת נשמרים)</span>
          </label>
          <div className="spread">
            <span className="muted">עלות משוערת</span>
            <span>{est.local ? "חינם (מקומי)" : est.cost_usd === null ? "לא ידוע (לא הוגדר מחיר לדגם)" : `~${usd(est.cost_usd)}`}</span>
          </div>
          <span className="small faint">
            ההערכה גסה: מבוססת על כ-{Math.round(est.input_tokens / Math.max(1, est.pages_to_translate))} טוקנים נכנסים לעמוד.
            העלות בפועל נרשמת בתור התרגום.
          </span>
          {!est.local && (
            <div className="callout small">
              {est.sends_images
                ? "הטקסט שזוהה ותמונות העמודים יישלחו לספק הענן לצורך התרגום."
                : "רק הטקסט שזוהה יישלח לספק הענן; התמונות נשארות במחשב."}
            </div>
          )}
          {needsNotice && (
            <div className="callout bad small">לפני תרגום בענן צריך לאשר את הודעת הפרטיות בהגדרות התרגום.</div>
          )}
          {est.needs_confirmation && !needsNotice && (
            <div className="callout small">העלות המשוערת גבוהה מסף האישור שהגדרת ({usd(settings?.translation.confirm_cost_above_usd)}).</div>
          )}
        </div>
      )}
    </Modal>
  );
}
