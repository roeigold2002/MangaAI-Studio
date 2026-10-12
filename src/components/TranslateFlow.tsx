import { useEffect, useState } from "react";
import { api, errMessage } from "../lib/api";
import { toast, useApp } from "../lib/store";
import { chapterTranslationBadge, langName } from "../lib/util";
import { Field, Modal, Spinner } from "./ui";
import { TranslateDialog } from "./TranslateDialog";
import type { ChapterInfo, ProviderStatus } from "../lib/types";

const CHOICES = [
  { id: "claude_cli", title: "Claude Code", note: "משתמש בחשבון Claude שכבר מחובר במחשב. בלי מפתח API.", key: false, model: "opus" },
  { id: "anthropic", title: "Anthropic API", note: "צריך מפתח API מ-console.anthropic.com. תשלום לפי שימוש.", key: true, model: "claude-opus-5-5" },
  { id: "openai", title: "OpenAI API", note: "צריך מפתח API ושם דגם שתומך בתמונות.", key: true, model: "" },
  { id: "gemini", title: "Google Gemini", note: "צריך מפתח API מ-Google AI Studio ושם דגם.", key: true, model: "" },
  { id: "ollama", title: "Ollama (מקומי)", note: "רץ על המחשב בלי לשלוח כלום החוצה. איכות העברית תלויה בדגם.", key: false, model: "" },
] as const;

const MISSING: Record<string, string> = {
  no_key: "חסר מפתח API",
  no_model: "לא נבחר דגם",
  cli_missing: "Claude Code לא נמצא במחשב",
  cloud_notice: "צריך לאשר שליחת תוכן לספק",
  local_only: "מצב מקומי בלבד מופעל, והספק הזה בענן",
  no_provider: "לא נבחר ספק",
};

/** Short, beginner-friendly setup that saves one provider and returns. */
export function ProviderSetup({ status, onDone, onClose }: { status: ProviderStatus | null; onDone: () => void; onClose: () => void }) {
  const { settings, saveSettings } = useApp();
  const [choice, setChoice] = useState<string>(status?.provider_id || "claude_cli");
  const cfg = settings!.translation.providers.find((p) => p.id === choice)!;
  const meta = CHOICES.find((c) => c.id === choice)!;
  const [model, setModel] = useState(cfg?.model || meta.model);
  const [key, setKey] = useState("");
  const [accept, setAccept] = useState(settings!.cloud_notice_accepted);
  const [models, setModels] = useState<string[]>([]);
  const [cli, setCli] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const local = choice === "ollama";

  useEffect(() => {
    const c = settings!.translation.providers.find((p) => p.id === choice);
    setModel(c?.model || CHOICES.find((x) => x.id === choice)!.model);
    setError(null);
    if (choice === "ollama") api.ollamaModels(c?.endpoint || "http://127.0.0.1:11434").then(setModels).catch(() => setModels([]));
    if (choice === "claude_cli") api.claudeCliStatus(c?.endpoint).then((r) => setCli(r.found)).catch(() => setCli(false));
  }, [choice, settings]);

  async function save() {
    setBusy(true);
    setError(null);
    try {
      if (meta.key && key.trim()) await api.setApiKey(choice, key.trim());
      const s = settings!;
      await saveSettings({
        ...s,
        cloud_notice_accepted: local ? s.cloud_notice_accepted : accept,
        translation: {
          ...s.translation,
          active_provider: choice,
          providers: s.translation.providers.map((p) => (p.id === choice ? { ...p, model } : p)),
        },
      });
      const st = await api.providerStatus();
      if (!st.ready) {
        setError(st.missing.map((m) => MISSING[m] ?? m).join(" · "));
        return;
      }
      toast("ספק התרגום נשמר", "ok");
      onDone();
    } catch (e) {
      setError(errMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal title="איך לתרגם?" onClose={onClose}
      actions={<>
        <button className="btn ghost" onClick={onClose}>אחר כך</button>
        <button className="btn primary" disabled={busy} onClick={save}>{busy ? "שומר…" : "שמירה והמשך"}</button>
      </>}>
      <p className="muted small">
        זיהוי הטקסט וציור העברית על העמוד קורים במחשב שלך. לתרגום עצמו צריך לבחור מודל שפה. אפשר לשנות את הבחירה אחר כך בהגדרות.
      </p>
      <div className="col" style={{ gap: 6 }}>
        {CHOICES.map((c) => (
          <label key={c.id} className="list-row clickable" style={{ border: "1px solid var(--line)", borderRadius: 8, borderColor: c.id === choice ? "var(--marker)" : undefined }}>
            <input type="radio" name="prov" checked={c.id === choice} onChange={() => setChoice(c.id)} style={{ accentColor: "var(--marker)" }} />
            <div className="col grow" style={{ gap: 2 }}>
              <strong>{c.title}</strong>
              <span className="small muted">{c.note}</span>
            </div>
            {c.key ? <span className="badge">דורש מפתח</span> : <span className="badge ok">בלי מפתח</span>}
          </label>
        ))}
      </div>
      {choice === "claude_cli" && cli === false && (
        <div className="callout bad small">Claude Code לא נמצא במחשב. התקן אותו והתחבר (פקודה: claude), או בחר ספק אחר.</div>
      )}
      {meta.key && (
        <Field label="מפתח API" hint="נשמר ב-Keychain של macOS בלבד">
          <input className="input ltr" type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} placeholder="מדביקים כאן" />
        </Field>
      )}
      {choice === "ollama" ? (
        <Field label="דגם" hint={models.length ? undefined : "Ollama לא זמין או שאין בו דגמים מותקנים (ollama pull …)"}>
          <select className="select ltr" value={model} onChange={(e) => setModel(e.target.value)}>
            <option value="">בחר דגם</option>
            {models.map((m) => <option key={m} value={m}>{m}</option>)}
          </select>
        </Field>
      ) : (
        (choice === "openai" || choice === "gemini") && (
          <Field label="שם הדגם"><input className="input ltr" value={model} onChange={(e) => setModel(e.target.value)} /></Field>
        )
      )}
      {!local && (
        <label className="check small">
          <input type="checkbox" checked={accept} onChange={(e) => setAccept(e.target.checked)} />
          <span>מאשר לשלוח לספק את הטקסט שזוהה בעמודים (ואת תמונות העמודים, לשיפור הדיוק) כשאני מבקש תרגום.</span>
        </label>
      )}
      {error && <div className="callout bad small" dir="auto">{error}</div>}
    </Modal>
  );
}

/** "תרגם לעברית": provider check → (setup) → choose chapters → estimate & start. */
export function TranslateFlow({ chapters, preselect, onClose }: { chapters: ChapterInfo[]; preselect?: number[]; onClose: () => void }) {
  const [status, setStatus] = useState<ProviderStatus | null>(null);
  const [step, setStep] = useState<"check" | "setup" | "pick" | "confirm">("check");
  const untranslated = chapters.filter((c) => c.translation.done < c.page_count);
  const [sel, setSel] = useState<Set<number>>(new Set(preselect ?? (untranslated[0] ? [untranslated[0].id] : [])));
  const [lang, setLang] = useState<string | null>(null);
  useEffect(() => {
    if (chapters[0]) api.mangaDetail(chapters[0].manga_id).then((m) => setLang(m.source_lang)).catch(() => {});
  }, [chapters]);
  const langWarning = lang && lang !== "ja" ? (
    <div className="callout bad small">
      המנגה הזו ב{langName(lang)}. זיהוי הטקסט (OCR) תומך כרגע רק בעמודים ביפנית, ולכן התרגום של העמודים האלה צפוי להיכשל או לצאת שגוי.
    </div>
  ) : null;

  useEffect(() => {
    api.providerStatus().then((s) => {
      setStatus(s);
      setStep(s.ready ? (preselect && preselect.length ? "confirm" : "pick") : "setup");
    }).catch(() => setStep("setup"));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (step === "check") return <Modal title="תרגום לעברית" onClose={onClose}><Spinner label="בודק את הגדרות התרגום…" /></Modal>;
  if (step === "setup") {
    return <ProviderSetup status={status} onClose={onClose} onDone={() => setStep(preselect && preselect.length ? "confirm" : "pick")} />;
  }
  if (step === "confirm") return <TranslateDialog chapterIds={[...sel]} onClose={onClose} warning={langWarning} />;
  return (
    <Modal title="אילו פרקים לתרגם?" onClose={onClose}
      actions={<>
        <button className="btn ghost" onClick={onClose}>ביטול</button>
        <button className="btn primary" disabled={!sel.size} onClick={() => setStep("confirm")}>המשך ({sel.size})</button>
      </>}>
      {langWarning}
      <div className="row wrap">
        <button className="btn sm ghost" onClick={() => setSel(new Set(untranslated.map((c) => c.id)))}>כל מה שעוד לא תורגם ({untranslated.length})</button>
        <button className="btn sm ghost" onClick={() => setSel(new Set())}>ניקוי</button>
      </div>
      <div className="panel" style={{ padding: 0, maxHeight: 360, overflow: "auto" }}>
        {chapters.map((c) => {
          const b = chapterTranslationBadge(c.translation);
          return (
            <label key={c.id} className="list-row clickable">
              <input type="checkbox" checked={sel.has(c.id)} style={{ accentColor: "var(--marker)" }}
                onChange={(e) => { const s = new Set(sel); if (e.target.checked) s.add(c.id); else s.delete(c.id); setSel(s); }} />
              <span className="grow ellipsis" dir="auto">{c.title}</span>
              <span className="faint small">{c.page_count} עמ׳</span>
              {b && <span className={`badge ${b.cls}`}>{b.text}</span>}
            </label>
          );
        })}
      </div>
    </Modal>
  );
}
