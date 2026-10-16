import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { openPath } from "@tauri-apps/plugin-opener";
import { Compass, Download, FolderOpen, Plus, Power, RefreshCw, Search, Trash2 } from "lucide-react";
import { api, errMessage } from "../lib/api";
import { toast, toastError, useApp, useEvent, useLoad } from "../lib/store";
import { credentialStoreName, langName, remoteUrl } from "../lib/util";
import { Empty, ErrorBox, Field, Modal, Spinner } from "./ui";
import { SourceGate, sourceState } from "./SourceGate";
import type { Extension, InstalledSource, SourcePreference } from "../lib/types";

/** Advanced: server connection, managed engine, extension stores and extensions. */
export function SourcesAdvanced() {
  const overview = useLoad(() => api.sourceOverview(), []);
  useEvent("sources-changed", overview.reload);
  const o = overview.data;
  const state = sourceState(o);
  const serverUp = o?.health?.server === "ok";

  return (
    <div className="col" style={{ gap: 14 }}>
      <EngineAndConnection onChanged={overview.reload} />
      {!overview.loading && state !== "ready" && state !== "local_only" && <SourceGate overview={o} onChange={overview.reload} compact />}
      {o?.health && serverUp && (
        <div className="panel">
          <div className="panel-head"><h2>מצב</h2><button className="btn ghost sm" onClick={overview.reload}><RefreshCw size={14} /> רענון</button></div>
          <div className="row wrap small" style={{ gap: 18 }}>
            <span><span className="dot" style={{ color: "var(--ok)" }} /> {o.health.server_version}</span>
            <span>{o.health.online_sources} מקורות אונליין</span>
            <span>{o.health.stores} חנויות תוספים</span>
            <span>{o.health.installed_extensions} תוספים מותקנים</span>
            <Link className="btn ghost sm" to="/browse"><Compass size={14} /> עיון לפי מקור</Link>
            {o.local_dir && (
              <button className="btn ghost sm" onClick={() => openPath(o.local_dir!).catch(toastError)} title="קבצי CBZ שתשים כאן יופיעו במקור Local source">
                <FolderOpen size={14} /> התיקייה המקומית של המנוע
              </button>
            )}
          </div>
        </div>
      )}
      {serverUp && <InstalledSources key={o?.health?.sources} />}
      {serverUp && <Stores onChanged={overview.reload} />}
      {serverUp && <Extensions onChanged={overview.reload} />}
    </div>
  );
}

function EngineAndConnection({ onChanged }: { onChanged: () => void }) {
  const { settings, saveSettings } = useApp();
  const [s, setS] = useState(settings!.sources);
  const [pwd, setPwd] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => setS(settings!.sources), [settings]);

  async function saveAndTest() {
    setBusy(true);
    setMsg(null);
    try {
      await saveSettings({ ...useApp.getState().settings!, sources: { ...s, managed: useApp.getState().settings!.sources.managed } });
      if (pwd) await api.setSourcePassword(pwd);
      setMsg(`מחובר: ${await api.sourceStatus()}`);
      onChanged();
    } catch (e) {
      setMsg(`שגיאה: ${errMessage(e)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel">
      <div className="panel-head"><h2>מנוע המקורות (Suwayomi)</h2></div>
      <p className="muted small" style={{ marginBottom: 12 }}>
        {s.managed
          ? "האפליקציה התקינה את Suwayomi ומפעילה אותו בעצמה על 127.0.0.1 בלבד. הוא נעצר כשהאפליקציה נסגרת."
          : "כתובת של שרת Suwayomi שאתה מריץ בעצמך. זו כתובת שרת, לא כתובת של חנות תוספים."}
      </p>
      <label className="check" style={{ marginBottom: 12 }}>
        <input type="checkbox" checked={s.suwayomi_enabled} onChange={(e) => setS({ ...s, suwayomi_enabled: e.target.checked })} /> מקורות אונליין מופעלים
      </label>
      <div className="grid-form">
        <Field label="כתובת השרת"><input className="input ltr" value={s.suwayomi_url} onChange={(e) => setS({ ...s, suwayomi_url: e.target.value })} /></Field>
        <Field label="שם משתמש (אם הוגדרה הזדהות)"><input className="input ltr" value={s.suwayomi_user} onChange={(e) => setS({ ...s, suwayomi_user: e.target.value })} /></Field>
        <Field label="סיסמה" hint={`נשמרת ב${credentialStoreName()}`}><input className="input ltr" type="password" value={pwd} onChange={(e) => setPwd(e.target.value)} /></Field>
        <Field label="הורדות במקביל" hint="נכנס לתוקף אחרי הפעלה מחדש"><input className="input" type="number" min={1} max={6} value={s.download_concurrency} onChange={(e) => setS({ ...s, download_concurrency: Number(e.target.value) })} /></Field>
      </div>
      <Field label="שפות לחיפוש" hint="מקורות כמו MangaDex מותקנים כמקור נפרד לכל שפה. החיפוש רץ רק על השפות שנבחרו. זיהוי הטקסט לתרגום תומך כרגע ביפנית.">
        <div className="row wrap">
          {["ja", "en", "ko", "zh-Hans", "zh-Hant", "es", "fr", "pt-BR", "ru", "he"].map((l) => (
            <label key={l} className="check small">
              <input type="checkbox" checked={s.languages.includes(l)}
                onChange={(e) => setS({ ...s, languages: e.target.checked ? [...s.languages, l] : s.languages.filter((x) => x !== l) })} />
              {langName(l)}
            </label>
          ))}
        </div>
      </Field>
      <div className="row wrap" style={{ marginTop: 12 }}>
        <button className="btn" onClick={saveAndTest} disabled={busy}>{busy ? <Spinner label="בודק…" /> : "שמירה ובדיקת חיבור"}</button>
        {s.managed && (
          <>
            <button className="btn ghost" onClick={() => api.startSources().then(onChanged).catch(toastError)}><Power size={14} /> הפעלת המנוע</button>
            <button className="btn ghost" onClick={() => api.stopSources().then(onChanged).catch(toastError)}>עצירת המנוע</button>
          </>
        )}
        {msg && <span className="small" style={{ color: msg.startsWith("שגיאה") ? "var(--bad)" : "var(--ok)" }} dir="auto">{msg}</span>}
      </div>
    </div>
  );
}

function Stores({ onChanged }: { onChanged: () => void }) {
  const { data, error, loading, reload } = useLoad(() => api.stores(), []);
  const [adding, setAdding] = useState(false);
  const [url, setUrl] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function add() {
    setErr(null);
    try {
      await api.validateStoreUrl(url);
    } catch (e) {
      setErr(errMessage(e));
      return;
    }
    setBusy(true);
    try {
      const st = await api.addStore(url);
      toast(`החנות "${st.name}" נוספה`, "ok");
      setAdding(false);
      setUrl("");
      reload();
      onChanged();
    } catch (e) {
      setErr(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function addDefaults() {
    setBusy(true);
    try {
      const r = await api.applySourceDefaults();
      const added = [...r.stores_added, ...r.extensions_installed];
      toast(added.length ? `נוספו: ${added.join(", ")}` : "כל ברירות המחדל כבר מותקנות", "ok");
      reload();
      onChanged();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel">
      <div className="panel-head">
        <h2>חנויות תוספים</h2>
        <div className="row">
          <button className="btn sm" disabled={busy} onClick={addDefaults} title="Keiyoushi + התוספים שמותקנים אוטומטית">
            <Plus size={14} /> השלמת ברירות המחדל
          </button>
          <button className="btn sm" onClick={() => setAdding(true)}><Plus size={14} /> הוספת חנות</button>
        </div>
      </div>
      <p className="muted small" style={{ marginBottom: 12 }}>
        חנות תוספים היא קובץ אינדקס (JSON) שמפרסם גורם חיצוני, עם רשימת תוספי מקורות. Suwayomi מוריד ממנה רק את התוספים שתבחר להתקין.
        ברירת המחדל: החנות הרשמית של Suwayomi (רק תוסף לחיבור לשרת אחר) וחנות הקהילה Keiyoushi (אלפי תוספים, ביניהם אתרי הוצאות לאור יפניות).
        בהתקנה הראשונה מותקנים אוטומטית גם MangaDex ו-MangaFire (מקורות באנגלית כברירת מחדל); תוספים אחרים מותקנים רק בבחירתך.
      </p>
      {loading && <Spinner label="טוען…" />}
      {error && <ErrorBox error={error} onRetry={reload} />}
      {data && data.length === 0 && <p className="muted">אין חנויות. התוספים המותקנים ממשיכים לעבוד גם בלי חנות.</p>}
      {data && data.length > 0 && (
        <div className="list">
          {data.map((s) => (
            <div key={s.index_url} className="list-row">
              <div className="col grow" style={{ gap: 2 }}>
                <strong dir="auto">{s.name}</strong>
                <span className="small faint ltr" style={{ textAlign: "start" }}>{s.index_url}</span>
              </div>
              {s.is_legacy && <span className="badge">פורמט ישן</span>}
              <button className="btn ghost sm danger" onClick={() => api.removeStore(s.index_url).then(() => { reload(); onChanged(); }).catch(toastError)}>
                <Trash2 size={14} /> הסרה
              </button>
            </div>
          ))}
        </div>
      )}
      {adding && (
        <Modal title="הוספת חנות תוספים" onClose={() => setAdding(false)}
          actions={<>
            <button className="btn ghost" onClick={() => setAdding(false)}>ביטול</button>
            <button className="btn primary" disabled={busy || !url.trim()} onClick={add}>{busy ? "השרת בודק…" : "הוספה"}</button>
          </>}>
          <Field label="כתובת קובץ האינדקס" hint="https, ומסתיימת ב-.json (למשל …/index.min.json)">
            <input className="input ltr" autoFocus value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…/index.min.json" />
          </Field>
          <button className="btn ghost sm" style={{ alignSelf: "flex-start" }} onClick={() => setUrl("https://raw.githubusercontent.com/suwayomi/tachiyomi-extension/repo/index.min.json")}>
            מילוי החנות הרשמית של Suwayomi
          </button>
          <button className="btn ghost sm" style={{ alignSelf: "flex-start" }} onClick={() => setUrl("https://raw.githubusercontent.com/keiyoushi/extensions/repo/repo.json")}>
            מילוי חנות Keiyoushi
          </button>
          <p className="small muted">התוספים בחנות לא מותקנים אוטומטית. השרת בודק את הכתובת ומסרב אם היא לא חנות תקינה.</p>
          {err && <div className="callout bad small" dir="auto">{err}</div>}
        </Modal>
      )}
    </div>
  );
}

function Extensions({ onChanged }: { onChanged: () => void }) {
  const [list, setList] = useState<Extension[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [lang, setLang] = useState("all");
  const [confirm, setConfirm] = useState<Extension | null>(null);

  const load = async (refresh: boolean) => {
    setBusy(refresh ? "מרענן את החנויות…" : "טוען…");
    setError(null);
    try {
      setList(await api.extensions(refresh));
    } catch (e) {
      setError(errMessage(e));
    } finally {
      setBusy(null);
    }
  };
  useEffect(() => {
    (async () => {
      await load(false);
      // a store added elsewhere may not have been fetched yet
      const now = await api.extensions(false).catch(() => []);
      if (now.length === 0 && (await api.stores().catch(() => [])).length > 0) await load(true);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function change(e: Extension, action: "install" | "uninstall" | "update") {
    setBusy(`${action === "install" ? "מתקין" : action === "uninstall" ? "מסיר" : "מעדכן"} את ${e.name}…`);
    try {
      const r = await api.changeExtension(e.pkg_name, action);
      toast(`${e.name}: ${action === "uninstall" ? "הוסר" : "הותקן"}. מקורות אונליין זמינים: ${r.online_sources}`, "ok");
      setList(await api.extensions(false));
      onChanged();
    } catch (err) {
      toast(`${e.name}: ${errMessage(err)}`, "bad");
    } finally {
      setBusy(null);
    }
  }

  const langs = useMemo(() => [...new Set((list ?? []).map((e) => e.lang))].sort(), [list]);
  const filtered = (list ?? []).filter((e) => (lang === "all" || e.lang === lang) && (!q || e.name.toLowerCase().includes(q.toLowerCase())));
  const installed = filtered.filter((e) => e.is_installed);
  const available = filtered.filter((e) => !e.is_installed);

  const row = (e: Extension) => (
    <div key={e.pkg_name} className="list-row">
      {e.icon_url && <img src={remoteUrl(e.icon_url)} width={28} height={28} style={{ borderRadius: 6 }} alt="" loading="lazy" />}
      <div className="col grow" style={{ gap: 2 }}>
        <span dir="auto"><strong>{e.name}</strong> <span className="faint small">{e.version_name}</span></span>
        <span className="small faint ltr" style={{ textAlign: "start" }}>{e.pkg_name}</span>
      </div>
      <span className="badge">{e.lang}</span>
      {e.is_nsfw && <span className="badge warn">18+</span>}
      {e.is_obsolete && <span className="badge bad">לא נתמך יותר</span>}
      {e.is_installed ? (
        <>
          {e.has_update && <button className="btn sm" disabled={!!busy} onClick={() => change(e, "update")}>עדכון</button>}
          <button className="btn ghost sm danger" disabled={!!busy} onClick={() => change(e, "uninstall")}>הסרה</button>
        </>
      ) : (
        <button className="btn sm" disabled={!!busy} onClick={() => setConfirm(e)}><Download size={14} /> התקנה</button>
      )}
    </div>
  );

  return (
    <div className="panel">
      <div className="panel-head">
        <h2>תוספי מקורות</h2>
        <button className="btn sm" disabled={!!busy} onClick={() => load(true)}><RefreshCw size={14} /> רענון החנויות</button>
      </div>
      <div className="row wrap" style={{ marginBottom: 12 }}>
        <div className="search" style={{ width: 240 }}>
          <Search size={16} />
          <input className="input" placeholder="חיפוש תוסף" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <select className="select" style={{ width: 140 }} value={lang} onChange={(e) => setLang(e.target.value)} aria-label="שפה">
          <option value="all">כל השפות</option>
          {langs.map((l) => <option key={l} value={l}>{l}</option>)}
        </select>
        {busy && <Spinner label={busy} />}
      </div>
      {error && <ErrorBox error={error} onRetry={() => load(false)} />}
      {list && list.length === 0 && <Empty title="אין תוספים">הוסף חנות תוספים ולחץ על "רענון החנויות".</Empty>}
      {installed.length > 0 && (<><h3 style={{ margin: "6px 0" }}>מותקנים ({installed.length})</h3><div className="list">{installed.map(row)}</div></>)}
      {available.length > 0 && (<><h3 style={{ margin: "14px 0 6px" }}>זמינים להתקנה ({available.length})</h3><div className="list">{available.slice(0, 300).map(row)}</div></>)}
      {confirm && (
        <Modal title={`להתקין את ${confirm.name}?`} onClose={() => setConfirm(null)}
          actions={<>
            <button className="btn ghost" onClick={() => setConfirm(null)}>ביטול</button>
            <button className="btn primary" onClick={() => { const e = confirm; setConfirm(null); change(e, "install"); }}>התקנה</button>
          </>}>
          <p className="muted">
            התוסף ({confirm.pkg_name}, גרסה {confirm.version_name}) יורד מהחנות ומותקן במנוע המקורות. הוא קוד של גורם חיצוני שרץ בתוך Suwayomi
            ופונה לאתר של המקור.
          </p>
        </Modal>
      )}
    </div>
  );
}

/** Installed sources: search them and open a source's own settings (server address, login, …). */
function InstalledSources() {
  const { data, error, reload } = useLoad(() => api.installedSources(), []);
  const [q, setQ] = useState("");
  const [open, setOpen] = useState<InstalledSource | null>(null);
  const list = (data ?? []).filter((s) => s.id !== "0" && (!q || s.displayName.toLowerCase().includes(q.toLowerCase())));
  return (
    <div className="panel">
      <div className="panel-head">
        <h2>מקורות מותקנים</h2>
        <div className="search" style={{ width: 220 }}>
          <Search size={16} />
          <input className="input" placeholder="חיפוש מקור" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
      </div>
      {error && <ErrorBox error={error} onRetry={reload} />}
      {data && list.length === 0 && <p className="muted">{q ? "אין מקור בשם הזה." : "אין מקורות אונליין מותקנים. התקן תוסף מהרשימה למטה."}</p>}
      <div className="list">
        {list.map((s) => (
          <div key={s.id} className="list-row">
            {s.iconUrl && <img src={remoteUrl(s.iconUrl)} width={26} height={26} style={{ borderRadius: 6 }} alt="" />}
            <span className="grow" dir="auto">{s.displayName}</span>
            <span className="badge">{s.lang}</span>
            {s.isNsfw && <span className="badge warn">18+</span>}
            {s.isConfigurable && <button className="btn sm" onClick={() => setOpen(s)}>הגדרות המקור</button>}
          </div>
        ))}
      </div>
      {open && <SourceSettings source={open} onClose={() => setOpen(null)} />}
    </div>
  );
}

function SourceSettings({ source, onClose }: { source: InstalledSource; onClose: () => void }) {
  const [prefs, setPrefs] = useState<SourcePreference[] | null>(null);
  const [draft, setDraft] = useState<Record<number, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api.sourcePreferences(source.id).then(setPrefs).catch((e) => setError(errMessage(e)));
  }, [source.id]);

  async function set(p: SourcePreference, value: unknown) {
    setBusy(true);
    setError(null);
    try {
      setPrefs(await api.setSourcePreference(source.id, p.position, p.kind, value));
      setDraft((d) => { const n = { ...d }; delete n[p.position]; return n; });
      toast(`נשמר: ${p.title}`, "ok");
    } catch (e) {
      setError(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal title={`הגדרות: ${source.displayName}`} onClose={onClose} actions={<button className="btn primary" onClick={onClose}>סגירה</button>}>
      <p className="small muted">ההגדרות נשמרות במנוע המקורות (Suwayomi) ומגיעות מהתוסף עצמו.</p>
      {!prefs && !error && <Spinner label="טוען…" />}
      {error && <div className="callout bad small" dir="auto">{error}</div>}
      {prefs?.filter((p) => p.kind !== "unknown").map((p) => (
        <div key={p.position} className="col" style={{ gap: 6 }}>
          {p.kind === "text" && (
            <Field label={p.title} hint={p.summary || undefined}>
              <div className="row">
                <input className="input ltr" type={p.secret ? "password" : "text"} autoComplete="off"
                  value={draft[p.position] ?? (p.secret ? "" : String(p.value ?? ""))}
                  placeholder={p.secret && p.value ? "••••••" : p.summary}
                  onChange={(e) => setDraft({ ...draft, [p.position]: e.target.value })} />
                <button className="btn sm" disabled={busy || draft[p.position] === undefined} onClick={() => set(p, draft[p.position])}>שמירה</button>
              </div>
            </Field>
          )}
          {(p.kind === "switch" || p.kind === "checkbox") && (
            <label className="check"><input type="checkbox" checked={!!p.value} disabled={busy} onChange={(e) => set(p, e.target.checked)} /> {p.title}</label>
          )}
          {p.kind === "list" && (
            <Field label={p.title}>
              <select className="select" value={String(p.value ?? "")} disabled={busy} onChange={(e) => set(p, e.target.value)}>
                {p.entry_values.map((v, i) => <option key={v} value={v}>{p.entries[i] ?? v}</option>)}
              </select>
            </Field>
          )}
          {p.kind === "multi" && (
            <Field label={p.title}>
              <div className="row wrap">
                {p.entry_values.map((v, i) => {
                  const cur = Array.isArray(p.value) ? p.value : [];
                  return (
                    <label key={v} className="check small">
                      <input type="checkbox" checked={cur.includes(v)} disabled={busy}
                        onChange={(e) => set(p, e.target.checked ? [...cur, v] : cur.filter((x) => x !== v))} /> {p.entries[i] ?? v}
                    </label>
                  );
                })}
              </div>
            </Field>
          )}
        </div>
      ))}
    </Modal>
  );
}
