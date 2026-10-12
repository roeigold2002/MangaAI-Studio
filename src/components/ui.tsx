import { ReactNode, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { BookOpen } from "lucide-react";
import { coverUrl } from "../lib/util";
import { useApp } from "../lib/store";
import type { MangaSummary } from "../lib/types";

export function Modal({ title, children, onClose, actions }: { title: string; children: ReactNode; onClose: () => void; actions?: ReactNode }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div className="modal-bg" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-modal="true" aria-label={title}>
        <h2>{title}</h2>
        {children}
        {actions && <div className="actions">{actions}</div>}
      </div>
    </div>
  );
}

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`}>{t.text}</div>
      ))}
    </div>
  );
}

export function Empty({ title, children, icon, action }: { title: string; children?: ReactNode; icon?: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty tone">
      {icon ?? <BookOpen size={30} strokeWidth={1.5} />}
      <h3>{title}</h3>
      {children && <p>{children}</p>}
      {action}
    </div>
  );
}

export function Progress({ value, total, ok }: { value: number; total: number; ok?: boolean }) {
  const pct = total > 0 ? Math.min(100, (value / total) * 100) : 0;
  return (
    <div className={`progress ${ok ? "ok" : ""}`} role="progressbar" aria-valuenow={value} aria-valuemax={total}>
      <div style={{ width: `${pct}%` }} />
    </div>
  );
}

export function Spinner({ label }: { label?: string }) {
  return (
    <span className="row muted" role="status">
      <span className="spinner" /> {label}
    </span>
  );
}

export function ErrorBox({ error, onRetry }: { error: string; onRetry?: () => void }) {
  return (
    <div className="callout bad col">
      <span>{error}</span>
      {onRetry && (
        <div>
          <button className="btn sm" onClick={onRetry}>נסה שוב</button>
        </div>
      )}
    </div>
  );
}

export function Cover({ mangaId, title, children, version }: { mangaId: number; title: string; children?: ReactNode; version?: string }) {
  const [failed, setFailed] = useState(false);
  return (
    <div className="cover">
      {!failed ? (
        <img src={coverUrl(mangaId, version)} alt="" loading="lazy" decoding="async" onError={() => setFailed(true)} />
      ) : (
        <div className="fallback tone">{title}</div>
      )}
      {children}
    </div>
  );
}

export function MangaCard({ m }: { m: MangaSummary }) {
  const nav = useNavigate();
  const unread = m.chapter_count - m.read_count;
  const open = () => nav(`/manga/${m.id}`);
  return (
    <div className="cover-card" role="link" tabIndex={0} onClick={open} onKeyDown={(e) => e.key === "Enter" && open()}>
      <Cover mangaId={m.id} title={m.title}>
        <div className="flags">
          {m.translated_chapters > 0 ? (
            <span className="badge mark">עברית · {m.translated_chapters} פרקים</span>
          ) : m.translated_pages > 0 ? (
            <span className="badge mark">עברית · חלקי</span>
          ) : null}
          {m.failed_pages > 0 && <span className="badge">{m.failed_pages} עמ׳ נכשלו</span>}
          {m.downloaded_chapters > 0 && <span className="badge">הורד</span>}
        </div>
        {m.chapter_count > 0 && (
          <div className="bar">
            <div style={{ width: `${(m.read_count / m.chapter_count) * 100}%` }} />
          </div>
        )}
      </Cover>
      <div>
        <div className="cover-title" dir="auto">{m.title}</div>
        <div className="cover-sub">
          {m.read_count > 0 ? `${m.read_count}/${m.chapter_count} נקראו` : `${m.chapter_count} פרקים`}
          {unread > 0 && m.read_count > 0 ? ` · ${unread} לא נקראו` : ""}
        </div>
        {m.continue_chapter_id && (
          <div className="card-actions">
            <button className="btn sm primary" onClick={(e) => { e.stopPropagation(); nav(`/read/${m.continue_chapter_id}?page=${m.continue_page ?? 0}`); }}>
              המשך קריאה
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
      {hint && <span className="hint">{hint}</span>}
    </label>
  );
}
