import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  FileText,
  Database,
  Mic,
  Rows3,
  Sparkles,
  Keyboard,
  Loader2,
} from "lucide-react";
import { searchApi, type PageType } from "@/lib/api";
import { usePages } from "@/hooks/usePages";
import { useUi } from "@/store/ui";
import {
  Command,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export function CommandPalette() {
  const open = useUi((s) => s.paletteOpen);
  const setOpen = useUi((s) => s.setPaletteOpen);
  const openPage = useUi((s) => s.openPage);
  const setActivePane = useUi((s) => s.setActivePane);
  const setIngestOpen = useUi((s) => s.setIngestOpen);
  const setShortcutsOpen = useUi((s) => s.setShortcutsOpen);
  const { data: pages } = usePages();

  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");

  useEffect(() => {
    const t = setTimeout(() => setDebounced(query), 150);
    return () => clearTimeout(t);
  }, [query]);

  // reset query each time it opens
  useEffect(() => {
    if (open) {
      setQuery("");
      setDebounced("");
    }
  }, [open]);

  const term = query.trim();
  const debouncedTerm = debounced.trim();

  const { data: results, isFetching } = useQuery({
    queryKey: ["search", debouncedTerm],
    queryFn: () => searchApi.query(debouncedTerm),
    enabled: open && debouncedTerm.length > 0,
  });

  const recent = (pages ?? [])
    .slice()
    .sort((a, b) => b.updated_at - a.updated_at)
    .slice(0, 5);

  const go = (fn: () => void) => {
    fn();
    setOpen(false);
  };

  // Branch on what's typed right now, not on the debounced value, so the
  // actions list doesn't linger over the first keystroke.
  const searching = term.length > 0;
  // While the next search is in flight the previous hits stay on screen, so the
  // list doesn't flicker and the highlighted row doesn't jump.
  const shown = searching ? results : undefined;
  const pending = searching && (term !== debouncedTerm || isFetching);
  const hits = (shown?.pages.length ?? 0) + (shown?.rows.length ?? 0);

  // The search box takes focus as soon as the palette opens, so ⌘K then typing
  // just works without a click.
  const inputRef = useRef<HTMLInputElement>(null);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent
        showCloseButton={false}
        className="overflow-hidden p-0 sm:max-w-xl"
        initialFocus={inputRef}
      >
        <DialogHeader className="sr-only">
          <DialogTitle>Search</DialogTitle>
        </DialogHeader>
        <Command shouldFilter={false} className="bg-transparent">
          <CommandInput
            ref={inputRef}
            value={query}
            onValueChange={setQuery}
            placeholder="Search pages and database rows…"
          />
          <CommandList>
            {!searching && (
              <>
                <CommandGroup heading="Actions">
                  <CommandItem
                    value="file-note ingest"
                    onSelect={() => go(() => setIngestOpen(true))}
                  >
                    <Sparkles className="size-4" /> File a note
                  </CommandItem>
                  <CommandItem
                    value="new-meeting"
                    onSelect={() => go(() => setActivePane({ kind: "meeting" }))}
                  >
                    <Mic className="size-4" /> New meeting
                  </CommandItem>
                  <CommandItem
                    value="keyboard-shortcuts help"
                    onSelect={() => go(() => setShortcutsOpen(true))}
                  >
                    <Keyboard className="size-4" /> Keyboard shortcuts
                  </CommandItem>
                </CommandGroup>
                {recent.length > 0 && (
                  <CommandGroup heading="Recent">
                    {recent.map((p) => (
                      <CommandItem
                        key={p.id}
                        value={p.id}
                        onSelect={() => go(() => openPage(p.id))}
                      >
                        {p.icon ?? <PageIcon type={p.type} />}
                        <span className="truncate">{p.title || "Untitled"}</span>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                )}
              </>
            )}

            {searching && pending && hits === 0 && (
              <div
                role="status"
                className="flex items-center gap-2 px-3 py-6 text-sm text-text-muted"
              >
                <Loader2 className="size-3.5 animate-spin" />
                Searching…
              </div>
            )}

            {searching && !pending && hits === 0 && (
              <div className="px-3 py-6 text-center text-sm text-text-muted">
                No matches for “{query}”
              </div>
            )}

            {shown && shown.pages.length > 0 && (
              <CommandGroup heading="Pages">
                {shown.pages.map((p) => (
                  <CommandItem
                    key={p.id}
                    value={`page-${p.id}`}
                    onSelect={() => go(() => openPage(p.id))}
                  >
                    {p.icon ?? <PageIcon type={p.type} />}
                    <span className="truncate">{p.title || "Untitled"}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            )}

            {shown && shown.rows.length > 0 && (
              <CommandGroup heading="Database rows">
                {shown.rows.map((r) => (
                  <CommandItem
                    key={r.row_id}
                    value={`row-${r.row_id}`}
                    // Row hits open the database page that holds them.
                    onSelect={() => go(() => openPage(r.page_id))}
                  >
                    <Rows3 className="size-4 text-text-faint" />
                    <span className="truncate">{r.text}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  );
}

function PageIcon({ type }: { type: PageType }) {
  return type === "database" ? (
    <Database className="size-4 text-text-faint" />
  ) : (
    <FileText className="size-4 text-text-faint" />
  );
}
