import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowRight, ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight, Columns2, Expand, GalleryVerticalEnd, Languages, LayoutPanelLeft,
  Maximize, PenLine, RectangleVertical, X, ZoomIn, ZoomOut,
} from "lucide-react";
import { api } from "../lib/api";
import { toast, toastError, useApp, useEvent, useLoad, useThrottled } from "../lib/store";
import { arrowStep, pageUrl, spreads, statusLabel, thumbUrl, translatedUrl } from "../lib/util";
import { TranslateFlow } from "../components/TranslateFlow";
import { Progress } from "../components/ui";
import type { ChapterView, Job, PageInfo } from "../lib/types";

type Mode = "single" | "double" | "vertical";
type Dir = "rtl" | "ltr";
type Fit = "height" | "width" | "original";

function useStageSize(ref: React.RefObject<HTMLDivElement | null>) {
  const [size, setSize] = useState({ w: 1000, h: 800 });
  useLayoutEffect(() => {
    if (!ref.current) return;
    const ro = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(ref.current);
    return () => ro.disconnect();
  }, [ref]);
  return size;
}

export default function ReaderPage() {
  const chapterId = Number(useParams().chapterId);
  const [params] = useSearchParams();
  const nav = useNavigate();
  const { settings, saveSettings } = useApp();
  const { data: view, reload } = useLoad(() => api.chapterView(chapterId), [chapterId]);
  const refresh = useThrottled(reload, 600);
  useEvent<{ chapter_id: number }>("page-updated", (p) => p.chapter_id === chapterId && refresh());
  // translation job for this chapter, shown inside the reader
  const [job, setJob] = useState<Job | null>(null);
  const [bannerHidden, setBannerHidden] = useState(false);
  useEffect(() => {
    setJob(null);
    setBannerHidden(false);
    api.jobs(false).then((js) => setJob(js.find((j) => j.chapter_id === chapterId && ["queued", "running", "paused"].includes(j.status)) ?? null)).catch(() => {});
  }, [chapterId]);
  useEvent<Job>("job-updated", (j) => {
    if (j.chapter_id !== chapterId) return;
    setJob(["queued", "running", "paused"].includes(j.status) ? j : null);
    refresh();
  });

  const rs = settings!.reader;
  const [mode, setMode] = useState<Mode>(rs.mode);
  const [dir, setDir] = useState<Dir>(rs.direction);
  const [fit, setFit] = useState<Fit>(rs.fit);
  const [translated, setTranslated] = useState(rs.show_translated);
  const [compare, setCompare] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [idx, setIdx] = useState(0);
  const [chrome, setChrome] = useState(true);
  const [thumbs, setThumbs] = useState(false);
  const [translateOpen, setTranslateOpen] = useState(false);
  const stageRef = useRef<HTMLDivElement>(null);
  const size = useStageSize(stageRef);
  const hideTimer = useRef<number>(0);
  const initialised = useRef<number | null>(null);

  // per-manga overrides + initial page
  useEffect(() => {
    if (!view || initialised.current === chapterId) return;
    initialised.current = chapterId;
    // per-manga override, else the global default (don't carry over from another manga)
    setMode((view.reading_mode as Mode) ?? useApp.getState().settings!.reader.mode);
    setDir((view.reading_dir as Dir) ?? useApp.getState().settings!.reader.direction);
    const p = Number(params.get("page") ?? view.chapter.last_page ?? 0);
    const n = view.pages.length;
    setIdx(p < 0 ? Math.max(0, n - 1) : Math.min(Math.max(0, p), Math.max(0, n - 1)));
    setZoom(1);
  }, [view, chapterId, params]);

  const pages = view?.pages ?? [];
  const groups = useMemo(
    () => (mode === "double" ? spreads(pages, rs.double_cover_alone) : pages.map((_, i) => [i])),
    [pages, mode, rs.double_cover_alone],
  );
  const gIdx = Math.max(0, groups.findIndex((g) => g.includes(idx)));
  const group = groups[gIdx] ?? [];

  // save progress (debounced)
  useEffect(() => {
    if (!view || initialised.current !== chapterId) return;
    const last = mode === "double" ? Math.max(...group, idx) : idx;
    const t = window.setTimeout(() => api.saveProgress(chapterId, last).catch(() => {}), 500);
    return () => window.clearTimeout(t);
  }, [idx, chapterId, view, mode, group]);

  const src = useCallback(
    (p: PageInfo) => (translated && p.translated_version ? translatedUrl(p.id, p.translated_version) : pageUrl(p.id)),
    [translated],
  );

  // preload upcoming pages
  useEffect(() => {
    if (mode === "vertical") return;
    const ahead = groups.slice(gIdx + 1, gIdx + 1 + Math.max(1, rs.preload)).flat();
    const behind = groups[gIdx - 1] ?? [];
    const imgs = [...ahead, ...behind].map((i) => {
      const im = new Image();
      im.decoding = "async";
      im.src = src(pages[i]);
      return im;
    });
    return () => imgs.forEach((im) => (im.src = ""));
  }, [gIdx, groups, pages, src, mode, rs.preload]);

  const go = useCallback(
    (step: number) => {
      if (!view) return;
      if (mode === "vertical") {
        const el = stageRef.current;
        if (el) el.scrollBy({ top: step * el.clientHeight * 0.85, behavior: "smooth" });
        return;
      }
      const ng = gIdx + step;
      if (ng < 0) {
        if (view.prev_chapter_id) nav(`/read/${view.prev_chapter_id}?page=-1`, { replace: true });
        else toast("זה העמוד הראשון");
        return;
      }
      if (ng >= groups.length) {
        if (view.next_chapter_id) nav(`/read/${view.next_chapter_id}?page=0`, { replace: true });
        else toast("סוף הפרק האחרון");
        return;
      }
      setIdx(groups[ng][0]);
      setZoom(1);
      stageRef.current?.scrollTo({ top: 0 });
    },
    [view, mode, gIdx, groups, nav],
  );

  const exit = useCallback(async () => {
    const w = getCurrentWindow();
    if (await w.isFullscreen()) await w.setFullscreen(false);
    nav(view ? `/manga/${view.chapter.manga_id}` : "/library");
  }, [nav, view]);

  const toggleFullscreen = useCallback(async () => {
    const w = getCurrentWindow();
    await w.setFullscreen(!(await w.isFullscreen()));
  }, []);

  async function persist(patch: Partial<{ mode: Mode; dir: Dir; fit: Fit; translated: boolean }>) {
    if (!view) return;
    if (patch.mode || patch.dir) {
      api.updateManga(view.chapter.manga_id, {
        ...(patch.mode ? { reading_mode: patch.mode } : {}),
        ...(patch.dir ? { reading_dir: patch.dir } : {}),
      }).catch(toastError);
    }
    if (patch.fit !== undefined || patch.translated !== undefined) {
      const s = useApp.getState().settings!;
      saveSettings({ ...s, reader: { ...s.reader, fit: patch.fit ?? s.reader.fit, show_translated: patch.translated ?? s.reader.show_translated } }).catch(toastError);
    }
  }
  const changeMode = (m: Mode) => { setMode(m); persist({ mode: m }); };
  const changeDir = (d: Dir) => { setDir(d); persist({ dir: d }); };
  const changeFit = (f: Fit) => { setFit(f); setZoom(1); persist({ fit: f }); };
  const toggleTranslated = () => { setTranslated((t) => { persist({ translated: !t }); return !t; }); };

  // keyboard
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (translateOpen) return;
      if ((e.target as HTMLElement)?.tagName === "INPUT") return;
      const step = arrowStep(e.key, dir);
      if (step && mode !== "vertical") { e.preventDefault(); go(step); return; }
      switch (e.key) {
        case " ": case "PageDown": e.preventDefault(); go(e.shiftKey ? -1 : 1); break;
        case "PageUp": e.preventDefault(); go(-1); break;
        case "ArrowDown": if (mode === "vertical") return; e.preventDefault(); go(1); break;
        case "ArrowUp": if (mode === "vertical") return; e.preventDefault(); go(-1); break;
        case "Home": setIdx(0); break;
        case "End": setIdx(Math.max(0, pages.length - 1)); break;
        case "Escape": exit(); break;
        case "f": case "F": toggleFullscreen(); break;
        case "t": case "T": toggleTranslated(); break;
        case "c": case "C": setCompare((c) => !c); break;
        case "+": case "=": setZoom((z) => Math.min(4, z * 1.2)); break;
        case "-": setZoom((z) => Math.max(0.3, z / 1.2)); break;
        case "0": setZoom(1); break;
        case "g": case "G": setThumbs((t) => !t); break;
        case "e": case "E": if (pages[idx]) nav(`/edit/${pages[idx].id}`); break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [go, dir, mode, pages, idx, exit, toggleFullscreen, translateOpen]);

  // auto-hide chrome
  const poke = useCallback(() => {
    setChrome(true);
    window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => setChrome(false), 2600);
  }, []);
  useEffect(() => {
    poke();
    return () => window.clearTimeout(hideTimer.current);
  }, [poke]);

  if (!view || !settings) return <div className="reader" />;
  const page = pages[idx];

  function onStageClick(e: React.MouseEvent) {
    if (mode === "vertical") { setChrome((c) => !c); return; }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = (e.clientX - r.left) / r.width;
    if (x < 0.33) go(arrowStep("ArrowLeft", dir));
    else if (x > 0.67) go(arrowStep("ArrowRight", dir));
    else setChrome((c) => !c);
  }

  const showCompare = compare && mode !== "vertical" && group.length === 1 && !!page?.translated_version;

  return (
    <div className="reader" onMouseMove={poke}>
      <div className={`reader-bar top ${chrome ? "" : "hidden"}`} data-tauri-drag-region>
        <button className="btn icon" onClick={exit} aria-label="יציאה מהקורא"><ArrowRight size={18} /></button>
        <div className="grow ellipsis" dir="auto" data-tauri-drag-region>
          <strong>{view.manga_title}</strong> <span className="muted">· {view.chapter.title}</span>
        </div>
        {page && (
          <span className={`version-tag ${translated && page.translated_version ? "he" : ""}`} aria-live="polite">
            {translated && page.translated_version ? "מוצג: עברית" : translated ? "מוצג: מקור (העמוד עוד לא תורגם)" : "מוצג: מקור"}
          </span>
        )}
        <div className="row" style={{ gap: 6 }}>
          <button className={`btn sm ${translated ? "on" : ""}`} onClick={toggleTranslated} title="מקור / מתורגם (T)" aria-pressed={translated}>
            <Languages size={15} /> {translated ? "מתורגם" : "מקור"}
          </button>
          <button className={`btn icon sm ${compare ? "on" : ""}`} onClick={() => setCompare(!compare)} title="השוואה זה לצד זה (C)" aria-pressed={compare}>
            <LayoutPanelLeft size={15} />
          </button>
          <span style={{ width: 8 }} />
          <button className={`btn icon sm ${mode === "single" ? "on" : ""}`} onClick={() => changeMode("single")} title="עמוד בודד" aria-label="עמוד בודד"><RectangleVertical size={15} /></button>
          <button className={`btn icon sm ${mode === "double" ? "on" : ""}`} onClick={() => changeMode("double")} title="כפולת עמודים" aria-label="כפולת עמודים"><Columns2 size={15} /></button>
          <button className={`btn icon sm ${mode === "vertical" ? "on" : ""}`} onClick={() => changeMode("vertical")} title="גלילה אנכית" aria-label="גלילה אנכית"><GalleryVerticalEnd size={15} /></button>
          <span style={{ width: 8 }} />
          <button className="btn sm" onClick={() => changeDir(dir === "rtl" ? "ltr" : "rtl")} title="כיוון דפדוף">
            {dir === "rtl" ? "דפדוף ←" : "דפדוף →"}
          </button>
          <select className="btn sm" value={fit} onChange={(e) => changeFit(e.target.value as Fit)} aria-label="התאמה" style={{ paddingInline: 8 }}>
            <option value="height">התאמה למסך</option>
            <option value="width">התאמה לרוחב</option>
            <option value="original">גודל מקורי</option>
          </select>
          <button className="btn icon sm" onClick={() => setZoom((z) => Math.max(0.3, z / 1.2))} aria-label="הקטנה"><ZoomOut size={15} /></button>
          <button className="btn icon sm" onClick={() => setZoom((z) => Math.min(4, z * 1.2))} aria-label="הגדלה"><ZoomIn size={15} /></button>
          <button className="btn icon sm" onClick={toggleFullscreen} title="מסך מלא (F)" aria-label="מסך מלא"><Maximize size={15} /></button>
        </div>
      </div>

      <TranslationBanner
        view={view} job={job} hidden={bannerHidden} chrome={chrome} onHide={() => setBannerHidden(true)}
        onTranslate={() => setTranslateOpen(true)}
        onRetry={async () => {
          try {
            await api.enqueue([chapterId], { priority: 50 });
            await api.prioritizeChapter(chapterId);
            toast("העמודים שחסרים נכנסו לתרגום", "ok");
          } catch (e) { toastError(e); }
        }}
      />

      {page && page.status !== "done" && page.status !== "none" && chrome && (
        <div className="page-status">
          <span className={`badge ${page.status === "failed" ? "bad" : "run"}`} title={page.error ?? undefined}>
            {statusLabel[page.status] ?? page.status}{page.status === "failed" && page.error ? `: ${page.error.slice(0, 80)}` : ""}
          </span>
        </div>
      )}

      <div
        ref={stageRef}
        className={`reader-stage ${mode === "vertical" ? "" : "paged"}`}
        onClick={onStageClick}
        onWheel={(e) => {
          if (e.ctrlKey) { e.preventDefault(); setZoom((z) => Math.min(4, Math.max(0.3, z * (e.deltaY < 0 ? 1.08 : 0.92)))); }
        }}
        tabIndex={0}
      >
        {mode === "vertical" ? (
          <VerticalPages view={view} src={src} zoom={zoom} width={size.w} gap={rs.vertical_gap} startIdx={idx} onVisible={setIdx} />
        ) : showCompare ? (
          <PagedImages pages={[page, page]} srcs={[pageUrl(page.id), translatedUrl(page.id, page.translated_version)]} labels={["מקור", "מתורגם"]} dir={dir} fit={fit} zoom={zoom} size={size} />
        ) : (
          <PagedImages pages={group.map((i) => pages[i])} srcs={group.map((i) => src(pages[i]))} dir={dir} fit={fit} zoom={zoom} size={size} />
        )}
      </div>

      <div className={`reader-bar bottom ${chrome ? "" : "hidden"}`}>
        {thumbs && (
          <div className="thumbs" dir={dir}>
            {pages.map((p, i) => (
              <button key={p.id} className={group.includes(i) ? "on" : ""} onClick={() => setIdx(i)} aria-label={`עמוד ${i + 1}`}>
                <img src={thumbUrl(p.id)} loading="lazy" alt="" />
                {p.status === "done" && <span className="tdot dot" style={{ color: "#f5cf4a" }} />}
                {p.status === "failed" && <span className="tdot dot" style={{ color: "#f07272" }} />}
              </button>
            ))}
          </div>
        )}
        <div className="row">
          <button className="btn icon sm" disabled={!view.prev_chapter_id} onClick={() => nav(`/read/${view.prev_chapter_id}?page=0`, { replace: true })}
            title="הפרק הקודם" aria-label="הפרק הקודם">{dir === "rtl" ? <ChevronsRight size={15} /> : <ChevronsLeft size={15} />}</button>
          <button className="btn icon sm" onClick={() => go(-1)} title="העמוד הקודם" aria-label="העמוד הקודם">
            {dir === "rtl" ? <ChevronRight size={15} /> : <ChevronLeft size={15} />}</button>
          <button className="btn icon sm" onClick={() => go(1)} title="העמוד הבא" aria-label="העמוד הבא">
            {dir === "rtl" ? <ChevronLeft size={15} /> : <ChevronRight size={15} />}</button>
          <button className="btn icon sm" disabled={!view.next_chapter_id} onClick={() => nav(`/read/${view.next_chapter_id}?page=0`, { replace: true })}
            title="הפרק הבא" aria-label="הפרק הבא">{dir === "rtl" ? <ChevronsLeft size={15} /> : <ChevronsRight size={15} />}</button>
          <bdi className="muted small nowrap" dir="ltr">
            {group.length > 1 ? `${group[0] + 1}–${group[group.length - 1] + 1}` : idx + 1} / {pages.length}
          </bdi>
          <input type="range" min={0} max={Math.max(0, pages.length - 1)} value={idx} dir={dir}
            onChange={(e) => setIdx(Number(e.target.value))} aria-label="מעבר לעמוד" />
          <button className={`btn sm ${thumbs ? "on" : ""}`} onClick={() => setThumbs(!thumbs)} title="תמונות ממוזערות (G)"><Expand size={14} /> עמודים</button>
          {page && (
            <button className="btn sm" onClick={async () => {
              try {
                await api.enqueue([chapterId], { pageIds: [page.id], priority: 100 });
                toast("העמוד נכנס לראש תור התרגום", "ok");
              } catch (e) { toastError(e); }
            }} title="תרגום העמוד הנוכחי">
              <Languages size={14} /> עמוד
            </button>
          )}
          <button className="btn sm" onClick={() => setTranslateOpen(true)}><Languages size={14} /> פרק</button>
          {page && <button className="btn sm" onClick={() => nav(`/edit/${page.id}`)} title="עורך התרגום (E)"><PenLine size={14} /> עריכה</button>}
        </div>
      </div>

      {translateOpen && (
        <TranslateFlow chapters={[view.chapter]} preselect={[chapterId]}
          onClose={() => { setTranslateOpen(false); api.prioritizeChapter(chapterId).catch(() => {}); }} />
      )}
    </div>
  );
}

function PagedImages({ pages, srcs, labels, dir, fit, zoom, size }: {
  pages: PageInfo[]; srcs: string[]; labels?: string[]; dir: Dir; fit: Fit; zoom: number; size: { w: number; h: number };
}) {
  const totalW = pages.reduce((s, p) => s + (p.width || 1000), 0);
  const maxH = Math.max(...pages.map((p) => p.height || 1400));
  // "height" fits the whole spread on screen; "width" fills the width (scroll vertically)
  const fitScale = fit === "height" ? Math.min(size.h / maxH, size.w / totalW) : fit === "width" ? size.w / totalW : 1;
  const s = fitScale * zoom;
  return (
    <div className="reader-pages" dir={dir}>
      {pages.map((p, i) => (
        <div key={`${p.id}-${i}`} style={{ position: "relative" }}>
          <img src={srcs[i]} width={Math.round((p.width || 1000) * s)} height={Math.round((p.height || 1400) * s)} alt={`עמוד ${p.idx + 1}`} decoding="async" draggable={false} />
          {labels && <span className="compare-label">{labels[i]}</span>}
        </div>
      ))}
    </div>
  );
}

function VerticalPages({ view, src, zoom, width, gap, startIdx, onVisible }: {
  view: ChapterView; src: (p: PageInfo) => string; zoom: number; width: number; gap: number; startIdx: number; onVisible: (i: number) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const colW = Math.min(width, 900) * zoom;
  const scrolled = useRef(false);
  useEffect(() => {
    const root = ref.current?.parentElement;
    if (!root || !ref.current) return;
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) if (e.isIntersecting) onVisible(Number((e.target as HTMLElement).dataset.idx));
      },
      { root, rootMargin: "-45% 0px -45% 0px" },
    );
    ref.current.querySelectorAll(".vpage").forEach((el) => io.observe(el));
    if (!scrolled.current && startIdx > 0) {
      scrolled.current = true;
      (ref.current.children[startIdx] as HTMLElement | undefined)?.scrollIntoView();
    }
    return () => io.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view.pages.length, colW]);
  return (
    <div ref={ref} className="reader-vertical" style={{ width: colW, gap }}>
      {view.pages.map((p, i) => (
        <div key={p.id} className="vpage" data-idx={i} style={{ height: colW * ((p.height || 1400) / (p.width || 1000)) }}>
          <img src={src(p)} loading="lazy" decoding="async" alt={`עמוד ${i + 1}`} draggable={false} />
        </div>
      ))}
    </div>
  );
}

/** Translation state of this chapter, right in the reader: start, progress, partial results, retry. */
function TranslationBanner({ view, job, hidden, chrome, onHide, onTranslate, onRetry }: {
  view: ChapterView; job: Job | null; hidden: boolean; chrome: boolean; onHide: () => void; onTranslate: () => void; onRetry: () => void;
}) {
  const t = view.chapter.translation;
  const total = view.pages.length;
  if (job) {
    return (
      <div className="reader-banner">
        <div>
          <Languages size={16} />
          <div className="col" style={{ gap: 4, minWidth: 220 }}>
            <span className="small">{job.status === "queued" ? "ממתין בתור התרגום…" : job.status === "paused" ? "התרגום מושהה" : `מתרגם לעברית… ${job.completed}/${job.total} עמודים`}</span>
            <Progress value={job.completed + job.failed} total={job.total} />
          </div>
          <span className="small muted">אפשר להמשיך לקרוא; עמודים מתורגמים יופיעו לבד</span>
        </div>
      </div>
    );
  }
  if (hidden || !chrome) return null;
  if (t.done === 0 && t.failed === 0) {
    return (
      <div className="reader-banner">
        <div>
          <span>הפרק הזה עוד לא תורגם.</span>
          <button className="btn sm on" onClick={onTranslate}><Languages size={14} /> תרגם לעברית</button>
          <button className="btn icon sm" onClick={onHide} aria-label="הסתרה"><X size={14} /></button>
        </div>
      </div>
    );
  }
  if (t.done < total) {
    return (
      <div className="reader-banner">
        <div>
          <span>{t.done}/{total} עמודים תורגמו{t.failed ? ` · ${t.failed} נכשלו` : ""}.</span>
          <button className="btn sm on" onClick={onRetry}>{t.failed ? "נסה שוב לשאר העמודים" : "תרגם את השאר"}</button>
          <button className="btn icon sm" onClick={onHide} aria-label="הסתרה"><X size={14} /></button>
        </div>
      </div>
    );
  }
  return null;
}
