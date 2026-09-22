import { describe, it, expect } from "vitest";
import { safeName } from "@/features/export/pageExport";

describe("safeName", () => {
  it("keeps letters and digits from any script", () => {
    expect(safeName("Résumé déjà vu")).toBe("Résumé déjà vu");
    expect(safeName("会議メモ 2026")).toBe("会議メモ 2026");
    expect(safeName("Ελληνικά")).toBe("Ελληνικά");
  });

  it("keeps hyphens and underscores", () => {
    expect(safeName("client-acme_notes")).toBe("client-acme_notes");
  });

  it("strips path and shell characters", () => {
    expect(safeName("../etc/passwd")).toBe("etcpasswd");
    expect(safeName("Notes: Q1/Q2 *draft*")).toBe("Notes Q1Q2 draft");
  });

  it("falls back to Untitled after sanitising, not before", () => {
    expect(safeName("///")).toBe("Untitled");
    expect(safeName("***")).toBe("Untitled");
    expect(safeName("   ")).toBe("Untitled");
    expect(safeName("")).toBe("Untitled");
  });

  it("caps the length and trims what is left", () => {
    expect(safeName("a".repeat(200))).toHaveLength(80);
    expect(safeName(`${"b".repeat(79)}   tail`)).toBe("b".repeat(79));
  });
});
