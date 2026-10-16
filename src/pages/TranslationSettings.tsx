import { useEffect, useState } from "react";
import { Check, KeyRound, ShieldCheck } from "lucide-react";
import { api, errMessage } from "../lib/api";
import { toast, toastError, useApp, useLoad } from "../lib/store";
import { credentialStoreName, platform, usd } from "../lib/util";
import { Field, Spinner } from "../components/ui";
import type { ProviderConfig, Settings } from "../lib/types";

const KIND_INFO: Record<string, { local: boolean; key: boolean; note: string }> = {
  claude_cli: { local: false, key: false, note: "משתמש ב-Claude Code המותקן במחשב ובחשבון Claude שלך. בלי מפתח API. הבקשות נשלחות ל-Anthropic." },
  anthropic: { local: false, key: true, note: "Anthropic Messages API עם פלט JSON מובנה. עלות לפי טוקנים." },
  openai: { local: false, key: true, note: "OpenAI Chat Completions עם JSON Schema. בחר דגם שתומך בתמונות כדי לשלוח עמודים." },
  gemini: { local: false, key: true, note: "Google Gemini API (generateContent) עם JSON Schema." },
  ollama: { local: true, key: false, note: "מודל שרץ על המחשב דרך Ollama. אין שליחה לענן. איכות התרגום תלויה במודל; דגמים קטנים מתקשים בעברית." },
  openai_compatible: { local: true, key: false, note: "כל שרת תואם OpenAI (LM Studio, vLLM, llama.cpp). אם הכתובת לא מקומית, הנתונים יוצאים מהמחשב." },
};

const FONTS = ["VarelaRound-Regular.ttf", "SecularOne-Regular.ttf", "Karantina-Bold.ttf", "AmaticSC-Bold.ttf"];

export default function TranslationSettingsPage({ embedded = false }: { embedded?: boolean }) {
  const { settings, saveSettings } = useApp();
  const [s, setS] = useState<Settings>(settings!);
  const keys = useLoad(() => api.apiKeyStatus(), []);
  const engine = useLoad(() => api.engineStatus(), []);
  const dirty = JSON.stringify(s) !== JSON.stringify(settings);
  useEffect(() => setS(settings!), [settings]);

  const t = s.translation;
  const active = t.providers.find((p) => p.id === t.active_provider) ?? t.providers[0];
  const setT = (patch: Partial<Settings["translation"]>) => setS({ ...s, translation: { ...t, ...patch } });
  const setP = (id: string, patch: Partial<ProviderConfig>) => setT({ providers: t.providers.map((p) => (p.id === id ? { ...p, ...patch } : p)) });

  async function save() {
    try {
      await saveSettings(s);
      toast("ההגדרות נשמרו", "ok");
    } catch (e) {
      toastError(e);
    }
  }

  return (
    <div className={embedded ? "" : "page"} style={{ maxWidth: 1000 }}>
      <div className="page-head">
        <div>
          {embedded ? <h2>תרגום</h2> : <h1>הגדרות תרגום</h1>}
          <p className="muted">ספק התרגום, איכות, עלויות, עימוד העברית והמנוע המקומי.</p>
        </div>
        <button className="btn primary" disabled={!dirty} onClick={save}>{dirty ? "שמירת שינויים" : "נשמר"}</button>
      </div>

      <div className="panel">
        <div className="panel-head"><h2>ספק תרגום</h2></div>
        <div className="col" style={{ gap: 8 }}>
          {t.providers.map((p) => {
            const info = KIND_INFO[p.kind];
            return (
              <label key={p.id} className="list-row clickable" style={{ border: "1px solid var(--line)", borderRadius: 8, borderColor: p.id === active.id ? "var(--marker)" : undefined }}>
                <input type="radio" name="provider" checked={p.id === active.id} onChange={() => setT({ active_provider: p.id })} style={{ accentColor: "var(--marker)" }} />
                <div className="col grow" style={{ gap: 2 }}>
                  <strong>{p.label}</strong>
                  <span className="small muted">{info?.note}</span>
                </div>
                {info?.local ? <span className="badge ok">מקומי</span> : <span className="badge">ענן</span>}
                {info?.key && (keys.data?.[p.id] ? <span className="badge ok"><KeyRound size={11} /> מפתח שמור</span> : <span className="badge warn">חסר מפתח</span>)}
              </label>
            );
          })}
        </div>
        <ProviderForm p={active} hasKey={!!keys.data?.[active.id]} onChange={(patch) => setP(active.id, patch)} onKeySaved={keys.reload} dirty={dirty} />
      </div>

      <div className="panel">
        <div className="panel-head"><h2>פרטיות</h2><ShieldCheck size={18} className="muted" /></div>
        <div className="col">
          <p className="muted">
            בתרגום בענן נשלחים לספק הטקסט שזוהה בעמוד, ההקשר (מילון מונחים ושורות קודמות), ואם בחרת גם תמונות העמודים.
            זה לא עיבוד מקומי: הספק מקבל את התוכן לפי תנאי השימוש שלו. הספרייה עצמה, הקבצים והתרגומים נשמרים רק במחשב.
            מפתחות API נשמרים ב{credentialStoreName()} ולא בקבצי האפליקציה.
          </p>
          <label className="check">
            <input type="checkbox" checked={s.cloud_notice_accepted} onChange={(e) => setS({ ...s, cloud_notice_accepted: e.target.checked })} />
            <span>הבנתי. מותר לשלוח תוכן לספק ענן כשאני מבקש תרגום.</span>
          </label>
          <label className="check">
            <input type="checkbox" checked={t.local_only} onChange={(e) => setT({ local_only: e.target.checked })} />
            <span>מצב מקומי בלבד: לאפשר רק ספקים שרצים על המחשב (Ollama או שרת מקומי).</span>
          </label>
        </div>
      </div>

      <div className="panel">
        <div className="panel-head"><h2>איכות ועלות</h2></div>
        <div className="grid-form">
          <Field label="פרופיל איכות" hint="משפיע על עומק החשיבה של הדגם (effort) כשהספק תומך בזה">
            <select className="select" value={t.quality} onChange={(e) => setT({ quality: e.target.value })}>
              <option value="fast">מהיר וזול</option><option value="balanced">מאוזן</option><option value="best">האיכות הגבוהה ביותר</option>
            </select>
          </Field>
          <Field label="עמודים בכל בקשה" hint="יותר עמודים = יותר הקשר ופחות בקשות">
            <input className="input" type="number" min={1} max={8} value={t.pages_per_request} onChange={(e) => setT({ pages_per_request: Number(e.target.value) })} />
          </Field>
          <Field label="שורות הקשר קודמות" hint="שורות מתורגמות מעמודים קודמים שנשלחות להמשכיות">
            <input className="input" type="number" min={0} max={200} value={t.context_lines} onChange={(e) => setT({ context_lines: Number(e.target.value) })} />
          </Field>
          <Field label="אפקטים קוליים (SFX)">
            <select className="select" value={t.sfx_mode} onChange={(e) => setT({ sfx_mode: e.target.value })}>
              <option value="keep">להשאיר את הציור המקורי</option><option value="translate">לתרגם ולצייר מחדש</option>
            </select>
          </Field>
          <Field label="בקשת אישור מעל ($)" hint="לפני תרגום שעלותו המשוערת גבוהה מזה">
            <input className="input" type="number" min={0} step={0.5} value={t.confirm_cost_above_usd} onChange={(e) => setT({ confirm_cost_above_usd: Number(e.target.value) })} />
          </Field>
          <Field label="תקרת עלות למשימה ($)" hint="המשימה תושהה כשהיא מגיעה לסכום. 0 = בלי תקרה">
            <input className="input" type="number" min={0} step={0.5} value={t.job_cost_limit_usd} onChange={(e) => setT({ job_cost_limit_usd: Number(e.target.value) })} />
          </Field>
        </div>
      </div>

      <div className="panel">
        <div className="panel-head"><h2>עימוד העברית</h2></div>
        <div className="grid-form">
          <Field label="גופן לדיבור">
            <select className="select" value={s.render.font} onChange={(e) => setS({ ...s, render: { ...s.render, font: e.target.value } })}>
              {FONTS.map((f) => <option key={f} value={f}>{f.replace(/-.*$/, "")}</option>)}
            </select>
          </Field>
          <Field label="גופן לצעקות ואפקטים">
            <select className="select" value={s.render.font_loud} onChange={(e) => setS({ ...s, render: { ...s.render, font_loud: e.target.value } })}>
              {FONTS.map((f) => <option key={f} value={f}>{f.replace(/-.*$/, "")}</option>)}
            </select>
          </Field>
          <Field label="גודל גופן מינימלי (px)" hint="ביחס לעמוד ברוחב ~1600px">
            <input className="input" type="number" min={6} max={60} value={s.render.min_font_px} onChange={(e) => setS({ ...s, render: { ...s.render, min_font_px: Number(e.target.value) } })} />
          </Field>
          <Field label="גודל גופן מקסימלי (px)">
            <input className="input" type="number" min={10} max={200} value={s.render.max_font_px} onChange={(e) => setS({ ...s, render: { ...s.render, max_font_px: Number(e.target.value) } })} />
          </Field>
          <Field label="שוליים בתוך הבועה" hint="חלק מגודל הבועה, 0.05–0.3">
            <input className="input" type="number" min={0} max={0.4} step={0.01} value={s.render.padding} onChange={(e) => setS({ ...s, render: { ...s.render, padding: Number(e.target.value) } })} />
          </Field>
          <Field label="מסגרת לטקסט על רקע (px)">
            <input className="input" type="number" min={0} max={10} value={s.render.stroke_px} onChange={(e) => setS({ ...s, render: { ...s.render, stroke_px: Number(e.target.value) } })} />
          </Field>
          <Field label="ניקוי הטקסט המקורי" hint="LaMa משחזר רקע מצויר. מילוי פשוט מתאים רק לבועות לבנות">
            <select className="select" value={s.render.inpaint} onChange={(e) => setS({ ...s, render: { ...s.render, inpaint: e.target.value } })}>
              <option value="lama">LaMa (מומלץ)</option><option value="opencv">OpenCV (מהיר, בסיסי)</option><option value="fill">מילוי צבע</option>
            </select>
          </Field>
          <Field label="סף זיהוי טקסט" hint="נמוך יותר = יותר אזורים (גם שגויים)">
            <input className="input" type="number" min={0.1} max={0.9} step={0.05} value={s.ocr.detector_threshold} onChange={(e) => setS({ ...s, ocr: { ...s.ocr, detector_threshold: Number(e.target.value) } })} />
          </Field>
          <Field label="מעבד">
            <select className="select" value={s.ocr.device} onChange={(e) => setS({ ...s, ocr: { ...s.ocr, device: e.target.value } })}>
              <option value="auto">אוטומטי</option>
              {platform === "mac" ? <option value="mps">Apple GPU (MPS)</option> : <option value="cuda">NVIDIA (CUDA)</option>}
              <option value="cpu">CPU</option>
            </select>
          </Field>
        </div>
      </div>

      <EnginePanel status={engine} />
    </div>
  );
}

function ProviderForm({ p, hasKey, onChange, onKeySaved, dirty }: {
  p: ProviderConfig; hasKey: boolean; onChange: (patch: Partial<ProviderConfig>) => void; onKeySaved: () => void; dirty: boolean;
}) {
  const info = KIND_INFO[p.kind];
  const [key, setKey] = useState("");
  const [test, setTest] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [cli, setCli] = useState<{ found: boolean; path: string | null } | null>(null);

  useEffect(() => {
    setTest(null);
    setKey("");
    if (p.kind === "ollama") api.ollamaModels(p.endpoint).then(setModels).catch(() => setModels([]));
    if (p.kind === "claude_cli") api.claudeCliStatus(p.endpoint).then(setCli).catch(() => setCli(null));
  }, [p.id, p.kind, p.endpoint]);

  async function saveKey() {
    try {
      await api.setApiKey(p.id, key);
      setKey("");
      onKeySaved();
      toast(key ? `המפתח נשמר ב${credentialStoreName()}` : "המפתח נמחק", "ok");
    } catch (e) {
      toastError(e);
    }
  }

  async function runTest() {
    setTesting(true);
    setTest(null);
    try {
      const r = await api.testProvider(p.id);
      setTest(`עובד: ${r.text} (${r.model || p.model}, ${(r.ms / 1000).toFixed(1)} שנ׳${r.cost_usd ? `, ${usd(r.cost_usd)}` : ""})`);
    } catch (e) {
      setTest(`שגיאה: ${errMessage(e)}`);
    } finally {
      setTesting(false);
    }
  }

  return (
    <div className="col" style={{ marginTop: 18, gap: 14 }}>
      <div className="grid-form">
        <Field label="דגם" hint={p.kind === "claude_cli" ? "opus / sonnet / haiku או שם דגם מלא" : p.kind === "ollama" && models.length ? `${models.length} דגמים מותקנים` : undefined}>
          {p.kind === "ollama" && models.length ? (
            <select className="select ltr" value={p.model} onChange={(e) => onChange({ model: e.target.value })}>
              <option value="">בחר דגם</option>
              {models.map((m) => <option key={m} value={m}>{m}</option>)}
            </select>
          ) : (
            <input className="input ltr" value={p.model} onChange={(e) => onChange({ model: e.target.value })} placeholder="model id" />
          )}
        </Field>
        {p.kind !== "anthropic" || p.endpoint ? (
          <Field label={p.kind === "claude_cli" ? "נתיב ל-claude (לא חובה)" : "כתובת (endpoint)"}>
            <input className="input ltr" value={p.endpoint} onChange={(e) => onChange({ endpoint: e.target.value })} />
          </Field>
        ) : null}
        <Field label="שליחת תמונות העמודים" hint="משפר זיהוי דובר ומגדר. מגדיל עלות">
          <select className="select" value={p.send_images ? "1" : "0"} onChange={(e) => onChange({ send_images: e.target.value === "1" })}>
            <option value="1">כן</option><option value="0">רק טקסט</option>
          </select>
        </Field>
        <Field label="זמן המתנה לבקשה (שניות)">
          <input className="input" type="number" min={10} max={1800} value={p.timeout_secs} onChange={(e) => onChange({ timeout_secs: Number(e.target.value) })} />
        </Field>
        <Field label="ניסיונות חוזרים">
          <input className="input" type="number" min={0} max={8} value={p.max_retries} onChange={(e) => onChange({ max_retries: Number(e.target.value) })} />
        </Field>
        <Field label="בקשות במקביל" hint="כמה בקשות תרגום נשלחות בבת אחת; בינתיים המנוע המקומי מכין את העמודים הבאים">
          <input className="input" type="number" min={1} max={8} value={p.max_concurrency} onChange={(e) => onChange({ max_concurrency: Number(e.target.value) })} />
        </Field>
        {!info?.local && p.kind !== "claude_cli" && (
          <>
            <Field label="מחיר קלט ($ למיליון טוקנים)" hint="להערכת עלות בלבד. 0 = לא ידוע">
              <input className="input" type="number" min={0} step={0.01} value={p.price_input_per_mtok} onChange={(e) => onChange({ price_input_per_mtok: Number(e.target.value) })} />
            </Field>
            <Field label="מחיר פלט ($ למיליון טוקנים)">
              <input className="input" type="number" min={0} step={0.01} value={p.price_output_per_mtok} onChange={(e) => onChange({ price_output_per_mtok: Number(e.target.value) })} />
            </Field>
          </>
        )}
      </div>
      {info?.key && (
        <div className="row wrap">
          <div style={{ width: 360 }}>
            <Field label={hasKey ? "מפתח API (שמור. הזן כדי להחליף)" : "מפתח API"}>
              <input className="input ltr" type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} placeholder={hasKey ? "••••••••" : ""} />
            </Field>
          </div>
          <button className="btn" style={{ alignSelf: "flex-end" }} disabled={!key && !hasKey} onClick={saveKey}>
            {key ? <><Check size={14} /> שמירת מפתח</> : "מחיקת המפתח"}
          </button>
        </div>
      )}
      {p.kind === "claude_cli" && cli && (
        <div className={`callout small ${cli.found ? "" : "bad"}`}>
          {cli.found ? <>נמצא: <span className="ltr">{cli.path}</span></> : "Claude Code לא נמצא. התקן אותו או הזן את הנתיב."}
        </div>
      )}
      <div className="row">
        <button className="btn" onClick={runTest} disabled={testing || dirty} title={dirty ? "שמור קודם את ההגדרות" : undefined}>
          {testing ? <Spinner label="בודק…" /> : "בדיקת חיבור"}
        </button>
        <span className="small faint">שולח בקשה קטנה אחת (מילה אחת לתרגום).</span>
      </div>
      {test && <div className={`callout small ${test.startsWith("שגיאה") ? "bad" : ""}`} dir="auto">{test}</div>}
    </div>
  );
}

function EnginePanel({ status }: { status: ReturnType<typeof useLoad<Awaited<ReturnType<typeof api.engineStatus>>>> }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [gpuChoice, setGpuChoice] = useState<boolean | null>(null);
  const s = status.data;
  const gpuOffered = !!s?.cuda_build && (s?.gpus.length ?? 0) > 0;
  const gpu = gpuOffered && (gpuChoice ?? true);
  async function run(label: string, fn: () => Promise<unknown>) {
    setBusy(label);
    try {
      const r = await fn();
      toast(typeof r === "string" ? r : "הושלם", "ok");
      status.reload();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(null);
    }
  }
  const h = s?.health;
  const row = (ok: boolean | undefined, label: string) => (
    <div className="row small"><span className="dot" style={{ color: ok ? "var(--ok)" : "var(--bad)" }} /> {label}</div>
  );
  return (
    <div className="panel">
      <div className="panel-head"><h2>מנוע הזיהוי והעימוד</h2>{(status.loading || busy) && <Spinner label={busy ?? "בודק…"} />}</div>
      <p className="muted small" style={{ marginBottom: 12 }}>
        זיהוי הבועות, ה-OCR, ניקוי הטקסט ועימוד העברית רצים במחשב שלך (Python, PyTorch). רק שלב התרגום עצמו פונה לספק שבחרת.
      </p>
      {s && (
        <div className="col" style={{ gap: 6 }}>
          {row(s.installed, s.installed ? "סביבת Python מותקנת" : "סביבת Python לא מותקנת")}
          {h && row(!!h.torch, `PyTorch ${h.torch ?? "חסר"}${h.device ? ` · ${h.device}` : ""}${h.gpu ? ` · ${h.gpu}` : ""}`)}
          {h && gpuOffered && !h.cuda && (
            <div className="callout small">
              נמצא כרטיס NVIDIA ({s.gpus[0].replace(/^NVIDIAs+/i, "")}) אבל המנוע רץ על המעבד. התקנה מחדש עם תמיכת GPU תאיץ את הזיהוי והניקוי פי כמה.
            </div>
          )}
          {h && row(h.raqm, h.raqm ? "עיצוב טקסט עברי (HarfBuzz + FriBidi) זמין" : `libraqm/fribidi חסר: העברית לא תעומד. ${platform === "windows" ? "התקינו מחדש את האפליקציה (fribidi-0.dll חסר)" : "התקן: brew install fribidi"}`)}
          {h && row(h.models.detector, "מודל זיהוי בועות")}
          {h && row(h.models.ocr, "מודל OCR ליפנית (manga-ocr)")}
          {h && row(h.models.ocr_en, "מודל OCR לאנגלית (TrOCR, כ-1.3GB)")}
          {h && row(h.models.lama, "מודל ניקוי (LaMa)")}
          {s.error && s.installed && <div className="callout bad small">{s.error}</div>}
          {gpuOffered && (
            <label className="check small" style={{ marginTop: 8 }}>
              <input type="checkbox" checked={gpu} onChange={(e) => setGpuChoice(e.target.checked)} />
              <span>להשתמש בכרטיס המסך NVIDIA ({s.gpus[0].replace(/^NVIDIAs+/i, "")}) — הורדה גדולה יותר (כ-2.5GB), עיבוד מהיר בהרבה</span>
            </label>
          )}
          <div className="row wrap" style={{ marginTop: 8 }}>
            <button className="btn" disabled={!!busy || !s.uv} onClick={() => run("מתקין… (עלול לקחת כמה דקות)", () => api.installEngine(gpu))}>
              {s.installed ? "התקנה מחדש של סביבת Python" : "התקנת סביבת Python"}
            </button>
            <button className="btn" disabled={!!busy || !s.installed || (h?.models.detector && h?.models.ocr && h?.models.ocr_en && h?.models.lama)} onClick={() => run("מוריד מודלים…", api.prepareModels)}>
              הורדת מודלים חסרים
            </button>
            {!s.uv && <span className="small" style={{ color: "var(--bad)" }}>נדרש uv להתקנה ({platform === "windows" ? "winget install astral-sh.uv" : "brew install uv"})</span>}
          </div>
          <span className="small faint ltr" style={{ textAlign: "start" }}>{s.python}</span>
        </div>
      )}
    </div>
  );
}
