import { useCallback, useRef, useState } from "react";
import { useUi, SIDEBAR_BOUNDS } from "@/store/ui";

const clamp = (w: number) =>
  Math.min(SIDEBAR_BOUNDS.max, Math.max(SIDEBAR_BOUNDS.min, Math.round(w)));

/**
 * Thin draggable divider that resizes the sidebar (pointer + keyboard).
 * The drag is previewed straight onto the layout and only committed to the
 * store on release, so one drag writes to storage once.
 */
export function Splitter({
  onPreview,
}: {
  /** Live width during a drag; the store is left alone until pointer-up. */
  onPreview?: (width: number) => void;
}) {
  const setSidebarWidth = useUi((s) => s.setSidebarWidth);
  const storedWidth = useUi((s) => s.sidebarWidth);
  const [dragWidth, setDragWidth] = useState<number | null>(null);
  const liveRef = useRef(storedWidth);

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      e.preventDefault();
      const startX = e.clientX;
      const startW = useUi.getState().sidebarWidth;
      const handle = e.currentTarget;
      handle.setPointerCapture(e.pointerId);
      liveRef.current = startW;

      const onMove = (ev: PointerEvent) => {
        const width = clamp(startW + (ev.clientX - startX));
        liveRef.current = width;
        setDragWidth(width);
        onPreview?.(width);
      };
      const onUp = (ev: PointerEvent) => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointercancel", onUp);
        try {
          handle.releasePointerCapture(ev.pointerId);
        } catch {
          /* capture may already be released */
        }
        setDragWidth(null);
        setSidebarWidth(liveRef.current); // the one persisted write
      };
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
      window.addEventListener("pointercancel", onUp);
    },
    [setSidebarWidth, onPreview],
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
      aria-valuenow={dragWidth ?? storedWidth}
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
