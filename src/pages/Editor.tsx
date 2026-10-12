import { useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import {
  ArrowDown, ArrowRight, ArrowUp, Check, ChevronLeft, ChevronRight, EyeOff, Languages, RefreshCw, RotateCcw, ScanText, SquareDashed, Trash2,
} from "lucide-react";
import { api } from "../lib/api";
import { toast, toastError, useEvent, useLoad } from "../lib/store";
import { cleanUrl, pageUrl, previousUrl, statusLabel, translatedUrl } from "../lib/util";
import { Spinner } from "../components/ui";
import type { Box, EditorPage, Region } from "../lib/types";

type View = "translated" | "original" | "clean" | "previous";

const NOTE_TEXT: Record<string, string> = {
  overflow: "הטקסט לא נכנס במלואו לבועה",
  complex_background: "רקע מורכב: ייתכן שהניקוי לא מושלם",
  box_too_small: "האזור קטן מדי לטקסט",
  lama_unavailable: "מודל הניקוי לא זמין, נעשה ניקוי בסיסי",
  lama_failed: "הניקוי המתקדם נכשל, נעשה ניקוי בסיסי",
  no_text_pixels: "לא נמצאו פיקסלים של טקסט לניקוי",
  no_text: "לא זוהה טקסט בעמוד",
};

export default function EditorPageView() {
  const pageId = Number(useParams().pageId);
  const nav = useNavigate();
  const { data, reload, setData, error } = useLoad(() => api.editorPage(pageId), [pageId]);
  const chapter = useLoad(() => (data ? api.chapterView(data.chapter_id) : Promise.resolve(null)), [data?.chapter_id]);
  const [view, setView] = useState<View>("translated");
  const [sel, setSel] = useState<number | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [draw, setDraw] = useState(false);
  const [dragRect, setDragRect] = useState<Box | null>(null);
  const [scale, setScale] = useState(0.5);
  const canvasRef = useRef<HTMLDivElement>(null);

  useEvent<{ page_id: number }>("page-updated", (p) => p.page_id === pageId && reload());

  // fit the page to the canvas width
  useEffect(() => {
    if (!data || !canvasRef.current) return;
    const ro = new ResizeObserver(([e]) => setScale(Math.min(1, (e.contentRect.width - 40) / data.width, (e.contentRect.height - 70) / data.height * 1.6)));
    ro.observe(canvasRef.current);
    return () => ro.disconnect();
  }, [data?.width, data?.height, data]);

  const pages = chapter.data?.pages ?? [];
  const pos = pages.findIndex((p) => p.id === pageId);

  async function act<T>(label: string, fn: () => Promise<T>, after?: (v: T) => void) {
    setBusy(label);
    try {
      const v = await fn();
      after?.(v);
      await reload();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(null);
    }
  }

  const rerender = () => act("מעמד מחדש…", () => api.rerenderPage(pageId), () => setView("translated"));

  if (error) return <div className="page"><div className="callout bad">{error}</div></div>;
  if (!data) return <div className="page"><Spinner label="טוען…" /></div>;

  const rendered = !!data.state.rendered_path;
  const imgSrc =
    view === "translated" && rendered ? translatedUrl(pageId, data.state.render_key?.slice(0, 12) ?? null)
      : view === "clean" && data.state.clean_path ? cleanUrl(pageId, data.state.render_key?.slice(0, 12) ?? null)
      : view === "previous" && data.has_previous_render ? previousUrl(pageId)
      : pageUrl(pageId);

  const toImage = (e: React.MouseEvent<SVGSVGElement>): [number, number] => {
    const r = e.currentTarget.getBoundingClientRect();
    return [((e.clientX - r.left) / r.width) * data.width, ((e.clientY - r.top) / r.height) * data.height];
  };

  return (
    <div className="editor">
      <div className="editor-canvas" ref={canvasRef}>
        <div className="editor-toolbar">
          <div>
            <button className="btn ghost sm" onClick={() => nav(`/read/${data.chapter_id}?page=${data.idx}`)} title="חזרה לקורא"><ArrowRight size={14} /> לקורא</button>
            <div className="seg" role="group" aria-label="תצוגה">
              <button className={view === "translated" ? "on" : ""} onClick={() => setView("translated")} disabled={!rendered}>מתורגם</button>
              <button className={view === "original" ? "on" : ""} onClick={() => setView("original")}>מקור</button>
              <button className={view === "clean" ? "on" : ""} onClick={() => setView("clean")} disabled={!data.state.clean_path}>נקי</button>
              <button className={view === "previous" ? "on" : ""} onClick={() => setView("previous")} disabled={!data.has_previous_render}>גרסה קודמת</button>
            </div>
            <button className={`btn sm ${draw ? "primary" : "ghost"}`} onClick={() => setDraw(!draw)} title="סמן אזור טקסט שלא זוהה">
              <SquareDashed size={14} /> אזור חדש
            </button>
          </div>
        </div>
        <div className="stage" style={{ width: data.width * scale, height: data.height * scale }}>
          <img src={imgSrc} width={data.width * scale} height={data.height * scale} alt={`עמוד ${data.idx + 1}`} draggable={false} />
          <svg viewBox={`0 0 ${data.width} ${data.height}`} width={data.width * scale} height={data.height * scale}
            style={{ cursor: draw ? "crosshair" : "default" }}
            onMouseDown={(e) => { if (!draw) return; const [x, y] = toImage(e); setDragRect([x, y, x, y]); }}
            onMouseMove={(e) => { if (!dragRect) return; const [x, y] = toImage(e); setDragRect([dragRect[0], dragRect[1], x, y]); }}
            onMouseUp={async () => {
              if (!dragRect) return;
              const b: Box = [Math.min(dragRect[0], dragRect[2]), Math.min(dragRect[1], dragRect[3]), Math.max(dragRect[0], dragRect[2]), Math.max(dragRect[1], dragRect[3])].map(Math.round) as Box;
              setDragRect(null);
              setDraw(false);
              if (b[2] - b[0] < 8 || b[3] - b[1] < 8) return;
              await act("מזהה טקסט…", () => api.addRegion(pageId, b), (r) => setSel(r.id));
            }}>
            {view !== "translated" || !rendered ? data.regions.map((r) => (
              <RegionRect key={r.id} r={r} sel={sel === r.id} onClick={() => setSel(r.id)} />
            )) : data.regions.filter((r) => r.id === sel).map((r) => (
              <RegionRect key={r.id} r={r} sel onClick={() => setSel(r.id)} />
            ))}
            {dragRect && (
              <rect className="draw-rect" x={Math.min(dragRect[0], dragRect[2])} y={Math.min(dragRect[1], dragRect[3])}
                width={Math.abs(dragRect[2] - dragRect[0])} height={Math.abs(dragRect[3] - dragRect[1])} />
            )}
          </svg>
        </div>
      </div>

      <aside className="editor-side">
        <div className="head">
          <div className="spread">
            <h3>עמוד {data.idx + 1}{pages.length ? ` מתוך ${pages.length}` : ""}</h3>
            <div className="row" style={{ gap: 4 }}>
              <button className="btn icon sm" disabled={pos <= 0} onClick={() => nav(`/edit/${pages[pos - 1].id}`, { replace: true })} aria-label="העמוד הקודם"><ChevronRight size={16} /></button>
              <button className="btn icon sm" disabled={pos < 0 || pos >= pages.length - 1} onClick={() => nav(`/edit/${pages[pos + 1].id}`, { replace: true })} aria-label="העמוד הבא"><ChevronLeft size={16} /></button>
            </div>
          </div>
          <div className="row wrap" style={{ gap: 6 }}>
            <span className={`badge ${data.state.status === "done" ? "ok" : data.state.status === "failed" ? "bad" : ""}`}>{statusLabel[data.state.status] ?? data.state.status}</span>
            {data.state.model && <span className="badge ltr">{data.state.model}</span>}
            {data.state.cost_usd ? <span className="badge">${data.state.cost_usd.toFixed(3)}</span> : null}
            {busy && <Spinner label={busy} />}
          </div>
          {data.state.error && <div className="callout bad small">{data.state.error}</div>}
          {data.state.notes.length > 0 && (
            <div className="col small" style={{ gap: 4 }}>
              {data.state.notes.map((n, i) => (
                <span key={i} className="faint">{n.rid ? `${n.rid}: ` : ""}{NOTE_TEXT[n.kind] ?? n.kind}</span>
              ))}
            </div>
          )}
          <div className="row wrap" style={{ gap: 6 }}>
            <button className="btn sm primary" disabled={!!busy} onClick={rerender}><RefreshCw size={14} /> עימוד מחדש</button>
            <button className="btn sm" disabled={!!busy} onClick={() => act("נכנס לתור…", () => api.enqueue([data.chapter_id], { pageIds: [pageId], priority: 100 }), () => toast("העמוד בתור התרגום", "ok"))}>
              <Languages size={14} /> תרגום העמוד
            </button>
            <button className="btn sm ghost" disabled={!!busy} onClick={() => act("מזהה מחדש…", () => api.reanalyzePage(pageId), (d: EditorPage) => setData(d))} title="זיהוי בועות ו-OCR מחדש. תיקונים שאושרו נשמרים.">
              <ScanText size={14} /> זיהוי מחדש
            </button>
            <button className="btn sm ghost" disabled={!!busy || !rendered} onClick={() => act("משחזר…", () => api.restoreOriginal(pageId), () => setView("original"))}>
              <RotateCcw size={14} /> שחזור המקור
            </button>
          </div>
        </div>
        <div className="body">
          {data.regions.length === 0 ? (
            <div className="empty" style={{ margin: 16 }}>
              <h3>אין אזורי טקסט</h3>
              <p>{data.state.status === "none" ? "העמוד עוד לא עבר זיהוי. תרגם אותו או לחץ על זיהוי מחדש." : "לא זוהה טקסט. אפשר לסמן אזור ידנית."}</p>
            </div>
          ) : (
            data.regions.map((r, i) => (
              <RegionCard key={r.id} r={r} first={i === 0} last={i === data.regions.length - 1} selected={sel === r.id}
                onSelect={() => setSel(r.id)} act={act} onSaved={rerender} />
            ))
          )}
        </div>
      </aside>
    </div>
  );
}

function RegionRect({ r, sel, onClick }: { r: Region; sel: boolean; onClick: () => void }) {
  const [x1, y1, x2, y2] = r.layout.box ?? r.bbox;
  const cls = r.skip ? "skip" : r.approved ? "ok" : r.confidence === "low" || (r.ocr_confidence ?? 1) < 0.6 || !r.translation ? "low" : "";
  return (
    <g onClick={(e) => { e.stopPropagation(); onClick(); }}>
      <rect className={`region-box ${cls} ${sel ? "sel" : ""}`} x={x1} y={y1} width={x2 - x1} height={y2 - y1} rx={4} />
      <rect className="region-tag" x={x1} y={y1 - 22} width={34} height={20} rx={3} />
      <text className="region-label" x={x1 + 6} y={y1 - 7}>{r.rid}</text>
    </g>
  );
}

function RegionCard({ r, first, last, selected, onSelect, act, onSaved }: {
  r: Region; first: boolean; last: boolean; selected: boolean; onSelect: () => void;
  act: <T>(label: string, fn: () => Promise<T>, after?: (v: T) => void) => Promise<void>; onSaved: () => void;
}) {
  const [src, setSrc] = useState(r.source_text ?? r.ocr_text);
  const [tr, setTr] = useState(r.translation ?? "");
  const [size, setSize] = useState<string>(r.layout.font_size ? String(r.layout.font_size) : "");
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    setSrc(r.source_text ?? r.ocr_text);
    setTr(r.translation ?? "");
    setSize(r.layout.font_size ? String(r.layout.font_size) : "");
  }, [r.source_text, r.ocr_text, r.translation, r.layout.font_size]);
  useEffect(() => {
    if (selected) ref.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [selected]);

  const dirty = src !== (r.source_text ?? r.ocr_text) || tr !== (r.translation ?? "") || size !== (r.layout.font_size ? String(r.layout.font_size) : "");
  const lowOcr = (r.ocr_confidence ?? 1) < 0.6;

  async function save() {
    const edit: Parameters<typeof api.updateRegion>[1] = {};
    if (src !== (r.source_text ?? r.ocr_text)) edit.source_text = src;
    if (tr !== (r.translation ?? "")) edit.translation = tr;
    if (size !== (r.layout.font_size ? String(r.layout.font_size) : "")) edit.layout = { font_size: size ? Number(size) : null };
    await act("שומר…", () => api.updateRegion(r.id, edit), () => onSaved());
  }
  const nudge = (dx: number, dy: number) =>
    act("מזיז…", () => api.updateRegion(r.id, { layout: { dx: (r.layout.dx ?? 0) + dx, dy: (r.layout.dy ?? 0) + dy } }), () => onSaved());

  return (
    <div ref={ref} className={`region-card ${selected ? "sel" : ""}`} onClick={onSelect}>
      <div className="spread">
        <div className="row" style={{ gap: 6 }}>
          <span className="rid">{r.rid}</span>
          {r.category && <span className="badge">{({ dialogue: "דיבור", thought: "מחשבה", narration: "קריינות", sfx: "אפקט קולי", sign: "שלט", other: "אחר" } as Record<string, string>)[r.category] ?? r.category}</span>}
          {r.speaker && <span className="badge" dir="auto">{r.speaker}</span>}
          {r.approved && <span className="badge ok"><Check size={11} /> אושר</span>}
          {r.skip && <span className="badge">מושאר במקור</span>}
          {!r.approved && r.confidence === "low" && <span className="badge warn">לא ודאי</span>}
        </div>
        <div className="row" style={{ gap: 2 }}>
          <button className="btn ghost icon sm" disabled={first} onClick={() => act("…", () => api.moveRegion(r.id, -1))} aria-label="הקדם בסדר הקריאה"><ArrowUp size={14} /></button>
          <button className="btn ghost icon sm" disabled={last} onClick={() => act("…", () => api.moveRegion(r.id, 1))} aria-label="דחה בסדר הקריאה"><ArrowDown size={14} /></button>
        </div>
      </div>
      <label className="field">
        <span>טקסט המקור {lowOcr && <span className="badge warn">OCR לא ודאי</span>}</span>
        <textarea className="textarea src-text" rows={2} dir="auto" lang="ja" value={src} onChange={(e) => setSrc(e.target.value)} />
      </label>
      <label className="field">
        <span>תרגום</span>
        <textarea className="textarea he-text" rows={3} dir="rtl" lang="he" value={tr} onChange={(e) => setTr(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) save(); }} />
      </label>
      {r.note && <span className="small faint" dir="auto">הערת המתרגם: {r.note}</span>}
      {selected && (
        <>
          <div className="row wrap" style={{ gap: 6 }}>
            <button className="btn sm primary" disabled={!dirty} onClick={save}><Check size={14} /> שמירה ואישור</button>
            {!r.approved && r.translation && !dirty && (
              <button className="btn sm" onClick={() => act("מאשר…", () => api.updateRegion(r.id, { approved: true, translation: r.translation! }))}>אישור כמו שהוא</button>
            )}
            <button className="btn sm ghost" onClick={() => act("OCR…", () => api.retryRegionOcr(r.id))}><ScanText size={14} /> OCR מחדש</button>
            <button className="btn sm ghost" onClick={() => act("מתרגם…", () => api.retranslateRegion(r.id), () => onSaved())}><Languages size={14} /> תרגום מחדש</button>
          </div>
          <div className="row wrap" style={{ gap: 6 }}>
            <input className="input" style={{ width: 92, height: 28 }} type="number" min={6} max={300} placeholder="גודל" value={size}
              onChange={(e) => setSize(e.target.value)} aria-label="גודל גופן (פיקסלים)" title="גודל גופן ידני. ריק = אוטומטי" />
            <div className="seg">
              {(["right", "center", "left"] as const).map((a) => (
                <button key={a} className={(r.layout.align ?? "center") === a ? "on" : ""}
                  onClick={() => act("…", () => api.updateRegion(r.id, { layout: { align: a } }), () => onSaved())}>
                  {a === "right" ? "ימין" : a === "center" ? "מרכז" : "שמאל"}
                </button>
              ))}
            </div>
            <button className="btn ghost icon sm" onClick={() => nudge(0, -8)} aria-label="הזזה למעלה"><ArrowUp size={14} /></button>
            <button className="btn ghost icon sm" onClick={() => nudge(0, 8)} aria-label="הזזה למטה"><ArrowDown size={14} /></button>
            <button className="btn ghost icon sm" onClick={() => nudge(8, 0)} aria-label="הזזה ימינה"><ChevronLeft size={14} style={{ transform: "scaleX(-1)" }} /></button>
            <button className="btn ghost icon sm" onClick={() => nudge(-8, 0)} aria-label="הזזה שמאלה"><ChevronLeft size={14} /></button>
          </div>
          <div className="row wrap" style={{ gap: 6 }}>
            <button className="btn sm ghost" onClick={() => act("…", () => api.updateRegion(r.id, { skip: !r.skip }), () => onSaved())}>
              <EyeOff size={14} /> {r.skip ? "תרגם את האזור" : "השאר במקור"}
            </button>
            <button className="btn sm ghost danger" onClick={() => act("מוחק…", () => api.deleteRegion(r.id), () => onSaved())}><Trash2 size={14} /> מחיקת האזור</button>
          </div>
          {r.history.length > 0 && (
            <details>
              <summary className="small muted" style={{ cursor: "pointer" }}>גרסאות קודמות ({r.history.length})</summary>
              <div className="col" style={{ gap: 4, marginTop: 6 }}>
                {[...r.history].reverse().map((h, i) => (
                  <div key={i} className="spread small">
                    <span dir="rtl" className="grow">{h.text}</span>
                    <button className="btn ghost sm" onClick={() => setTr(h.text)}>שחזור</button>
                  </div>
                ))}
              </div>
            </details>
          )}
        </>
      )}
    </div>
  );
}

