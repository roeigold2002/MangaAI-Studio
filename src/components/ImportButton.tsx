import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderInput, Library } from "lucide-react";
import { api } from "../lib/api";
import { toast, toastError } from "../lib/store";
import { Modal } from "./ui";
import type { ImportReport } from "../lib/types";

/** Import a folder / CBZ / ZIP, or scan a library root. */
export function ImportButtons({ onDone }: { onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ reports: ImportReport[]; errors: string[] } | null>(null);

  async function run(mode: "manga" | "library", kind: "folder" | "file") {
    const picked = await open({
      directory: kind === "folder",
      multiple: true,
      title: mode === "library" ? "בחר תיקיית ספרייה" : "בחר תיקייה או קובץ מנגה",
      filters: kind === "file" ? [{ name: "Manga archives", extensions: ["cbz", "zip"] }] : undefined,
    });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    setBusy(true);
    try {
      const r = await api.importPaths(paths, mode);
      onDone();
      if (r.errors.length || r.reports.some((x) => x.warnings.length)) setResult(r);
      else toast(`יובאו ${r.reports.length} כותרים`, "ok");
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div className="row wrap">
        <button className="btn primary" disabled={busy} onClick={() => run("manga", "folder")}>
          <FolderInput size={16} /> {busy ? "מייבא…" : "ייבוא תיקייה"}
        </button>
        <button className="btn" disabled={busy} onClick={() => run("manga", "file")}>ייבוא CBZ / ZIP</button>
        <button className="btn ghost" disabled={busy} onClick={() => run("library", "folder")} title="כל תת-תיקייה תיובא כמנגה נפרדת">
          <Library size={16} /> סריקת תיקיית ספרייה
        </button>
      </div>
      {result && (
        <Modal title="תוצאות הייבוא" onClose={() => setResult(null)} actions={<button className="btn primary" onClick={() => setResult(null)}>סגור</button>}>
          <div className="col">
            {result.reports.map((r) => (
              <div key={r.manga_id} className="col" style={{ gap: 4 }}>
                <strong dir="auto">{r.title}</strong>
                <span className="muted small">
                  {r.chapters_added} פרקים חדשים, {r.chapters_updated} עודכנו, {r.chapters_unchanged} ללא שינוי
                </span>
                {r.warnings.slice(0, 8).map((w, i) => (
                  <span key={i} className="small faint" dir="auto">{w}</span>
                ))}
              </div>
            ))}
            {result.errors.map((e, i) => (
              <div key={i} className="callout bad small" dir="auto">{e}</div>
            ))}
          </div>
        </Modal>
      )}
    </>
  );
}
