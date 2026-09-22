import type { ModelInfo } from "@/lib/api";
import type { ModelRange } from "./ModelGuidance";

export interface VisibleModel {
  model: ModelInfo;
  /** Shown only because it is the selected model, not because it matches the filter. */
  outsideRange: boolean;
}

/**
 * Filter the model list by memory range while always keeping the selected
 * model on screen: hiding the model a Mac actually transcribes with makes the
 * filter look like it changed the setting.
 */
export function visibleModels(
  models: ModelInfo[],
  range: ModelRange,
): VisibleModel[] {
  const matches = (model: ModelInfo) => range === "all" || model.tier === range;
  return models
    .filter((model) => matches(model) || model.selected)
    .map((model) => ({ model, outsideRange: !matches(model) }));
}
