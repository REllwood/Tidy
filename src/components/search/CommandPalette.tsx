import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { FileText, Database, Mic, Rows3, Sparkles, Keyboard } from "lucide-react";
import { searchApi, type PageType } from "@/lib/api";
import { usePages } from "@/hooks/usePages";
import { useUi } from "@/store/ui";
import {
  Command,
  CommandEmpty,
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

  const { data: results, isFetching } = useQuery({
    queryKey: ["search", debounced],
    queryFn: () => searchApi.query(debounced),
    enabled: open && debounced.trim().length > 0,
  });

  const recent = (pages ?? [])
    .slice()
    .sort((a, b) => b.updated_at - a.updated_at)
    .slice(0, 5);

  const go = (fn: () => void) => {
    fn();
    setOpen(false);
  };

  const hasQuery = debounced.trim().length > 0;

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent
        showCloseButton={false}
        className="overflow-hidden p-0 sm:max-w-xl"
      >
        <DialogHeader className="sr-only">
          <DialogTitle>Search</DialogTitle>
        </DialogHeader>
        <Command shouldFilter={false} className="bg-transparent">
          <CommandInput
            value={query}
            onValueChange={setQuery}
            placeholder="Search pages and database rows…"
          />
          <CommandList>
            <CommandEmpty>
              {isFetching
                ? "Searching…"
                : hasQuery
                  ? `No matches for "${debounced}"`
                  : "Type to search."}
            </CommandEmpty>

            {!hasQuery && (
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

            {hasQuery && results && results.pages.length > 0 && (
              <CommandGroup heading="Pages">
                {results.pages.map((p) => (
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

            {hasQuery && results && results.rows.length > 0 && (
              <CommandGroup heading="Database rows">
                {results.rows.map((r) => (
                  <CommandItem
                    key={r.row_id}
                    value={`row-${r.row_id}`}
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
