import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"
import { useCallback, type Ref } from "react"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/** Assign one node to every ref in the list (callback or object refs). */
function assignRefs<T>(refs: (Ref<T> | undefined)[], node: T | null) {
  for (const ref of refs) {
    if (!ref) continue
    if (typeof ref === "function") ref(node)
    else (ref as { current: T | null }).current = node
  }
}

/** Compose multiple refs (callback or object) onto one element. */
export function mergeRefs<T>(...refs: (Ref<T> | undefined)[]) {
  return (node: T | null) => assignRefs(refs, node)
}

/**
 * Stable version of `mergeRefs` for components: the callback identity only
 * changes when one of the composed refs does, so libraries that own a ref
 * (dnd-kit, for one) aren't detached and re-attached on every render.
 */
export function useMergedRefs<T>(...refs: (Ref<T> | undefined)[]) {
  // The spread is fixed-length at a given call site, so it's a valid dep list.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useCallback((node: T | null) => assignRefs(refs, node), refs)
}
