import { useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { useUpdate } from "../lib/updates";
import { toastError } from "../lib/store";
import { Modal, Progress } from "./ui";

/** Asks before installing; nothing is downloaded until the user agrees. */
export function UpdatePrompt() {
  const { update, set } = useUpdate();
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  if (!update) return null;
  const busy = progress !== null;
  const close = () => !busy && set(null);

  async function install() {
    if (!update) return;
    setProgress({ done: 0, total: 0 });
    try {
      let done = 0;
      let total = 0;
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") done += e.data.chunkLength;
        setProgress({ done, total });
      });
      // Windows: the installer already closed the app and starts the new version itself.
      await relaunch();
    } catch (e) {
      setProgress(null);
      toastError(e);
    }
  }

  return (
    <Modal
      title={`גרסה חדשה זמינה: ${update.version}`}
      onClose={close}
      actions={
        <>
          <button className="btn ghost" disabled={busy} onClick={close}>לא עכשיו</button>
          <button className="btn primary" disabled={busy} onClick={install}>{busy ? "מתקין…" : "התקנה והפעלה מחדש"}</button>
        </>
      }
    >
      <p className="small muted">הגרסה המותקנת: {update.currentVersion}</p>
      {update.body && <p style={{ whiteSpace: "pre-wrap", marginTop: 10 }}>{update.body}</p>}
      <p className="small faint" style={{ marginTop: 10 }}>
        האפליקציה תיסגר ותיפתח מחדש. הורדות ותרגומים שבתור ימשיכו מאותה נקודה אחרי ההפעלה מחדש.
      </p>
      {progress && progress.total > 0 && <Progress value={progress.done} total={progress.total} />}
    </Modal>
  );
}
