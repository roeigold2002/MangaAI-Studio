import { useEffect, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { api, errMessage } from "../lib/api";
import { toast, toastError, useApp, useLoad } from "../lib/store";
import { bytes } from "../lib/util";
import { Field, Spinner } from "../components/ui";
import type { Settings } from "../lib/types";

export default function SettingsPage() {
  const { settings, saveSettings } = useApp();
  const [s, setS] = useState<Settings>(settings!);
  useEffect(() => setS(settings!), [settings]);
  const dirty = JSON.stringify(s) !== JSON.stringify(settings);
  const paths = useLoad(() => api.appPaths(), []);
  const cache = useLoad(() => api.cacheSize(), []);
  const [pwd, setPwd] = useState("");
  const [conn, setConn] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  async function save() {
    try {
      await saveSettings(s);
      toast("ההגדרות נשמרו", "ok");
    } catch (e) {
      toastError(e);
    }
  }

  async function testConn() {
    setTesting(true);
    setConn(null);
    try {
      await saveSettings(s);
      if (pwd) await api.setSourcePassword(pwd);
      setConn(`מחובר: ${await api.sourceStatus()}`);
    } catch (e) {
      setConn(`שגיאה: ${errMessage(e)}`);
    } finally {
      setTesting(false);
    }
  }

  async function exportData() {
    const path = await saveDialog({ defaultPath: "mangaai-export.json", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (!path) return;
    try {
      await api.exportData(path);
      toast("הייצוא נשמר", "ok");
    } catch (e) {
      toastError(e);
    }
  }

  const r = s.reader;
  const setR = (patch: Partial<Settings["reader"]>) => setS({ ...s, reader: { ...r, ...patch } });
  const src = s.sources;
  const setSrc = (patch: Partial<Settings["sources"]>) => setS({ ...s, sources: { ...src, ...patch } });

  return (
    <div className="page" style={{ maxWidth: 1000 }}>
      <div className="page-head">
        <h1>הגדרות</h1>
        <button className="btn primary" disabled={!dirty} onClick={save}>{dirty ? "שמירת שינויים" : "נשמר"}</button>
      </div>

      <div className="panel">
        <div className="panel-head"><h2>מראה וקריאה</h2></div>
        <div className="grid-form">
          <Field label="ערכת צבעים">
            <select className="select" value={s.theme} onChange={(e) => setS({ ...s, theme: e.target.value as Settings["theme"] })}>
              <option value="dark">כהה</option><option value="light">בהירה</option>
            </select>
          </Field>
          <Field label="מצב קריאה כברירת מחדל">
            <select className="select" value={r.mode} onChange={(e) => setR({ mode: e.target.value as Settings["reader"]["mode"] })}>
              <option value="single">עמוד בודד</option><option value="double">כפולת עמודים</option><option value="vertical">גלילה אנכית</option>
            </select>
          </Field>
          <Field label="כיוון דפדוף כברירת מחדל" hint="אפשר לשנות לכל מנגה בנפרד">
            <select className="select" value={r.direction} onChange={(e) => setR({ direction: e.target.value as Settings["reader"]["direction"] })}>
              <option value="rtl">מימין לשמאל (מנגה)</option><option value="ltr">משמאל לימין</option>
            </select>
          </Field>
          <Field label="התאמת עמוד">
            <select className="select" value={r.fit} onChange={(e) => setR({ fit: e.target.value as Settings["reader"]["fit"] })}>
              <option value="height">התאמה למסך</option><option value="width">התאמה לרוחב</option><option value="original">גודל מקורי</option>
            </select>
          </Field>
          <Field label="עמודים לטעינה מראש">
            <input className="input" type="number" min={1} max={10} value={r.preload} onChange={(e) => setR({ preload: Number(e.target.value) })} />
          </Field>
          <Field label="רווח בין עמודים בגלילה (px)">
            <input className="input" type="number" min={0} max={80} value={r.vertical_gap} onChange={(e) => setR({ vertical_gap: Number(e.target.value) })} />
          </Field>
        </div>
        <div className="col" style={{ marginTop: 14 }}>
          <label className="check"><input type="checkbox" checked={r.double_cover_alone} onChange={(e) => setR({ double_cover_alone: e.target.checked })} /> בכפולת עמודים, להציג את העמוד הראשון לבד</label>
          <label className="check"><input type="checkbox" checked={r.show_translated} onChange={(e) => setR({ show_translated: e.target.checked })} /> להציג גרסה מתורגמת כשיש</label>
        </div>
        <p className="small faint" style={{ marginTop: 14 }}>
          קיצורים בקורא: חיצים / רווח לדפדוף, <span className="kbd">T</span> מקור או תרגום, <span className="kbd">C</span> השוואה,
          <span className="kbd">F</span> מסך מלא, <span className="kbd">G</span> עמודים, <span className="kbd">E</span> עורך, <span className="kbd">+</span>/<span className="kbd">-</span> זום, <span className="kbd">Esc</span> יציאה.
        </p>
      </div>

      <div className="panel">
        <div className="panel-head"><h2>מקורות אונליין (Suwayomi)</h2></div>
        <p className="muted small" style={{ marginBottom: 12 }}>
          חיפוש והורדה עוברים דרך <span className="ltr">Suwayomi-Server</span> שרץ אצלך, עם הרחבות המקורות שהתקנת בו. האפליקציה מורידה את העמודים לספרייה המקומית,
          כך שהם נשארים זמינים גם אם השרת כבוי.
        </p>
        <label className="check" style={{ marginBottom: 14 }}>
          <input type="checkbox" checked={src.suwayomi_enabled} onChange={(e) => setSrc({ suwayomi_enabled: e.target.checked })} /> הפעלת מקורות אונליין
        </label>
        <div className="grid-form">
          <Field label="כתובת השרת"><input className="input ltr" value={src.suwayomi_url} onChange={(e) => setSrc({ suwayomi_url: e.target.value })} /></Field>
          <Field label="שם משתמש (אם הוגדרה הזדהות)"><input className="input ltr" value={src.suwayomi_user} onChange={(e) => setSrc({ suwayomi_user: e.target.value })} /></Field>
          <Field label="סיסמה" hint="נשמרת ב-Keychain"><input className="input ltr" type="password" value={pwd} onChange={(e) => setPwd(e.target.value)} /></Field>
          <Field label="הורדות במקביל" hint="נכנס לתוקף אחרי הפעלה מחדש"><input className="input" type="number" min={1} max={6} value={src.download_concurrency} onChange={(e) => setSrc({ download_concurrency: Number(e.target.value) })} /></Field>
        </div>
        <div className="row" style={{ marginTop: 12 }}>
          <button className="btn" onClick={testConn} disabled={testing || !src.suwayomi_enabled}>{testing ? <Spinner label="בודק…" /> : "שמירה ובדיקת חיבור"}</button>
          {conn && <span className="small" style={{ color: conn.startsWith("שגיאה") ? "var(--bad)" : "var(--ok)" }} dir="auto">{conn}</span>}
        </div>
      </div>

      <div className="panel">
        <div className="panel-head"><h2>נתונים</h2></div>
        <div className="col">
          {paths.data && (
            <div className="spread">
              <span className="small muted ltr" style={{ textAlign: "start" }}>{paths.data.data}</span>
              <button className="btn sm" onClick={() => openPath(paths.data!.data).catch(toastError)}>פתיחת התיקייה</button>
            </div>
          )}
          {cache.data && (
            <div className="spread">
              <span className="small muted">
                עמודים מתורגמים ומנוקים: {bytes(cache.data.derived)} · תמונות ממוזערות: {bytes(cache.data.thumbs)} · הורדות: {bytes(cache.data.library)}
              </span>
              <button className="btn sm ghost" onClick={() => api.clearThumbnails().then(cache.reload).catch(toastError)}>ניקוי תמונות ממוזערות</button>
            </div>
          )}
          <div className="spread">
            <span className="small muted">ייצוא הספרייה, ההתקדמות, המילונים והתרגומים לקובץ JSON (בלי מפתחות ובלי תמונות).</span>
            <button className="btn sm" onClick={exportData}>ייצוא</button>
          </div>
          {paths.data && (
            <div className="spread">
              <span className="small muted">יומני שגיאות (מפתחות API לא נרשמים).</span>
              <button className="btn sm ghost" onClick={() => openPath(paths.data!.logs).catch(toastError)}>פתיחת היומנים</button>
            </div>
          )}
        </div>
      </div>

      <div className="panel">
        <div className="panel-head"><h2>אודות</h2></div>
        <p className="small muted">
          MangaAI Studio 0.1.0. בלי חשבון, בלי טלמטריה ובלי שרת ציבורי. רשימת רכיבי הקוד הפתוח והרישיונות שלהם נמצאת בקובץ THIRDPARTYNOTICES.md.
        </p>
      </div>
    </div>
  );
}
