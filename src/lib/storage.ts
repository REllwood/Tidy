import previous from "../../compatibility/previous-installation.json";

/** Copy old browser preferences only when the new key has never been written. */
export function readStoredValue(
  storage: Pick<Storage, "getItem" | "setItem">,
  name: string,
): string | null {
  const current = storage.getItem(name);
  if (current !== null) return current;
  const legacy =
    previous.storage_keys[name as keyof typeof previous.storage_keys];
  const value = legacy ? storage.getItem(legacy) : null;
  if (value !== null) storage.setItem(name, value);
  return value;
}
