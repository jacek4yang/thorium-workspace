import { describe, expect, it } from "vitest";

import {
  describeProgress,
  formatBytes,
  formatRelative,
  formatTimestamp,
  groupCode,
  passwordHint,
  serviceLabel,
  vaultSummary,
} from "./format";

describe("formatBytes", () => {
  it("scales through the units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(188_743_680)).toBe("180 MB");
    expect(formatBytes(3 * 1024 ** 3)).toBe("3.0 GB");
  });

  it("does not invent a size it does not have", () => {
    expect(formatBytes(-1)).toBe("unknown");
    expect(formatBytes(Number.NaN)).toBe("unknown");
  });
});

describe("timestamps", () => {
  it("renders an absent time as never rather than 1970", () => {
    expect(formatTimestamp(null)).toBe("never");
    expect(formatTimestamp(0)).toBe("never");
    expect(formatTimestamp(undefined)).toBe("never");
  });

  it("renders a real time", () => {
    expect(formatTimestamp(1_700_000_000)).not.toBe("never");
  });
});

describe("formatRelative", () => {
  const now = 1_700_000_000;

  it("describes recent times in words", () => {
    expect(formatRelative(now, now)).toBe("just now");
    expect(formatRelative(now - 120, now)).toBe("2 min ago");
    expect(formatRelative(now - 7200, now)).toBe("2 h ago");
    expect(formatRelative(now - 86_400, now)).toBe("yesterday");
    expect(formatRelative(now - 3 * 86_400, now)).toBe("3 days ago");
  });

  it("never shows a negative age for a clock skew", () => {
    expect(formatRelative(now + 500, now)).toBe("just now");
  });
});

describe("serviceLabel", () => {
  it("names the built-in presets and falls back for others", () => {
    expect(serviceLabel({ kind: "git_hub" })).toBe("GitHub");
    expect(serviceLabel({ kind: "microsoft" })).toBe("Microsoft");
    expect(serviceLabel({ kind: "other", label: "Fastmail" })).toBe("Fastmail");
    expect(serviceLabel({ kind: "other", label: "" })).toBe("Other");
  });
});

describe("vaultSummary", () => {
  it("summarises each state and pluralises correctly", () => {
    expect(vaultSummary({ state: "uninitialized" })).toBe("No vault yet");
    expect(vaultSummary({ state: "locked", reason: "idle" })).toBe("Locked");
    expect(
      vaultSummary({
        state: "unlocked",
        secret_count: 1,
        unlocked_at: 1,
        idle_lock_seconds: 600,
      }),
    ).toBe("Unlocked, 1 secret");
    expect(
      vaultSummary({
        state: "unlocked",
        secret_count: 7,
        unlocked_at: 1,
        idle_lock_seconds: null,
      }),
    ).toBe("Unlocked, 7 secrets");
  });
});

describe("describeProgress", () => {
  it("describes every install stage", () => {
    expect(describeProgress({ stage: "resolving" }).fraction).toBeNull();
    const downloading = describeProgress({
      stage: "downloading",
      received: 50,
      total: 200,
    });
    expect(downloading.fraction).toBeCloseTo(0.25);
    expect(downloading.label).toContain("Downloading");

    expect(
      describeProgress({ stage: "downloading", received: 50, total: null }).fraction,
    ).toBeNull();
    expect(describeProgress({ stage: "extracting", done: 5, total: 10 }).fraction).toBe(0.5);
    expect(describeProgress({ stage: "done", version: "M152" }).fraction).toBe(1);
    expect(describeProgress({ stage: "done", version: "M152" }).label).toContain("M152");
  });

  it("cannot divide by zero on an empty archive", () => {
    expect(describeProgress({ stage: "extracting", done: 0, total: 0 }).fraction).toBeNull();
  });
});

describe("groupCode", () => {
  it("groups the standard digit counts and leaves others alone", () => {
    expect(groupCode("123456")).toBe("123 456");
    expect(groupCode("12345678")).toBe("1234 5678");
    expect(groupCode("1234567")).toBe("1234567");
  });
});

describe("passwordHint", () => {
  it("refuses to call a short password anything but too short", () => {
    expect(passwordHint("").level).toBe(0);
    expect(passwordHint("short").label).toContain("Too short");
    expect(passwordHint("elevenchars").level).toBe(0);
  });

  it("rates longer passphrases higher without demanding symbols", () => {
    expect(passwordHint("twelvechars!").level).toBeGreaterThan(0);
    expect(passwordHint("correct horse battery staple").level).toBe(3);
    expect(passwordHint("Tr0ub4dor&3xxxxxx").level).toBe(3);
  });

  it("counts characters rather than bytes", () => {
    // Twelve emoji is twelve characters, even though it is far more bytes.
    expect(passwordHint("🔒".repeat(12)).level).toBeGreaterThan(0);
  });
});
