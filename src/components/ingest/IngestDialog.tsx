import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Sparkles } from "lucide-react";
import { toast } from "sonner";
import { databasesApi, ingestApi } from "@/lib/api";
import { useUi } from "@/store/ui";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";

/**
 * "File a note", paste a blob (meeting notes, an agent dump) and let the
 * ingest pipeline summarize it, file it under a client, and spin action items
 * into Task rows. The same `ingest_note` the MCP server and recorder use.
 */
export function IngestDialog() {
  const open = useUi((s) => s.ingestOpen);
  const setOpen = useUi((s) => s.setIngestOpen);
  const openPage = useUi((s) => s.openPage);
  const qc = useQueryClient();

  const [text, setText] = useState("");
  const [client, setClient] = useState("");
  const [title, setTitle] = useState("");
  const [taskDbId, setTaskDbId] = useState("");
  const [busy, setBusy] = useState(false);

  const { data: databases = [] } = useQuery({
    queryKey: ["databases"],
    queryFn: () => databasesApi.list(),
    enabled: open,
    staleTime: 10_000,
  });

  const reset = () => {
    setText("");
    setClient("");
    setTitle("");
    setTaskDbId("");
  };

  const submit = async () => {
    if (!text.trim() || busy) return;
    setBusy(true);
    try {
      const res = await ingestApi.ingestNote({
        rawText: text.trim(),
        clientHint: client.trim() || undefined,
        title: title.trim() || undefined,
        taskDbId: taskDbId || undefined,
      });
      await qc.invalidateQueries();
      const n = res.task_row_ids.length;
      toast.success(
        `Filed the note${res.client_page_id ? ` under ${client.trim()}` : ""}` +
          (n ? ` · ${n} task${n === 1 ? "" : "s"}` : ""),
      );
      reset();
      setOpen(false);
      openPage(res.page_id);
    } catch (e) {
      toast.error(`Couldn't file the note: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const field =
    "w-full rounded-md border border-border bg-surface px-2.5 py-1.5 text-sm outline-none focus:border-brand";

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) reset();
      }}
    >
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="size-4 text-brand" /> File a note
          </DialogTitle>
          <DialogDescription>
            Paste text: a meeting transcript, notes, anything. It gets
            summarised, filed under the client, and the action items become tasks.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <textarea
            autoFocus
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Paste the note here…"
            rows={8}
            className={`${field} resize-y font-mono`}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === "Enter") submit();
            }}
          />
          <div className="grid grid-cols-2 gap-3">
            <label className="space-y-1">
              <span className="text-xs font-medium text-text-faint">Client (optional)</span>
              <input
                value={client}
                onChange={(e) => setClient(e.target.value)}
                placeholder="e.g. Acme Corp"
                className={field}
              />
            </label>
            <label className="space-y-1">
              <span className="text-xs font-medium text-text-faint">Title (optional)</span>
              <input
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="Auto from summary"
                className={field}
              />
            </label>
          </div>
          <label className="block space-y-1">
            <span className="text-xs font-medium text-text-faint">Send action items to (optional)</span>
            <select
              value={taskDbId}
              onChange={(e) => setTaskDbId(e.target.value)}
              className={field}
            >
              <option value="">Don't create tasks</option>
              {databases.map((db) => (
                <option key={db.database_id} value={db.database_id}>
                  {db.title}
                </option>
              ))}
            </select>
          </label>
        </div>

        <DialogFooter>
          <button
            onClick={() => setOpen(false)}
            className="rounded-md px-3 py-1.5 text-sm text-text-muted hover:bg-surface-hover"
          >
            Cancel
          </button>
          <button
            disabled={!text.trim() || busy}
            onClick={submit}
            className="rounded-md bg-brand px-3 py-1.5 text-sm font-medium text-white hover:bg-brand/90 disabled:opacity-40"
          >
            {busy ? "Filing…" : "File it"}
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
