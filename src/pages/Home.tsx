import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Play, Search as SearchIcon } from "lucide-react";
import { api } from "../lib/api";
import { toastError, useApp, useEvent, useLoad, useThrottled } from "../lib/store";
import { ago, jobLabel } from "../lib/util";
import { Cover, ErrorBox, MangaCard, Progress } from "../components/ui";
import { ImportButtons } from "../components/ImportButton";
import { SourceGate, sourceState } from "../components/SourceGate";
import { ProviderSetup } from "../components/TranslateFlow";
import { OneClickSetup } from "../components/OneClickSetup";

export default function HomePage() {
  const nav = useNavigate();
  const { settings, saveSettings } = useApp();
  const { data, error, reload } = useLoad(() => api.home(), []);
  const overview = useLoad(() => api.sourceOverview(), []);
  const provider = useLoad(() => api.providerStatus(), []);
  const [q, setQ] = useState("");
  const [setup, setSetup] = useState(false);
  const refresh = useThrottled(reload, 800);
  useEvent("job-updated", refresh);
  useEvent("download-updated", refresh);
  useEvent("sources-changed", overview.reload);

  if (error) return <div className="page"><ErrorBox error={error} onRetry={reload} /></div>;
  if (!data || !settings) return null;

  const cont = data.continue_reading[0];
  const sources = sourceState(overview.data);
  const firstRun = !settings.onboarding_done;
  const finishOnboarding = () => saveSettings({ ...settings, onboarding_done: true }).catch(toastError);

  return (
    <div className="page">
      <form className="hero" onSubmit={(e) => { e.preventDefault(); if (q.trim()) nav(`/search?q=${encodeURIComponent(q.trim())}`); }}>
        <h1>איזו מנגה נקרא היום?</h1>
        <div className="search hero-search">
          <SearchIcon size={20} />
          <input className="input" autoFocus value={q} onChange={(e) => setQ(e.target.value)} dir="auto" placeholder="חיפוש מנגה לפי שם" aria-label="חיפוש מנגה" />
          <button className="btn primary" type="submit" disabled={!q.trim()}>חיפוש</button>
        </div>
        <p className="muted small">חיפוש ← הורדה ← הספרייה שלי ← תרגום לעברית ← קריאה</p>
      </form>

      <OneClickSetup />
      {firstRun && (
        <div className="panel tone col" style={{ gap: 14, marginTop: 22 }}>
          <div className="spread"><h2>ברוך הבא</h2><button className="btn ghost sm" onClick={finishOnboarding}>הסתרה</button></div>
          <ol className="steps">
            <li><strong>מחפשים</strong> מנגה בשורת החיפוש ופותחים תוצאה.</li>
            <li><strong>מורידים</strong> פרק או את כל הפרקים. הם נכנסים לבד ל"הספרייה שלי".</li>
            <li><strong>מתרגמים</strong> בלחיצה על "תרגם לעברית" בעמוד המנגה או בתוך הקורא.</li>
            <li><strong>קוראים</strong>. התרגום נשמר במחשב ולא משלמים עליו שוב.</li>
          </ol>
          <div className="col" style={{ gap: 8 }}>
            <div className="row small">
              <span className="dot" style={{ color: sources === "ready" ? "var(--ok)" : "var(--warn)" }} />
              {sources === "ready" ? `אתרי מנגה: ${overview.data?.health?.website_sources} מותקנים` : "אתרי מנגה: עוד לא מותקנים (פרטים למטה)"}
            </div>
            <div className="row small">
              <span className="dot" style={{ color: data.library_count ? "var(--ok)" : "var(--faint)" }} />
              {data.library_count ? `בספרייה: ${data.library_count} כותרים` : "הספרייה עוד ריקה"}
            </div>
            <div className="row small">
              <span className="dot" style={{ color: provider.data?.ready ? "var(--ok)" : "var(--faint)" }} />
              {provider.data?.ready ? `תרגום: ${provider.data.label}` : "תרגום: עוד לא הוגדר ספק (אפשר גם אחר כך)"}
              {!provider.data?.ready && <button className="btn ghost sm" onClick={() => setSetup(true)}>הגדרה עכשיו</button>}
            </div>
          </div>
          <div className="row"><button className="btn primary" onClick={() => { finishOnboarding(); document.querySelector<HTMLInputElement>(".hero input")?.focus(); }}>בוא נתחיל</button></div>
        </div>
      )}

      {!overview.loading && sources !== "ready" && (
        <div style={{ marginTop: 22 }}><SourceGate overview={overview.data} onChange={overview.reload} compact={!firstRun} /></div>
      )}

      {cont && (
        <div className="section">
          <div className="panel tone" style={{ display: "grid", gridTemplateColumns: "92px 1fr auto", gap: 20, alignItems: "center" }}>
            <div style={{ width: 92 }}><Cover mangaId={cont.manga_id} title={cont.manga_title} /></div>
            <div className="col" style={{ gap: 6, minWidth: 0 }}>
              <span className="muted small">המשך קריאה</span>
              <h2 className="ellipsis" dir="auto">{cont.manga_title}</h2>
              <span className="muted" dir="auto">{cont.chapter_title} · עמוד {cont.page_idx + 1} מתוך {cont.page_count}</span>
              <div style={{ maxWidth: 360 }}><Progress value={cont.page_idx + 1} total={cont.page_count} /></div>
            </div>
            <button className="btn primary" onClick={() => nav(`/read/${cont.chapter_id}?page=${cont.page_idx}`)}>
              <Play size={16} /> המשך קריאה
            </button>
          </div>
        </div>
      )}

      {data.active_jobs.length + data.active_downloads.length > 0 && (
        <div className="section">
          <div className="spread"><h2>בעבודה עכשיו</h2></div>
          <div className="panel list" style={{ padding: 0 }}>
            {data.active_downloads.map((d) => (
              <Link to="/downloads" key={`d${d.id}`} className="list-row clickable">
                <span className="badge">הורדה</span>
                <span className="grow ellipsis" dir="auto">{d.manga_title} · {d.chapter_title}</span>
                <span className="muted small">{d.progress}/{d.total || "?"}</span>
                <div style={{ width: 140 }}><Progress value={d.progress} total={d.total} /></div>
              </Link>
            ))}
            {data.active_jobs.map((j) => (
              <Link to="/queue" key={`j${j.id}`} className="list-row clickable">
                <span className="badge run">תרגום</span>
                <span className="grow ellipsis" dir="auto">{j.manga_title} · {j.chapter_title}</span>
                <span className="muted small">{jobLabel[j.status] ?? j.status}</span>
                <div style={{ width: 140 }}><Progress value={j.completed + j.failed} total={j.total} /></div>
              </Link>
            ))}
          </div>
        </div>
      )}

      {data.continue_reading.length > 1 && (
        <div className="section">
          <div className="spread"><h2>נקראו לאחרונה</h2><Link className="btn ghost sm" to="/history">כל ההיסטוריה</Link></div>
          <div className="covers compact">
            {data.continue_reading.slice(1).map((h) => (
              <div key={h.manga_id} className="cover-card" onClick={() => nav(`/read/${h.chapter_id}?page=${h.page_idx}`)}>
                <Cover mangaId={h.manga_id} title={h.manga_title} />
                <div>
                  <div className="cover-title" dir="auto">{h.manga_title}</div>
                  <div className="cover-sub" dir="auto">{h.chapter_title} · {ago(h.at)}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {data.recently_added.length > 0 && (
        <div className="section">
          <div className="spread"><h2>נוספו לאחרונה</h2><Link className="btn ghost sm" to="/library">להספרייה שלי</Link></div>
          <div className="covers compact">{data.recently_added.slice(0, 8).map((m) => <MangaCard key={m.id} m={m} />)}</div>
        </div>
      )}

      {sources === "ready" && (
        <div className="section">
          <div className="spread"><h3 className="muted">יש לך מנגה במחשב?</h3></div>
          <ImportButtons onDone={reload} />
        </div>
      )}

      {setup && <ProviderSetup status={provider.data} onClose={() => setSetup(false)} onDone={() => { setSetup(false); provider.reload(); }} />}
    </div>
  );
}
