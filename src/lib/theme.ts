import { useEffect, useState } from "react";
import { useUi, type Theme } from "@/store/ui";

const DARK_QUERY = "(prefers-color-scheme: dark)";

const darkMedia = (): MediaQueryList | null =>
  typeof window === "undefined" ? null : (window.matchMedia?.(DARK_QUERY) ?? null);

const prefersDark = () => darkMedia()?.matches ?? false;

/** Non-React resolution (event handlers, one-off checks). */
export function resolveTheme(theme: Theme): "light" | "dark" {
  if (theme === "system") return prefersDark() ? "dark" : "light";
  return theme;
}

function apply(theme: Theme) {
  const resolved = resolveTheme(theme);
  const root = document.documentElement;
  root.classList.toggle("dark", resolved === "dark");
  // Keeps native form controls, scrollbars and the window chrome in step.
  root.style.colorScheme = resolved;
}

/**
 * The theme actually in effect, as React state: re-renders on both a store
 * change and an OS appearance change, so components can paint with it.
 */
export function useResolvedTheme(): "light" | "dark" {
  const theme = useUi((s) => s.theme);
  const [systemDark, setSystemDark] = useState(prefersDark);

  useEffect(() => {
    const mq = darkMedia();
    if (!mq) return;
    setSystemDark(mq.matches); // catch a change between first render and mount
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  if (theme === "system") return systemDark ? "dark" : "light";
  return theme;
}

/** Mount once near the app root: applies the theme and follows system changes. */
export function useThemeEffect() {
  const theme = useUi((s) => s.theme);
  useEffect(() => {
    apply(theme);
    if (theme !== "system") return;
    const mq = darkMedia();
    if (!mq) return;
    const onChange = () => apply("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [theme]);
}
