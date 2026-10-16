import { create } from "zustand";
import { check, type Update } from "@tauri-apps/plugin-updater";

const LAST_CHECK = "mangaai.update.lastCheck";
const DAY_MS = 24 * 60 * 60 * 1000;

/** The update waiting for the user's answer; UpdatePrompt shows it. */
export const useUpdate = create<{ update: Update | null; set: (u: Update | null) => void }>((set) => ({
  update: null,
  set: (update) => set({ update }),
}));

/** Asks the GitHub release feed for a newer version. Returns it (and opens the prompt) or null. */
export async function checkForUpdate(): Promise<Update | null> {
  const update = await check();
  if (update) useUpdate.getState().set(update);
  return update;
}

/** Startup check, at most once a day; never shows an error (offline is normal, and retried next launch). */
export function checkOnStartup(now = Date.now()) {
  let last = 0;
  try {
    last = Number(localStorage.getItem(LAST_CHECK) ?? 0);
  } catch {}
  if (now - last < DAY_MS) return;
  checkForUpdate()
    .then(() => {
      try {
        localStorage.setItem(LAST_CHECK, String(now));
      } catch {}
    })
    .catch(() => {});
}
