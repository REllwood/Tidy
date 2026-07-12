import { useMemo, useState } from "react";
import { Plus } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import {
  databasesApi,
  type Field,
  type FieldType,
  type FieldOptions,
  type RollupFn,
} from "@/lib/api";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

const TYPE_LABELS: Record<FieldType, string> = {
  text: "Text",
  number: "Number",
  select: "Select",
  date: "Date",
  checkbox: "Checkbox",
  dependencies: "Dependencies",
  relation: "Relation",
  lookup: "Lookup",
  rollup: "Rollup",
  formula: "Formula",
};

const ROLLUP_FNS: RollupFn[] = ["count", "sum", "avg", "min", "max"];

export function AddFieldPopover({
  databaseId,
  fields,
  onAdd,
}: {
  databaseId: string;
  fields: Field[];
  onAdd: (v: {
    databaseId: string;
    name: string;
    kind: FieldType;
    options?: FieldOptions;
  }) => void;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [kind, setKind] = useState<FieldType>("text");
  const [targetDatabaseId, setTargetDatabaseId] = useState("");
  const [relationFieldId, setRelationFieldId] = useState("");
  const [targetFieldId, setTargetFieldId] = useState("");
  const [fn, setFn] = useState<RollupFn>("sum");
  const [expr, setExpr] = useState("");

  // Databases (with their fields), needed to author relation/lookup/rollup.
  const needsDbs =
    kind === "relation" || kind === "lookup" || kind === "rollup";
  const { data: databases = [] } = useQuery({
    queryKey: ["databases"],
    queryFn: () => databasesApi.list(),
    enabled: open && needsDbs,
    staleTime: 10_000,
  });

  // Relation fields already on this database (targets for lookup/rollup).
  const relationFields = useMemo(
    () => fields.filter((f) => f.type === "relation"),
    [fields],
  );
  // For the chosen relation field, which database does it point at?
  const lookupTargetFields = useMemo(() => {
    const rel = relationFields.find((f) => f.id === relationFieldId);
    const tdb = rel?.options?.targetDatabaseId;
    return databases.find((d) => d.database_id === tdb)?.fields ?? [];
  }, [relationFields, relationFieldId, databases]);

  const reset = () => {
    setName("");
    setKind("text");
    setTargetDatabaseId("");
    setRelationFieldId("");
    setTargetFieldId("");
    setFn("sum");
    setExpr("");
  };

  const valid = (() => {
    if (kind === "relation") return !!targetDatabaseId;
    if (kind === "lookup") return !!relationFieldId && !!targetFieldId;
    if (kind === "rollup") return !!relationFieldId && !!targetFieldId && !!fn;
    if (kind === "formula") return expr.trim().length > 0;
    return true;
  })();

  const submit = () => {
    if (!valid) return;
    let options: FieldOptions | undefined;
    if (kind === "relation") options = { targetDatabaseId, multi: true };
    else if (kind === "lookup") options = { relationFieldId, targetFieldId };
    else if (kind === "rollup") options = { relationFieldId, targetFieldId, fn };
    else if (kind === "formula") options = { expr: expr.trim() };
    onAdd({
      databaseId,
      name: name.trim() || TYPE_LABELS[kind],
      kind,
      options,
    });
    reset();
    setOpen(false);
  };

  const selectCls =
    "w-full rounded border border-border bg-surface px-2 py-1 text-sm outline-none";

  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) reset();
      }}
    >
      <PopoverTrigger
        aria-label="Add field"
        className="grid size-6 place-items-center rounded text-text-faint hover:bg-surface-hover hover:text-text"
      >
        <Plus className="size-4" />
      </PopoverTrigger>
      <PopoverContent className="w-64 p-2">
        <input
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Field name"
          className="mb-2 w-full rounded border border-border bg-surface px-2 py-1 text-sm outline-none"
        />

        <label className="mb-1 block text-xs font-medium text-text-faint">
          Type
        </label>
        <select
          value={kind}
          onChange={(e) => setKind(e.target.value as FieldType)}
          className={`${selectCls} mb-2 capitalize`}
        >
          {(Object.keys(TYPE_LABELS) as FieldType[]).map((t) => (
            <option key={t} value={t}>
              {TYPE_LABELS[t]}
            </option>
          ))}
        </select>

        {kind === "relation" && (
          <Config label="Related database">
            <select
              value={targetDatabaseId}
              onChange={(e) => setTargetDatabaseId(e.target.value)}
              className={selectCls}
            >
              <option value="">Select a database…</option>
              {databases.map((db) => (
                <option key={db.database_id} value={db.database_id}>
                  {db.title}
                </option>
              ))}
            </select>
          </Config>
        )}

        {(kind === "lookup" || kind === "rollup") && (
          <>
            <Config label="Through relation">
              {relationFields.length === 0 ? (
                <p className="text-xs text-text-faint">
                  Add a relation field first.
                </p>
              ) : (
                <select
                  value={relationFieldId}
                  onChange={(e) => {
                    setRelationFieldId(e.target.value);
                    setTargetFieldId("");
                  }}
                  className={selectCls}
                >
                  <option value="">Select a relation…</option>
                  {relationFields.map((f) => (
                    <option key={f.id} value={f.id}>
                      {f.name}
                    </option>
                  ))}
                </select>
              )}
            </Config>
            {relationFieldId && (
              <Config label={kind === "rollup" ? "Roll up field" : "Look up field"}>
                <select
                  value={targetFieldId}
                  onChange={(e) => setTargetFieldId(e.target.value)}
                  className={selectCls}
                >
                  <option value="">Select a field…</option>
                  {lookupTargetFields.map((f) => (
                    <option key={f.id} value={f.id}>
                      {f.name}
                    </option>
                  ))}
                </select>
              </Config>
            )}
            {kind === "rollup" && (
              <Config label="Aggregate">
                <select
                  value={fn}
                  onChange={(e) => setFn(e.target.value as RollupFn)}
                  className={`${selectCls} capitalize`}
                >
                  {ROLLUP_FNS.map((r) => (
                    <option key={r} value={r}>
                      {r}
                    </option>
                  ))}
                </select>
              </Config>
            )}
          </>
        )}

        {kind === "formula" && (
          <Config label="Expression">
            <input
              value={expr}
              onChange={(e) => setExpr(e.target.value)}
              placeholder="{Budget} * 1.2"
              className={`${selectCls} font-mono`}
            />
            <p className="mt-1 text-2xs text-text-faint">
              Reference fields as <code>{"{Field Name}"}</code>. Functions:
              round, abs, min, max, floor, ceil, sqrt.
            </p>
          </Config>
        )}

        <button
          disabled={!valid}
          onClick={submit}
          className="mt-3 w-full rounded bg-brand px-2 py-1.5 text-sm font-medium text-white hover:bg-brand/90 disabled:opacity-40"
        >
          Create field
        </button>
      </PopoverContent>
    </Popover>
  );
}

function Config({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-2">
      <label className="mb-1 block text-xs font-medium text-text-faint">
        {label}
      </label>
      {children}
    </div>
  );
}
