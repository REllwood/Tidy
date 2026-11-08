import { useMemo, useState } from "react";
import { Plus, X } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import {
  databasesApi,
  type Field,
  type FieldType,
  type FieldOptions,
  type RollupFn,
  type SelectChoice,
} from "@/lib/api";
import { CHIP_PALETTE, chipColours } from "./cells";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

export const TYPE_LABELS: Record<FieldType, string> = {
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

const selectCls =
  "w-full rounded border border-border bg-surface px-2 py-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-brand/40";

const newId = () =>
  typeof crypto !== "undefined" && crypto.randomUUID
    ? crypto.randomUUID()
    : Math.random().toString(36).slice(2);

/** A sensible starter list, used when someone adds a select without choices. */
export function starterChoices(): SelectChoice[] {
  return [
    { id: newId(), name: "To do", color: "grey" },
    { id: newId(), name: "In progress", color: "blue" },
    { id: newId(), name: "Done", color: "green" },
  ];
}

/** True when the options a field type needs have been filled in. */
export function fieldOptionsValid(
  kind: FieldType,
  options: FieldOptions,
): boolean {
  if (kind === "relation") return !!options.targetDatabaseId;
  if (kind === "lookup")
    return !!options.relationFieldId && !!options.targetFieldId;
  if (kind === "rollup")
    return !!options.relationFieldId && !!options.targetFieldId && !!options.fn;
  if (kind === "formula") return (options.expr ?? "").trim().length > 0;
  return true;
}

/** Normalise the options for the field type being saved. */
export function optionsFor(
  kind: FieldType,
  options: FieldOptions,
): FieldOptions | undefined {
  switch (kind) {
    case "select":
      return {
        choices: (options.choices ?? []).filter((c) => c.name.trim())
          .length
          ? options.choices!.map((c) => ({ ...c, name: c.name.trim() }))
          : starterChoices(),
      };
    case "relation":
      return {
        targetDatabaseId: options.targetDatabaseId,
        multi: options.multi !== false,
      };
    case "lookup":
      return {
        relationFieldId: options.relationFieldId,
        targetFieldId: options.targetFieldId,
      };
    case "rollup":
      return {
        relationFieldId: options.relationFieldId,
        targetFieldId: options.targetFieldId,
        fn: options.fn ?? "sum",
      };
    case "formula":
      return { expr: (options.expr ?? "").trim() };
    default:
      return undefined;
  }
}

export function Config({
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

/** Add, rename, recolour and remove the choices on a select field. */
export function ChoicesEditor({
  choices,
  onChange,
}: {
  choices: SelectChoice[];
  onChange: (next: SelectChoice[]) => void;
}) {
  const patch = (id: string, next: Partial<SelectChoice>) =>
    onChange(choices.map((c) => (c.id === id ? { ...c, ...next } : c)));

  return (
    <div className="space-y-1.5">
      {choices.length === 0 && (
        <p className="text-xs text-text-faint">
          No choices yet. Add one, or leave it empty for a starter list.
        </p>
      )}
      {choices.map((c) => (
        <div key={c.id} className="flex items-center gap-1.5">
          <ColourPicker
            colour={c.color}
            onPick={(color) => patch(c.id, { color })}
          />
          <input
            value={c.name}
            aria-label="Choice name"
            placeholder="Choice name"
            onChange={(e) => patch(c.id, { name: e.target.value })}
            className={`${selectCls} min-w-0 flex-1`}
          />
          <button
            type="button"
            aria-label={`Remove ${c.name || "choice"}`}
            onClick={() => onChange(choices.filter((x) => x.id !== c.id))}
            className="grid size-6 shrink-0 place-items-center rounded text-text-faint hover:bg-surface-hover hover:text-danger-c"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() =>
          onChange([
            ...choices,
            {
              id: newId(),
              name: "",
              color: CHIP_PALETTE[choices.length % CHIP_PALETTE.length],
            },
          ])
        }
        className="flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-note text-text-muted hover:bg-surface-hover hover:text-text"
      >
        <Plus className="size-3.5" /> Add choice
      </button>
    </div>
  );
}

function ColourPicker({
  colour,
  onPick,
}: {
  colour: string;
  onPick: (c: string) => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        aria-label="Choose a colour"
        className="size-5 shrink-0 rounded-full border border-border outline-none focus-visible:ring-2 focus-visible:ring-brand/40"
        style={{ background: chipColours(colour).background }}
      />
      <PopoverContent className="w-auto flex-row gap-1 p-1.5">
        {CHIP_PALETTE.map((c) => (
          <button
            key={c}
            type="button"
            aria-label={c}
            title={c}
            onClick={() => {
              onPick(c);
              setOpen(false);
            }}
            className={`size-5 rounded-full border ${
              c === colour ? "border-brand" : "border-border"
            }`}
            style={{ background: chipColours(c).background }}
          />
        ))}
      </PopoverContent>
    </Popover>
  );
}

/** The relation / lookup / rollup / formula inputs, shared by add and edit. */
export function FieldOptionsEditor({
  kind,
  fields,
  options,
  onChange,
  enabled = true,
}: {
  kind: FieldType;
  fields: Field[];
  options: FieldOptions;
  onChange: (next: FieldOptions) => void;
  enabled?: boolean;
}) {
  const needsDbs = kind === "relation" || kind === "lookup" || kind === "rollup";
  const { data: databases = [] } = useQuery({
    queryKey: ["databases"],
    queryFn: () => databasesApi.list(),
    enabled: enabled && needsDbs,
    staleTime: 10_000,
  });

  const relationFields = useMemo(
    () => fields.filter((f) => f.type === "relation"),
    [fields],
  );
  const lookupTargetFields = useMemo(() => {
    const rel = relationFields.find((f) => f.id === options.relationFieldId);
    const tdb = rel?.options?.targetDatabaseId;
    return databases.find((d) => d.database_id === tdb)?.fields ?? [];
  }, [relationFields, options.relationFieldId, databases]);

  if (kind === "select")
    return (
      <Config label="Choices">
        <ChoicesEditor
          choices={options.choices ?? []}
          onChange={(choices) => onChange({ ...options, choices })}
        />
      </Config>
    );

  if (kind === "relation")
    return (
      <Config label="Related database">
        <select
          value={options.targetDatabaseId ?? ""}
          onChange={(e) =>
            onChange({ ...options, targetDatabaseId: e.target.value })
          }
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
    );

  if (kind === "lookup" || kind === "rollup")
    return (
      <>
        <Config label="Through relation">
          {relationFields.length === 0 ? (
            <p className="text-xs text-text-faint">
              Add a relation field first.
            </p>
          ) : (
            <select
              value={options.relationFieldId ?? ""}
              onChange={(e) =>
                onChange({
                  ...options,
                  relationFieldId: e.target.value,
                  targetFieldId: "",
                })
              }
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
        {options.relationFieldId && (
          <Config label={kind === "rollup" ? "Roll up field" : "Look up field"}>
            <select
              value={options.targetFieldId ?? ""}
              onChange={(e) =>
                onChange({ ...options, targetFieldId: e.target.value })
              }
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
              value={options.fn ?? "sum"}
              onChange={(e) =>
                onChange({ ...options, fn: e.target.value as RollupFn })
              }
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
    );

  if (kind === "formula")
    return (
      <Config label="Expression">
        <input
          value={options.expr ?? ""}
          onChange={(e) => onChange({ ...options, expr: e.target.value })}
          placeholder="{Budget} * 1.2"
          className={`${selectCls} font-mono`}
        />
        <p className="mt-1 text-2xs text-text-faint">
          Reference fields as <code>{"{Field Name}"}</code>. Functions: round,
          abs, min, max, floor, ceil, sqrt.
        </p>
      </Config>
    );

  return null;
}

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
  const [options, setOptions] = useState<FieldOptions>({});

  const reset = () => {
    setName("");
    setKind("text");
    setOptions({});
  };

  const valid = fieldOptionsValid(kind, options);

  const submit = () => {
    if (!valid) return;
    onAdd({
      databaseId,
      name: name.trim() || TYPE_LABELS[kind],
      kind,
      options: optionsFor(kind, options),
    });
    reset();
    setOpen(false);
  };

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
      <PopoverContent className="w-72 p-2">
        <input
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && valid) submit();
          }}
          placeholder="Field name"
          className={`${selectCls} mb-2`}
        />

        <Config label="Type">
          <select
            value={kind}
            onChange={(e) => {
              setKind(e.target.value as FieldType);
              setOptions({});
            }}
            className={selectCls}
          >
            {(Object.keys(TYPE_LABELS) as FieldType[]).map((t) => (
              <option key={t} value={t}>
                {TYPE_LABELS[t]}
              </option>
            ))}
          </select>
        </Config>

        <FieldOptionsEditor
          kind={kind}
          fields={fields}
          options={options}
          onChange={setOptions}
          enabled={open}
        />

        <button
          disabled={!valid}
          onClick={submit}
          className="mt-3 w-full rounded bg-brand px-2 py-1.5 text-sm font-medium text-brand-fg hover:bg-brand/90 disabled:opacity-40"
        >
          Create field
        </button>
      </PopoverContent>
    </Popover>
  );
}
