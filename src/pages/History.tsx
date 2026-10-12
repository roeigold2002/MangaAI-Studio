import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { History } from "lucide-react";
import { api } from "../lib/api";
import { toastError, useLoad } from "../lib/store";
import { Cover, Empty, ErrorBox, Modal, Progress } from "../components/ui";
import type { HistoryEntry } from "../lib/types";

function dayLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const diff = Math.floor((new Date(today.toDateString()).getTime() - new Date(d.toDateString()).getTime()) / 86400000);
  if (diff === 0) return "היום";
  if (diff === 1) return "אתמול";
  return d.toLocaleDateString("he-IL", { weekday: "long", day: "numeric", month: "long" });
}

export default function HistoryPage() {
  const nav = useNavigate();
  const { data, error, reload } = useLoad(() => api.history(300), []);
  const [confirm, setConfirm] = useState(false);
  const groups = new Map<string, HistoryEntry[]>();
  for (const h of data ?? []) {
    const k = dayLabel(h.at);
    groups.set(k, [...(groups.get(k) ?? []), h]);
  }
  return (
    <div className="page">
      <div className="page-head">
        <h1>היסטוריית קריאה</h1>
        {(data?.length ?? 0) > 0 && <button className="btn ghost" onClick={() => setConfirm(true)}>ניקוי ההיסטוריה</button>}
      </div>
      {error && <ErrorBox error={error} onRetry={reload} />}
      {data && data.length === 0 && <Empty title="עוד לא קראת כלום" icon={<History size={30} strokeWidth={1.5} />}>פרקים שתפתח בקורא יופיעו כאן.</Empty>}
      {[...groups.entries()].map(([day, items]) => (
        <div key={day} className="section" style={{ marginTop: 22 }}>
          <h3 className="muted" style={{ marginBottom: 10 }}>{day}</h3>
          <div className="panel" style={{ padding: 0 }}>
            {items.map((h, i) => (
              <div key={`${h.chapter_id}-${i}`} className="list-row clickable" onClick={() => nav(`/read/${h.chapter_id}?page=${h.page_idx}`)}>
                <div style={{ width: 38 }}><Cover mangaId={h.manga_id} title="" /></div>
                <div className="col grow" style={{ gap: 4 }}>
                  <span className="ellipsis" dir="auto"><strong>{h.manga_title}</strong> · {h.chapter_title}</span>
                  <div style={{ maxWidth: 300 }}><Progress value={h.page_idx + 1} total={h.page_count} /></div>
                </div>
                <span className="muted small">עמוד {h.page_idx + 1}/{h.page_count}</span>
                <span className="faint small">{new Date(h.at).toLocaleTimeString("he-IL", { hour: "2-digit", minute: "2-digit" })}</span>
              </div>
            ))}
          </div>
        </div>
      ))}
      {confirm && (
        <Modal title="לנקות את היסטוריית הקריאה?" onClose={() => setConfirm(false)}
          actions={<><button className="btn ghost" onClick={() => setConfirm(false)}>ביטול</button>
            <button className="btn danger" onClick={() => api.clearHistory().then(() => { setConfirm(false); reload(); }).catch(toastError)}>ניקוי</button></>}>
          <p className="muted">ההתקדמות בכל פרק (העמוד האחרון שנקרא) נשמרת. רק רשימת ההיסטוריה נמחקת.</p>
        </Modal>
      )}
    </div>
  );
}
