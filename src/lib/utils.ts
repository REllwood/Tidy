import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"
import type { Ref } from "react"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/** Compose multiple refs (callback or object) onto one element. */
export function mergeRefs<T>(...refs: (Ref<T> | undefined)[]) {
  return (node: T | null) => {
    for (const ref of refs) {
      if (!ref) continue
      if (typeof ref === "function") ref(node)
      else (ref as { current: T | null }).current = node
    }
  }
}
