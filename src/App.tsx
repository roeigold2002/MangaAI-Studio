import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { HashRouter, NavLink, Route, Routes, useLocation } from "react-router-dom";
import { BookMarked, Compass, Download, Heart, History, Home, Languages, ListChecks, Settings as Cog } from "lucide-react";
import { useApp, useEvent } from "./lib/store";
import { api } from "./lib/api";
import { Toasts } from "./components/ui";
import HomePage from "./pages/Home";
import LibraryPage from "./pages/Library";
import MangaPage from "./pages/Manga";
import ReaderPage from "./pages/Reader";
import EditorPage from "./pages/Editor";
import BrowsePage from "./pages/Browse";
import DownloadsPage from "./pages/Downloads";
import QueuePage from "./pages/Queue";
import HistoryPage from "./pages/History";
import TranslationSettingsPage from "./pages/TranslationSettings";
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
  useEvent("download-updated", refresh);

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
      {item("/library", "ספרייה", <BookMarked size={18} />)}
      {item("/favorites", "מועדפים", <Heart size={18} />)}
      {item("/history", "היסטוריית קריאה", <History size={18} />)}
      <div className="nav-sep" />
      {item("/browse", "עיון במקורות", <Compass size={18} />)}
      {item("/downloads", "הורדות", <Download size={18} />, active.downloads)}
      {item("/queue", "תור תרגום", <ListChecks size={18} />, active.jobs)}
      <div className="nav-sep" />
      {item("/translation", "הגדרות תרגום", <Languages size={18} />)}
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
          <Route path="/library" element={<LibraryPage />} />
          <Route path="/favorites" element={<LibraryPage favorites />} />
          <Route path="/manga/:id" element={<MangaPage />} />
          <Route path="/edit/:pageId" element={<EditorPage />} />
          <Route path="/browse" element={<BrowsePage />} />
          <Route path="/downloads" element={<DownloadsPage />} />
          <Route path="/queue" element={<QueuePage />} />
          <Route path="/history" element={<HistoryPage />} />
          <Route path="/translation" element={<TranslationSettingsPage />} />
          <Route path="/settings" element={<SettingsPage />} />
        </Routes>
      </main>
    </div>
  );
}

/** Debug builds: "route" opens a page; "tour:route|name;..." visits pages and saves webview snapshots. */
async function devStart(spec: string) {
  if (!spec.startsWith("tour:")) {
    window.location.hash = spec;
    return;
  }
  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
  await wait(1500);
  for (const item of spec.slice(5).split(";").filter(Boolean)) {
    const [route, name, delay] = item.split("|");
    window.location.hash = route;
    await wait(Number(delay ?? 2500));
    await invoke("dev_snapshot", { path: `/tmp/mangaai-shots/${name}.png` }).catch((e) => console.error(e));
  }
}

export default function App() {
  const { settings, loadSettings } = useApp();
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    loadSettings().catch((e) => setError(String(e?.message ?? e)));
    invoke<string | null>("dev_start_route").then((r) => { if (r) devStart(r); }).catch(() => {});
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
      <Toasts />
    </HashRouter>
  );
}
