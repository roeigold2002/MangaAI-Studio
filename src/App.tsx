import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { HashRouter, Navigate, NavLink, Route, Routes, useLocation } from "react-router-dom";
import { BookMarked, Download, Home, ListChecks, Search, Settings as Cog } from "lucide-react";
import { toast, useApp, useEvent } from "./lib/store";
import { api } from "./lib/api";
import { Toasts } from "./components/ui";
import { UpdatePrompt } from "./components/UpdatePrompt";
import type { CheckReport } from "./lib/types";
import { checkOnStartup } from "./lib/updates";
import HomePage from "./pages/Home";
import LibraryPage from "./pages/Library";
import MangaPage from "./pages/Manga";
import ReaderPage from "./pages/Reader";
import EditorPage from "./pages/Editor";
import BrowsePage from "./pages/Browse";
import SearchPage from "./pages/Search";
import TitlePage from "./pages/Title";
import DownloadsPage from "./pages/Downloads";
import QueuePage from "./pages/Queue";
import HistoryPage from "./pages/History";
import SettingsPage from "./pages/Settings";

function Sidebar() {
  const [active, setActive] = useState({ jobs: 0, downloads: 0 });
  const refresh = () =>
    Promise.all([api.jobs(false), api.downloads()])
      .then(([j, d]) =>
        setActive({
          jobs: j.filter((x) => x.status === "queued" || x.status === "running").length,
          downloads: d.filter((x) => x.status === "queued" || x.status === "running").length,
        }),
      )
      .catch(() => {});
  useEffect(() => {
    refresh();
  }, []);
  useEvent("job-updated", refresh);
  useEvent<{ status: string; manga_title: string; chapter_title: string; error: string | null }>("download-updated", (d) => {
    refresh();
    // completion indicator wherever the user is
    if (d.status === "done") toast(`הורד: ${d.manga_title} · ${d.chapter_title}. נמצא ב"הספרייה שלי".`, "ok");
    if (d.status === "failed") toast(`ההורדה נכשלה: ${d.chapter_title} (${d.error ?? ""})`, "bad");
  });
  useEvent<CheckReport>("new-chapters", (r) => {
    for (const n of r.new) {
      const what = n.chapters.length === 1 ? n.chapters[0] : `${n.chapters.length} פרקים חדשים`;
      toast(`${n.title}: ${what}${n.queued ? " (בהורדה)" : ""}`, "ok");
    }
  });
  useEvent<{ status: string; manga_title: string; chapter_title: string; failed: number }>("job-updated", (j) => {
    if (j.status === "done") toast(`התרגום הסתיים: ${j.manga_title} · ${j.chapter_title}`, "ok");
    if (j.status === "failed" && j.failed > 0) toast(`התרגום הסתיים עם ${j.failed} עמודים שנכשלו: ${j.chapter_title}`, "bad");
  });

  const item = (to: string, label: string, icon: React.ReactNode, badge?: number) => (
    <NavLink to={to} end={to === "/"} className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}>
      {icon}
      <span className="nav-label">{label}</span>
      {badge ? <span className="nav-badge">{badge}</span> : null}
    </NavLink>
  );
  return (
    <nav className="sidebar" aria-label="ניווט ראשי">
      <div className="brand">
        <div className="brand-mark" aria-hidden>מ</div>
        <div className="brand-name">MangaAI Studio</div>
      </div>
      {item("/", "בית", <Home size={18} />)}
      {item("/search", "חיפוש", <Search size={18} />)}
      {item("/library", "הספרייה שלי", <BookMarked size={18} />)}
      <div className="nav-sep" />
      {item("/downloads", "הורדות", <Download size={18} />, active.downloads)}
      {item("/queue", "תור תרגום", <ListChecks size={18} />, active.jobs)}
      <div className="nav-sep" />
      {item("/settings", "הגדרות", <Cog size={18} />)}
    </nav>
  );
}

function Layout() {
  const loc = useLocation();
  const fullscreen = loc.pathname.startsWith("/read/");
  const editor = loc.pathname.startsWith("/edit/");
  if (fullscreen) {
    return (
      <Routes>
        <Route path="/read/:chapterId" element={<ReaderPage />} />
      </Routes>
    );
  }
  return (
    <div className="shell">
      <div className="titlebar-drag" data-tauri-drag-region />
      <Sidebar />
      <main className="main" style={editor ? { overflow: "hidden" } : undefined}>
        <Routes>
          <Route path="/" element={<HomePage />} />
          <Route path="/search" element={<SearchPage />} />
          <Route path="/title/:ref" element={<TitlePage />} />
          <Route path="/library" element={<LibraryPage />} />
          <Route path="/favorites" element={<Navigate to="/library?tab=favorites" replace />} />
          <Route path="/manga/:id" element={<MangaPage />} />
          <Route path="/edit/:pageId" element={<EditorPage />} />
          <Route path="/browse" element={<BrowsePage />} />
          <Route path="/downloads" element={<DownloadsPage />} />
          <Route path="/queue" element={<QueuePage />} />
          <Route path="/history" element={<HistoryPage />} />
          <Route path="/translation" element={<Navigate to="/settings?tab=translation" replace />} />
          <Route path="/settings" element={<SettingsPage />} />
        </Routes>
      </main>
    </div>
  );
}

/** Debug builds only (MANGAAI_START_ROUTE). "route" opens a page; "tour:" runs steps separated by ";":
 *  go:<route>  click:<visible text>  type:<placeholder part>=<text>  wait:<text>[|timeout ms]  shot:<name>  sleep:<ms> */
async function devStart(spec: string) {
  if (!spec.startsWith("tour:")) {
    window.location.hash = spec;
    return;
  }
  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
  const log = (m: string) => invoke("dev_log", { message: m }).catch(() => {});
  const visible = (el: Element) => (el as HTMLElement).offsetParent !== null || (el as HTMLElement).getClientRects().length > 0;
  const findByText = (text: string) => {
    // buttons and links first, then cards/rows/labels; later in the DOM (e.g. an open dialog) wins
    for (const sel of ["button, a, [role=link]", "label, .cover-card, .list-row"]) {
      const els = [...document.querySelectorAll(sel)].filter(visible).filter((el) => !(el as HTMLButtonElement).disabled);
      const hit = els.reverse().find((el) => (el.textContent ?? "").includes(text));
      if (hit) return hit as HTMLElement;
    }
    return undefined;
  };
  await wait(2000);
  for (const step of spec.slice(5).split(";").map((x) => x.trim()).filter(Boolean)) {
    const [cmd, ...rest] = step.split(":");
    const arg = rest.join(":");
    try {
      if (cmd === "go") window.location.hash = arg;
      else if (cmd === "sleep") await wait(Number(arg));
      else if (cmd === "shot") await invoke("dev_snapshot", { path: `/tmp/mangaai-shots/${arg}.png` });
      else if (cmd === "click") {
        let el: HTMLElement | undefined;
        for (let i = 0; i < 40 && !(el = findByText(arg)); i++) await wait(250);
        if (!el) throw new Error(`no element with text "${arg}"`);
        el.click();
      } else if (cmd === "type") {
        const [ph, text] = arg.split("=");
        const input = [...document.querySelectorAll("input")].find((i) => (i.placeholder ?? "").includes(ph)) as HTMLInputElement | undefined;
        if (!input) throw new Error(`no input with placeholder "${ph}"`);
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, text);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      } else if (cmd === "wait") {
        const [text, ms] = arg.split("|");
        const until = Date.now() + Number(ms ?? 30000);
        while (!document.body.innerText.includes(text) && Date.now() < until) await wait(300);
        if (!document.body.innerText.includes(text)) throw new Error(`timeout waiting for "${text}"`);
      }
      await log(`ok ${step}`);
      await wait(700);
    } catch (e) {
      await log(`FAIL ${step}: ${e}`);
      await invoke("dev_snapshot", { path: `/tmp/mangaai-shots/FAIL-${Date.now()}.png` }).catch(() => {});
      return;
    }
  }
  await log("tour finished");
}

export default function App() {
  const { settings, loadSettings } = useApp();
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    loadSettings().catch((e) => setError(String(e?.message ?? e)));
    invoke<string | null>("dev_start_route").then((r) => { if (r) devStart(r); }).catch(() => {});
    // `npm run app:dev` builds are not installed, so there is nothing to update
    if (!import.meta.env.DEV) checkOnStartup();
  }, [loadSettings]);
  if (error) {
    return (
      <div className="page">
        <h1>האפליקציה לא הצליחה להיפתח</h1>
        <p className="callout bad" style={{ marginTop: 16 }}>{error}</p>
      </div>
    );
  }
  if (!settings) return null;
  return (
    <HashRouter>
      <Layout />
      <UpdatePrompt />
      <Toasts />
    </HashRouter>
  );
}
