import { beforeEach, describe, expect, it, vi } from "vitest";

const check = vi.fn();
vi.mock("@tauri-apps/plugin-updater", () => ({ check: () => check() }));

const { checkOnStartup, useUpdate } = await import("./updates");
const flush = () => new Promise((r) => setTimeout(r, 0));
const DAY = 24 * 60 * 60 * 1000;

describe("startup update check", () => {
  let store: Record<string, string>;
  beforeEach(() => {
    store = {};
    vi.stubGlobal("localStorage", { getItem: (k: string) => store[k] ?? null, setItem: (k: string, v: string) => (store[k] = v) });
    check.mockReset();
    useUpdate.getState().set(null);
  });

  it("checks at most once a day and opens the prompt for a newer version", async () => {
    check.mockResolvedValue({ version: "0.2.0" });
    checkOnStartup(10 * DAY);
    await flush();
    expect(check).toHaveBeenCalledTimes(1);
    expect(useUpdate.getState().update).toEqual({ version: "0.2.0" });

    checkOnStartup(10 * DAY + DAY - 1);
    expect(check).toHaveBeenCalledTimes(1);
    checkOnStartup(11 * DAY);
    expect(check).toHaveBeenCalledTimes(2);
  });

  it("a failed check (offline) is retried on the next launch", async () => {
    check.mockRejectedValue(new Error("offline"));
    checkOnStartup(10 * DAY);
    await flush();
    checkOnStartup(10 * DAY + 1000);
    expect(check).toHaveBeenCalledTimes(2);
    expect(useUpdate.getState().update).toBeNull();
  });
});
