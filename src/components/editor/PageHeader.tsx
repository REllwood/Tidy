import { useEffect, useRef, useState } from "react";
import { Smile } from "lucide-react";
import { usePages, useRenamePage, useSetPageIcon } from "@/hooks/usePages";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

const EMOJI = [
  "📝", "📄", "📒", "📓", "📔", "🗂️", "🗒️", "📅", "✅", "💡",
  "🚀", "🔥", "⭐", "🎯", "🧠", "📌", "🎙️", "🗓️", "📊", "🧩",
  "🏷️", "🔖", "📚", "🛠️", "🧪", "🌱", "🌿", "🍀", "💬", "🔒",
];

function timeAgo(ts: number): string {
  const s = Math.floor((Date.now() - ts) / 1000);
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return new Date(ts).toLocaleDateString();
}

export function PageHeader({
  pageId,
  saving,
}: {
  pageId: string;
  saving: boolean;
}) {
  const { data: pages } = usePages();
  const page = pages?.find((p) => p.id === pageId);
  const rename = useRenamePage();
  const setIcon = useSetPageIcon();

  const [title, setTitle] = useState(page?.title ?? "");
  const [emojiOpen, setEmojiOpen] = useState(false);
  const lastId = useRef(pageId);

  // sync local title when the page changes (switching pages)
  useEffect(() => {
    if (lastId.current !== pageId) {
      lastId.current = pageId;
      setTitle(page?.title ?? "");
    } else if (page && page.title !== title && document.activeElement?.tagName !== "TEXTAREA") {
      setTitle(page.title);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pageId, page?.title]);

  if (!page) return null;

  const commit = () => {
    const t = title.trim();
    if (t !== page.title) rename.mutate({ id: pageId, title: t || "Untitled" });
  };

  return (
    <div className="mb-1">
      <Popover open={emojiOpen} onOpenChange={setEmojiOpen}>
        <PopoverTrigger
          aria-label="Change icon"
          className="mb-2 grid size-[60px] place-items-center rounded-lg text-[56px] leading-none transition-colors hover:bg-surface-hover"
        >
          {page.icon ?? <Smile className="size-12 text-text-faint" />}
        </PopoverTrigger>
        <PopoverContent className="w-64 p-2">
          <div className="grid grid-cols-8 gap-1">
            {EMOJI.map((e) => (
              <button
                key={e}
                onClick={() => {
                  setIcon.mutate({ id: pageId, icon: e });
                  setEmojiOpen(false);
                }}
                className="grid size-7 place-items-center rounded text-lg hover:bg-surface-hover"
              >
                {e}
              </button>
            ))}
          </div>
          {page.icon && (
            <button
              onClick={() => {
                setIcon.mutate({ id: pageId, icon: null });
                setEmojiOpen(false);
              }}
              className="mt-2 w-full rounded px-2 py-1 text-left text-sm text-text-muted hover:bg-surface-hover"
            >
              Remove icon
            </button>
          )}
        </PopoverContent>
      </Popover>

      <textarea
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            (e.target as HTMLTextAreaElement).blur();
          }
        }}
        rows={1}
        placeholder="Untitled"
        aria-label="Page title"
        className="w-full resize-none bg-transparent text-[40px] font-bold leading-tight tracking-tight outline-none placeholder:text-text-faint"
      />
      <div className="mt-1 text-note text-text-faint">
        {saving ? "Saving…" : `Edited ${timeAgo(page.updated_at)} · Autosaved`}
      </div>
    </div>
  );
}
