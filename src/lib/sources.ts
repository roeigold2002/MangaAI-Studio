import type { SourceOverview } from "./types";

/** What stands between the user and online search, and the shortest way to fix it. */
export type SourceState = "ready" | "local_only" | "disabled" | "not_installed" | "stopped" | "auth" | "error";

export function sourceState(o: SourceOverview | null): SourceState {
  if (!o) return "error";
  if (!o.enabled) return "disabled";
  const h = o.health;
  if (!h || h.server === "offline") {
    if (o.managed && o.runtime.installed) return "stopped";
    return "not_installed";
  }
  if (h.server === "auth") return "auth";
  if (h.server !== "ok") return "error";
  return h.online_sources > 0 ? "ready" : "local_only";
}

