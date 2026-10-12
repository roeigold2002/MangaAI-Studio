import { describe, expect, it } from "vitest";
import { sourceState } from "./sources";
import type { SourceOverview } from "./types";

const base: SourceOverview = {
  enabled: true, url: "http://127.0.0.1:4567", managed: false, runtime_error: null, local_dir: null,
  runtime: { supported: true, installed: false, running: false, version: "v2.4.2366", install_dir: "", download_size_mb: 330 },
  health: { server: "ok", server_version: "x", message: null, sources: 2, online_sources: 1, website_sources: 1, stores: 1, installed_extensions: 1 },
};

describe("sourceState", () => {
  it("is ready only with online sources", () => {
    expect(sourceState(base)).toBe("ready");
    expect(sourceState({ ...base, health: { ...base.health!, online_sources: 0, website_sources: 0 } })).toBe("local_only");
    // only the Suwayomi bridge installed: still no manga websites
    expect(sourceState({ ...base, health: { ...base.health!, online_sources: 1, website_sources: 0 } })).toBe("local_only");
  });
  it("distinguishes not installed, stopped, disabled, auth", () => {
    const offline = { ...base, health: { ...base.health!, server: "offline" as const } };
    expect(sourceState(offline)).toBe("not_installed");
    expect(sourceState({ ...offline, managed: true, runtime: { ...base.runtime, installed: true } })).toBe("stopped");
    expect(sourceState({ ...base, enabled: false })).toBe("disabled");
    expect(sourceState({ ...base, health: { ...base.health!, server: "auth" } })).toBe("auth");
    expect(sourceState(null)).toBe("error");
  });
});
