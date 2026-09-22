import { describe, expect, it } from "vitest";
import { visibleModels } from "@/features/settings/modelVisibility";
import { isUserProgress, isWorkerProgress, type AiProgress } from "@/lib/localAi";
import type { ModelInfo } from "@/lib/api";

const model = (
  id: string,
  tier: string,
  extra: Partial<ModelInfo> = {},
): ModelInfo => ({
  description: id,
  recommended_ram_gb: 8,
  tier,
  id,
  name: id,
  size: 1e9,
  downloaded: false,
  selected: false,
  ...extra,
});

const progress = (origin: "user" | "worker"): AiProgress => ({
  label: "Working",
  model_id: null,
  completed: 5,
  total: 10,
  origin,
});

describe("transcription model filter", () => {
  const models = [
    model("tiny", "light"),
    model("medium", "power", { downloaded: true, selected: true }),
  ];

  it("keeps the selected model on screen and flags it as outside the range", () => {
    const visible = visibleModels(models, "light");
    expect(visible.map((v) => v.model.id)).toEqual(["tiny", "medium"]);
    expect(visible.find((v) => v.model.id === "medium")?.outsideRange).toBe(
      true,
    );
    expect(visible.find((v) => v.model.id === "tiny")?.outsideRange).toBe(false);
  });

  it("flags nothing when the range already includes the selected model", () => {
    expect(visibleModels(models, "all").every((v) => !v.outsideRange)).toBe(
      true,
    );
    expect(
      visibleModels(models, "power").map((v) => [v.model.id, v.outsideRange]),
    ).toEqual([["medium", false]]);
  });
});

describe("local AI progress origin", () => {
  it("separates work the person started from background meeting work", () => {
    expect(isUserProgress(progress("user"))).toBe(true);
    expect(isWorkerProgress(progress("user"))).toBe(false);
    expect(isUserProgress(progress("worker"))).toBe(false);
    expect(isWorkerProgress(progress("worker"))).toBe(true);
    expect(isUserProgress(null)).toBe(false);
    expect(isWorkerProgress(undefined)).toBe(false);
  });
});
