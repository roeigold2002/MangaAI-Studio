import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { api, errCode, errMessage } from "./api";
import type { Settings } from "./types";

interface Toast { id: number; text: string; kind: "info" | "ok" | "bad" }

interface AppStore {
  settings: Settings | null;
  toasts: Toast[];
  loadSettings: () => Promise<void>;
  saveSettings: (s: Settings) => Promise<Settings>;
  toast: (text: string, kind?: Toast["kind"]) => void;
}

let toastId = 0;

export const useApp = create<AppStore>((set, get) => ({
  settings: null,
  toasts: [],
  loadSettings: async () => {
    const s = await api.getSettings();
    set({ settings: s });
    document.documentElement.dataset.theme = s.theme;
  },
  saveSettings: async (s) => {
    const saved = await api.saveSettings(s);
    set({ settings: saved });
    document.documentElement.dataset.theme = saved.theme;
    return saved;
  },
  toast: (text, kind = "info") => {
    const id = ++toastId;
    set({ toasts: [...get().toasts, { id, text, kind }] });
    setTimeout(() => set({ toasts: get().toasts.filter((t) => t.id !== id) }), kind === "bad" ? 7000 : 3500);
  },
}));

export const toast = (text: string, kind?: Toast["kind"]) => useApp.getState().toast(text, kind);
export const toastError = (e: unknown) => useApp.getState().toast(errMessage(e), "bad");

/** Load data with a reload function; errors are kept, not thrown. */
export function useLoad<T>(fn: () => Promise<T>, deps: unknown[]) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const seq = useRef(0);
  const reload = useCallback(async () => {
    const n = ++seq.current;
    setLoading(true);
    try {
      const v = await fn();
      if (n === seq.current) {
        setData(v);
        setError(null);
        setErrorCode(null);
      }
    } catch (e) {
      if (n === seq.current) {
        setError(errMessage(e));
        setErrorCode(errCode(e));
      }
    } finally {
      if (n === seq.current) setLoading(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  useEffect(() => {
    reload();
  }, [reload]);
  return { data, error, errorCode, loading, reload, setData };
}

/** Subscribe to backend events; the handler is throttled to one call per animation frame. */
export function useEvent<T>(name: string, handler: (payload: T) => void) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    let off: (() => void) | undefined;
    let alive = true;
    listen<T>(name, (e) => ref.current(e.payload)).then((u) => {
      if (alive) off = u;
      else u();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, [name]);
}

/** Calls `fn` at most once per `ms` while events keep coming. */
export function useThrottled(fn: () => void, ms: number) {
  const t = useRef<number | null>(null);
  const f = useRef(fn);
  f.current = fn;
  return useCallback(() => {
    if (t.current !== null) return;
    t.current = window.setTimeout(() => {
      t.current = null;
      f.current();
    }, ms);
  }, [ms]);
}
