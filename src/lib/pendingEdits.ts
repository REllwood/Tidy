const savers = new Set<() => Promise<void>>();
const writes = new Set<Promise<unknown>>();
export function registerPendingEdits(save: () => Promise<void>) {
  savers.add(save);
  return () => {
    savers.delete(save);
  };
}
export function trackWrite<T>(write: Promise<T>): Promise<T> {
  writes.add(write);
  void write.then(
    () => writes.delete(write),
    () => writes.delete(write),
  );
  return write;
}
export async function saveBeforeRestart() {
  await Promise.all([...savers].map((save) => save()));
  await Promise.all([...writes]);
}
