import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: () => "mangaimg://localhost/" }));

import { arrowStep, chapterTranslationBadge, credentialStoreName, detectPlatform, imgUrl, publicationStatus, shortTitles, spreads, usd } from "./util";

const p = (w = 100, h = 140) => ({ width: w, height: h });

describe("spreads", () => {
  it("keeps the cover alone and pairs the rest", () => {
    expect(spreads([p(), p(), p(), p(), p()], true)).toEqual([[0], [1, 2], [3, 4]]);
  });
  it("puts wide (double-page) images alone", () => {
    expect(spreads([p(), p(), p(300, 140), p(), p()], false)).toEqual([[0, 1], [2], [3, 4]]);
  });
  it("handles odd endings", () => {
    expect(spreads([p(), p(), p()], false)).toEqual([[0, 1], [2]]);
  });
});

describe("arrowStep", () => {
  it("follows reading direction", () => {
    expect(arrowStep("ArrowLeft", "rtl")).toBe(1);
    expect(arrowStep("ArrowLeft", "ltr")).toBe(-1);
    expect(arrowStep("ArrowRight", "rtl")).toBe(-1);
    expect(arrowStep("x", "rtl")).toBe(0);
  });
});

describe("misc", () => {
  it("builds protocol urls", () => {
    expect(imgUrl("/page/3")).toBe("mangaimg://localhost/page/3");
  });
  it("formats money honestly", () => {
    expect(usd(null)).toBe("לא ידוע");
    expect(usd(0.004)).toBe("פחות מ-$0.01");
    expect(usd(1.234)).toBe("$1.23");
  });
  it("chapter badge reflects real page results", () => {
    expect(chapterTranslationBadge({ total: 3, done: 3, failed: 0, in_progress: 0 })?.text).toBe("מתורגם");
    expect(chapterTranslationBadge({ total: 3, done: 2, failed: 1, in_progress: 0 })?.cls).toBe("bad");
    expect(chapterTranslationBadge({ total: 3, done: 0, failed: 0, in_progress: 0 })).toBeNull();
  });
});

describe("shortTitles", () => {
  it("strips a shared series prefix at a word boundary", () => {
    expect(shortTitles(["Kuso Onna v01", "Kuso Onna v02", "Kuso Onna v10"])).toEqual(["v01", "v02", "v10"]);
  });
  it("keeps titles that share nothing meaningful", () => {
    expect(shortTitles(["Ch 1", "Ch 2"])).toEqual(["Ch 1", "Ch 2"]);
    expect(shortTitles(["One"])).toEqual(["One"]);
  });
});

describe("publicationStatus", () => {
  it("translates known values and hides unknown", () => {
    expect(publicationStatus("ONGOING")).toBe("בהמשכים");
    expect(publicationStatus("UNKNOWN")).toBe("");
    expect(publicationStatus(null)).toBe("");
  });
});

describe("platform", () => {
  it("detects the OS from the webview user agent", () => {
    expect(detectPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/155.0")).toBe("windows");
    expect(detectPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")).toBe("mac");
    expect(detectPlatform("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15")).toBe("linux");
  });
  it("names the credential store of each OS", () => {
    expect(credentialStoreName("windows")).toContain("Windows");
    expect(credentialStoreName("mac")).toContain("Keychain");
  });
});
