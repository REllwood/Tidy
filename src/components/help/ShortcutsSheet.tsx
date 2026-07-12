import { Keyboard } from "lucide-react";
import { useUi } from "@/store/ui";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";

const isMac =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
const mod = isMac ? "⌘" : "Ctrl";

const GROUPS: { title: string; items: [string[], string][] }[] = [
  {
    title: "Navigation",
    items: [
      [[mod, "K"], "Search & commands"],
      [[mod, "\\"], "Toggle the sidebar"],
      [["?"], "This shortcuts sheet"],
    ],
  },
  {
    title: "Capture",
    items: [
      [[mod, "Enter"], "File the note (in “File a note”)"],
    ],
  },
  {
    title: "In the editor",
    items: [
      [["/"], "Insert a block (heading, list, todo…)"],
      [["[", "["], "Link to another page"],
      [["#"], "Add a tag"],
    ],
  },
];

export function ShortcutsSheet() {
  const open = useUi((s) => s.shortcutsOpen);
  const setOpen = useUi((s) => s.setShortcutsOpen);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Keyboard className="size-4 text-brand" /> Keyboard shortcuts
          </DialogTitle>
          <DialogDescription>Move fast. Everything's a keystroke away.</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          {GROUPS.map((g) => (
            <div key={g.title}>
              <div className="mb-1.5 text-2xs font-semibold uppercase tracking-wide text-text-faint">
                {g.title}
              </div>
              <div className="space-y-1">
                {g.items.map(([keys, label], i) => (
                  <div key={i} className="flex items-center justify-between py-1">
                    <span className="text-sm text-text-muted">{label}</span>
                    <span className="flex gap-1">
                      {keys.map((k, j) => (
                        <kbd
                          key={j}
                          className="min-w-6 rounded border border-border bg-bg-subtle px-1.5 py-0.5 text-center text-xs font-medium text-text"
                        >
                          {k}
                        </kbd>
                      ))}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
