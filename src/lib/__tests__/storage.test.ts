import { describe, expect, it } from "vitest";
import { readStoredValue } from "../storage";
import previous from "../../../compatibility/previous-installation.json";
describe("existing preferences", () => {
  it("copies the previous value without removing it, and never replaces newer preferences", () => {
    const oldKey = previous.storage_keys["tidy-ui"];
    const values = new Map([[oldKey, "old preferences"]]);
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => {
        values.set(key, value);
      },
    };
    expect(readStoredValue(storage, "tidy-ui")).toBe("old preferences");
    expect(values.get(oldKey)).toBe("old preferences");
    storage.setItem("tidy-ui", "new preferences");
    expect(readStoredValue(storage, "tidy-ui")).toBe("new preferences");
    expect(readStoredValue(storage, "unrelated")).toBeNull();
  });
});
