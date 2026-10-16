import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import TranslationSettingsPage from "./TranslationSettings";
import { SourcesAdvanced } from "../components/SourcesAdvanced";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { BackupSync } from "../components/BackupSync";
import { openPath } from "@tauri-apps/plugin-opener";
import { getVersion } from "@tauri-apps/api/app";
import { api, errMessage } from "../lib/api";
import { checkForUpdate } from "../lib/updates";
import { toast, toastError, useApp, useLoad } from "../lib/store";
import { bytes } from "../lib/util";
import { Field } from "../components/ui";
import type { Settings } from "../lib/types";

const TABS = [["general", "כללי"], ["translation", "תרגום"], ["sources", "מקורות ותוספים (מתקדם)"], ["data", "נתונים"]] as const;

export default function SettingsPage() {
  const [params, setParams] = useSearchParams();
  const tab = params.get("tab") ?? "general";
  return (
    <div className="page" style={{ maxWidth: 1040 }}>
      <div className="page-head"><h1>הגדרות</h1></div>
      <div className="tabs" role="tablist">
        {TABS.map(([k, label]) => (
          <button key={k} role="tab" aria-selected={tab === k} className={tab === k ? "on" : ""} onClick={() => setParams({ tab: k })}>{label}</button>
        ))}
      </div>
      {tab === "translation" ? <TranslationSettingsPage embedded /> : tab === "sources" ? <SourcesAdvanced /> : <GeneralAndData tab={tab} />}
    </div>
  );
}

function GeneralAndData({ tab }: { tab: string }) {
  const { settings, saveSettings } = useApp();
  const [s, setS] = useState<Settings>(settings!);
  useEffect(() => setS(settings!), [settings]);
  const dirty = JSON.stringify(s) !== JSON.stringify(settings);
  const paths = useLoad(() => api.appPaths(), []);
  const cache = useLoad(() => api.cacheSize(), []);
  const version = useLoad(() => getVersion(), []);
  const [checking, setChecking] = useState(false);

  async function checkUpdates() {
    setChecking(true);
    try {
      // a newer version opens the update prompt
      if (!(await checkForUpdate())) toast("זו הגרסה העדכנית ביותר", "ok");
    } catch (e) {
      toast(`בדיקת העדכונים נכשלה: ${errMessage(e)}`, "bad");
    } finally {
      setChecking(false);
    }
  }

  async function save() {
    try {
      await saveSettings(s);
      toast("ההגדרות נשמרו", "ok");
    } catch (e) {
      toastError(e);
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

  return (
    <div>
      {tab === "general" && (
      <>
      <div className="row" style={{ justifyContent: "flex-end", marginBottom: 12 }}>
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
        <div className="panel-head"><h2>ברוך הבא</h2></div>
        <div className="row"><button className="btn sm" onClick={() => saveSettings({ ...settings!, onboarding_done: false }).catch(toastError)}>להציג שוב את מסך הפתיחה בבית</button></div>
      </div>
      </>
      )}

      {tab === "data" && (
      <>
      <BackupSync />
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
        <div className="panel-head"><h2>היסטוריה</h2></div>
        <a className="btn sm" href="#/history">היסטוריית קריאה</a>
      </div>
      <div className="panel">
        <div className="panel-head"><h2>אודות</h2></div>
        <p className="small muted">
          MangaAI Studio {version.data ?? ""}. בלי חשבון, בלי טלמטריה ובלי שרת ציבורי. רשימת רכיבי הקוד הפתוח והרישיונות שלהם נמצאת בקובץ THIRDPARTYNOTICES.md.
        </p>
        <div className="spread" style={{ marginTop: 10 }}>
          <span className="small muted">עדכונים נבדקים פעם ביום מול דף הגרסאות ב-GitHub, ומותקנים רק באישורך.</span>
          <button className="btn sm" disabled={checking} onClick={checkUpdates}>{checking ? "בודק…" : "בדיקת עדכונים"}</button>
        </div>
      </div>
      </>
      )}
    </div>
  );
}
