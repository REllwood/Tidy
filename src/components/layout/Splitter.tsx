import { useCallback, useRef } from "react";
import { useUi, SIDEBAR_BOUNDS } from "@/store/ui";

/** Thin draggable divider that resizes the sidebar (pointer + keyboard). */
export function Splitter() {
  const setSidebarWidth = useUi((s) => s.setSidebarWidth);
  const widthRef = useRef(useUi.getState().sidebarWidth);

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      e.preventDefault();
      const startX = e.clientX;
      const startW = useUi.getState().sidebarWidth;
      (e.target as HTMLElement).setPointerCapture(e.pointerId);

      const onMove = (ev: PointerEvent) => {
        widthRef.current = startW + (ev.clientX - startX);
        setSidebarWidth(widthRef.current);
      };
      const onUp = (ev: PointerEvent) => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        try {
          (e.target as HTMLElement).releasePointerCapture(ev.pointerId);
        } catch {
          /* capture may already be released */
        }
      };
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    },
    [setSidebarWidth],
  );

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      const cur = useUi.getState().sidebarWidth;
      if (e.key === "ArrowLeft") setSidebarWidth(cur - 16);
      else if (e.key === "ArrowRight") setSidebarWidth(cur + 16);
    },
    [setSidebarWidth],
  );

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      aria-valuemin={SIDEBAR_BOUNDS.min}
      aria-valuemax={SIDEBAR_BOUNDS.max}
      aria-valuenow={useUi((s) => s.sidebarWidth)}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
      className="group relative w-px shrink-0 cursor-col-resize bg-border outline-none"
    >
      {/* widened invisible hit-area + hover/focus highlight */}
      <span className="absolute inset-y-0 -left-1 -right-1 group-hover:bg-brand/30 group-focus-visible:bg-brand/40" />
    </div>
  );
}
