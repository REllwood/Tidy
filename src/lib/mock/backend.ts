/**
 * In-memory mock of the Rust command surface, used only in plain-web previews
 * (outside Tauri). Mirrors the real command names, args, and return shapes so
 * the UI behaves identically. Persisted to localStorage so reloads keep state.
 */
import type {
  Page,
  Field,
  FieldType,
  RowWithCells,
  DbView,
  DatabaseBundle,
  ModelInfo,
} from "@/lib/api";

// ---- mock meeting state (browser preview only) ----
let mockRecording = false;

const MODEL_DEFS: { id: string; name: string; size: number }[] = [
  { id: "tiny", name: "Whisper Tiny", size: 77_700_000 },
  { id: "base", name: "Whisper Base", size: 147_900_000 },
  { id: "small", name: "Whisper Small", size: 487_600_000 },
  { id: "medium", name: "Whisper Medium", size: 1_530_000_000 },
];
const MODEL_KEY = "appflower-mock-models";
type ModelState = Record<string, { downloaded: boolean; selected: boolean }>;

function mockModelState(): ModelState {
  try {
    const raw = localStorage.getItem(MODEL_KEY);
    if (raw) return JSON.parse(raw) as ModelState;
  } catch {
    /* default */
  }
  // base downloaded + selected by default so the recorder is demoable
  const def: ModelState = { base: { downloaded: true, selected: true } };
  localStorage.setItem(MODEL_KEY, JSON.stringify(def));
  return def;
}
function saveModelState(s: ModelState) {
  localStorage.setItem(MODEL_KEY, JSON.stringify(s));
}
function mockModels(): ModelInfo[] {
  const st = mockModelState();
  return MODEL_DEFS.map((d) => ({
    id: d.id,
    name: d.name,
    size: d.size,
    downloaded: st[d.id]?.downloaded ?? false,
    selected: st[d.id]?.selected ?? false,
  }));
}

interface MockDatabase {
  page_id: string;
  fields: Field[];
  rows: RowWithCells[];
  views: DbView[];
}
interface MockLink {
  source_page_id: string;
  target_page_id: string | null;
  dst_title: string | null;
  context: string | null;
  kind: string;
}
interface MockDb {
  pages: Page[];
  documents: Record<string, string>;
  databases: Record<string, MockDatabase>; // keyed by database_id
  links: MockLink[];
  pageTags: Record<string, string[]>;
  settings: Record<string, string>;
}

const KEY = "appflower-mock-db";
const uid = () =>
  (crypto.randomUUID?.() ?? Math.random().toString(36).slice(2));
const now = () => Date.now();

function mkField(
  database_id: string,
  name: string,
  type: FieldType,
  position: number,
  options: Field["options"] = null,
): Field {
  return { id: uid(), database_id, name, type, options, position };
}

/**
 * A blank database: the exact starter schema + views a freshly created database
 * gets in the real app (Rust `create_for_page`). No rows. Keep this in lockstep
 * with the Rust seeder so browser verification reflects the shipped behavior.
 */
function blankDatabase(page_id: string): MockDatabase & { database_id: string } {
  const dbId = uid();
  const toDo = uid(), inProg = uid(), done = uid();
  const name = mkField(dbId, "Name", "text", 1);
  const status = mkField(dbId, "Status", "select", 2, {
    choices: [
      { id: toDo, name: "To do", color: "grey" },
      { id: inProg, name: "In progress", color: "blue" },
      { id: done, name: "Done", color: "green" },
    ],
  });
  const date = mkField(dbId, "Start", "date", 3);
  const due = mkField(dbId, "Due", "date", 4);
  const deps = mkField(dbId, "Depends on", "dependencies", 5);
  const fields = [name, status, date, due, deps];
  const views: DbView[] = [
    { id: uid(), database_id: dbId, kind: "grid", config: null, position: 1 },
    {
      id: uid(),
      database_id: dbId,
      kind: "board",
      config: { groupByFieldId: status.id },
      position: 2,
    },
    {
      id: uid(),
      database_id: dbId,
      kind: "calendar",
      config: { dateFieldId: date.id },
      position: 3,
    },
    {
      id: uid(),
      database_id: dbId,
      kind: "gantt",
      config: {
        startFieldId: date.id,
        endFieldId: due.id,
        dependenciesFieldId: deps.id,
      },
      position: 4,
    },
  ];
  return { page_id, fields, rows: [], views, database_id: dbId };
}

/** The demo Roadmap: a blank database pre-populated with an example task set. */
function seedDatabase(page_id: string): MockDatabase & { database_id: string } {
  const db = blankDatabase(page_id);
  const dbId = db.database_id;
  const [name, status, date, due, deps] = db.fields;
  const choices = status.options!.choices!;
  const toDo = choices[0].id, inProg = choices[1].id, done = choices[2].id;
  const t = now();
  const ids = [uid(), uid(), uid(), uid(), uid()];
  const mkRow = (
    id: string,
    title: string,
    statusId: string,
    start: string,
    dueDay: string,
    pos: number,
    dependsOn: string[] = [],
  ): RowWithCells => ({
    id,
    database_id: dbId,
    position: pos,
    created_at: t,
    cells: {
      [name.id]: title,
      [status.id]: statusId,
      [date.id]: start,
      [due.id]: dueDay,
      [deps.id]: dependsOn,
    },
  });
  db.rows = [
    mkRow(ids[0], "Slash menu grouping", done, "2026-06-10", "2026-06-14", 1),
    mkRow(ids[1], "Whisper model manager UI", inProg, "2026-06-16", "2026-06-24", 2, [ids[0]]),
    mkRow(ids[2], "Kanban drag persistence", inProg, "2026-06-22", "2026-06-28", 3, [ids[1]]),
    mkRow(ids[3], "Markdown export polish", toDo, "2026-07-01", "2026-07-05", 4),
    mkRow(ids[4], "Speaker diarization research", toDo, "2026-08-04", "2026-08-12", 5, [ids[2]]),
  ];
  return db;
}

function seed(): MockDb {
  const t = now();
  const mkPage = (
    title: string,
    parent_id: string | null,
    type: "doc" | "database",
    position: number,
    icon: string | null,
  ): Page => ({
    id: uid(),
    parent_id,
    title,
    icon,
    type,
    position,
    is_favorite: false,
    created_at: t,
    updated_at: t,
  });

  const projects = mkPage("Projects", null, "doc", 1, "📁");
  const kickoff = mkPage("Q3 Kickoff Notes", projects.id, "doc", 1, "📝");
  const roadmap = mkPage("Roadmap", projects.id, "database", 2, "🗂️");
  const kb = mkPage("Knowledge Base", null, "doc", 2, "🧠");
  const meetings = mkPage("Meeting Notes", null, "doc", 3, "🎙️");
  const pages = [projects, kickoff, roadmap, kb, meetings];

  const documents: Record<string, string> = {
    [kickoff.id]: JSON.stringify([
      { type: "heading", props: { level: 1 }, content: "Q3 Kickoff" },
      { type: "paragraph", content: "Welcome to Tidy. Try the slash menu to add headings, lists and to-dos." },
    ]),
  };

  const seeded = seedDatabase(roadmap.id) as MockDatabase & { database_id: string };
  const roadmapDbId = seeded.database_id;

  // A second "Clients" database, related to the roadmap tasks.
  const clients = mkPage("Clients", null, "database", 4, "🏢");
  pages.push(clients);
  const clientsDbId = uid();
  const cName = mkField(clientsDbId, "Name", "text", 1);
  const cIndustry = mkField(clientsDbId, "Industry", "text", 2);
  const cRetainer = mkField(clientsDbId, "Retainer", "number", 3);
  const acme = uid();
  const globex = uid();
  const clientsDb: MockDatabase = {
    page_id: clients.id,
    fields: [cName, cIndustry, cRetainer],
    views: [
      { id: uid(), database_id: clientsDbId, kind: "grid", config: null, position: 1 },
    ],
    rows: [
      { id: acme, database_id: clientsDbId, position: 1, created_at: t, cells: { [cName.id]: "Acme Corp", [cIndustry.id]: "Manufacturing", [cRetainer.id]: 2000 } },
      { id: globex, database_id: clientsDbId, position: 2, created_at: t, cells: { [cName.id]: "Globex", [cIndustry.id]: "Finance", [cRetainer.id]: 3500 } },
    ],
  };

  // Add relational + computed fields to the roadmap tasks.
  const clientField = mkField(roadmapDbId, "Client", "relation", 6, {
    targetDatabaseId: clientsDbId,
    multi: false,
  });
  const clientIndustry = mkField(roadmapDbId, "Client industry", "lookup", 7, {
    relationFieldId: clientField.id,
    targetFieldId: cIndustry.id,
  });
  const budget = mkField(roadmapDbId, "Budget", "number", 8);
  const budgetVat = mkField(roadmapDbId, "Budget +VAT", "formula", 9, {
    expr: "{Budget} * 1.2",
  });
  const retainerRollup = mkField(roadmapDbId, "Client retainer", "rollup", 10, {
    relationFieldId: clientField.id,
    targetFieldId: cRetainer.id,
    fn: "sum",
  });
  seeded.fields.push(clientField, clientIndustry, budget, budgetVat, retainerRollup);
  seeded.rows[1].cells[clientField.id] = [acme];
  seeded.rows[1].cells[budget.id] = 5000;
  seeded.rows[2].cells[clientField.id] = [globex];
  seeded.rows[2].cells[budget.id] = 3000;

  // A few tasks dated relative to *now* so the Today dashboard always has a live
  // agenda (overdue / today / this week) regardless of the real date.
  const [rName, rStatus, rStart, rDue] = seeded.fields;
  const choices = rStatus.options!.choices!;
  const toDoId = choices[0].id, inProgId = choices[1].id;
  const relDate = (days: number) => {
    const d = new Date();
    d.setDate(d.getDate() + days);
    return d.toISOString().slice(0, 10);
  };
  const planTask = (title: string, statusId: string, offset: number, pos: number) => {
    seeded.rows.push({
      id: uid(),
      database_id: roadmapDbId,
      position: pos,
      created_at: t,
      cells: {
        [rName.id]: title,
        [rStatus.id]: statusId,
        [rStart.id]: relDate(offset - 1),
        [rDue.id]: relDate(offset),
      },
    });
  };
  planTask("Send Acme the revised SOW", inProgId, -1, 6); // overdue
  planTask("Review Globex security questionnaire", toDoId, 0, 7); // today
  planTask("Prep Q3 board deck", toDoId, 0, 8); // today
  planTask("1:1 with design lead", toDoId, 2, 9); // this week
  planTask("Publish the changelog", toDoId, 4, 10); // this week

  const databases: Record<string, MockDatabase> = {
    [roadmapDbId]: seeded,
    [clientsDbId]: clientsDb,
  };

  // A few seeded wiki-links so the graph view has a connected shape to show.
  const mention = (src: string, tgt: string, title: string): MockLink => ({
    source_page_id: src,
    target_page_id: tgt,
    dst_title: title,
    context: null,
    kind: "mention",
  });
  const links: MockLink[] = [
    mention(kb.id, kickoff.id, "Q3 Kickoff Notes"),
    mention(kb.id, roadmap.id, "Roadmap"),
    mention(meetings.id, kb.id, "Knowledge Base"),
    mention(kickoff.id, roadmap.id, "Roadmap"),
    mention(kickoff.id, clients.id, "Clients"),
  ];

  return { pages, documents, databases, links, pageTags: {}, settings: {} };
}

function load(): MockDb {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return JSON.parse(raw) as MockDb;
  } catch {
    /* reseed */
  }
  const fresh = seed();
  save(fresh);
  return fresh;
}
function save(db: MockDb) {
  try {
    localStorage.setItem(KEY, JSON.stringify(db));
  } catch {
    /* ignore */
  }
}

let db: MockDb | null = null;
function store(): MockDb {
  if (!db) db = load();
  // backfill v3 fields for DBs seeded before this version
  if (!db.links) db.links = [];
  if (!db.pageTags) db.pageTags = {};
  if (!db.settings) db.settings = {};
  return db;
}
function commit() {
  if (db) save(db);
}

function nextPos(parent_id: string | null): number {
  const sibs = store().pages.filter((p) => p.parent_id === parent_id);
  return (sibs.reduce((m, p) => Math.max(m, p.position), 0) || 0) + 1;
}
function descendants(id: string): Set<string> {
  const out = new Set<string>([id]);
  let added = true;
  while (added) {
    added = false;
    for (const p of store().pages) {
      if (p.parent_id && out.has(p.parent_id) && !out.has(p.id)) {
        out.add(p.id);
        added = true;
      }
    }
  }
  return out;
}
function dbByPage(pageId: string): (MockDatabase & { database_id: string }) | undefined {
  const d = store();
  for (const [id, mdb] of Object.entries(d.databases)) {
    if (mdb.page_id === pageId) return { ...mdb, database_id: id };
  }
  return undefined;
}
function dbById(databaseId: string): MockDatabase | undefined {
  return store().databases[databaseId];
}
function bundleOf(databaseId: string, mdb: MockDatabase): DatabaseBundle {
  // Deep-clone so callers (React Query cache) never share mutable references
  // with the store, mirrors the real backend, where each invoke deserializes
  // fresh objects, so invalidate→refetch actually yields changed references.
  return structuredClone({
    database_id: databaseId,
    fields: mdb.fields.slice().sort((a, b) => a.position - b.position),
    rows: mdb.rows.slice().sort((a, b) => a.position - b.position),
    views: mdb.views.slice().sort((a, b) => a.position - b.position),
  });
}
function rowById(id: string): RowWithCells | undefined {
  for (const mdb of Object.values(store().databases)) {
    const r = mdb.rows.find((x) => x.id === id);
    if (r) return r;
  }
  return undefined;
}
function maxPos(nums: number[]): number {
  return (nums.length ? Math.max(...nums) : 0) + 1;
}
/** Valid field types, mirrors Rust FIELD_TYPES; create_field rejects others. */
const FIELD_TYPES = new Set<string>([
  "text",
  "number",
  "select",
  "date",
  "checkbox",
  "dependencies",
  "relation",
  "lookup",
  "rollup",
  "formula",
]);
/** Match SQLite's default BINARY `ORDER BY title` (UTF-16 code-unit order),
 *  not locale order, so the mock and Rust agree on row/node order. */
function byTitle<T extends { title: string }>(a: T, b: T): number {
  return a.title < b.title ? -1 : a.title > b.title ? 1 : 0;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Args = Record<string, any>;

export async function mockInvoke<T>(cmd: string, args: Args): Promise<T> {
  const d = store();
  const find = (id: string) => d.pages.find((p) => p.id === id);

  switch (cmd) {
    case "list_pages":
      return d.pages.slice() as T;
    case "get_page": {
      const p = find(args.id);
      if (!p) throw err("not_found", `page ${args.id}`);
      return p as T;
    }
    case "create_page": {
      const kind = (args.kind ?? "doc") as "doc" | "database";
      if (kind !== "doc" && kind !== "database")
        throw err("invalid", `page type ${kind}`);
      const p: Page = {
        id: uid(),
        parent_id: args.parentId ?? null,
        title: args.title ?? "Untitled",
        icon: null,
        type: kind,
        position: nextPos(args.parentId ?? null),
        is_favorite: false,
        created_at: now(),
        updated_at: now(),
      };
      d.pages.push(p);
      if (kind === "doc") d.documents[p.id] = "[]";
      else {
        // New databases get the blank starter schema (no example rows), exactly
        // as the real Rust `create_for_page` does.
        const seeded = blankDatabase(p.id);
        d.databases[seeded.database_id] = seeded;
      }
      // resolve any dangling links pointing at this new title
      for (const l of d.links)
        if (!l.target_page_id && l.dst_title === p.title) l.target_page_id = p.id;
      commit();
      return p as T;
    }
    case "rename_page": {
      const p = find(args.id);
      if (!p) throw err("not_found", args.id);
      p.title = args.title;
      p.updated_at = now();
      for (const l of d.links)
        if (!l.target_page_id && l.dst_title === p.title) l.target_page_id = p.id;
      commit();
      return p as T;
    }

    // ---- knowledge (v3) ----
    case "set_page_links": {
      d.links = d.links.filter(
        (l) => !(l.source_page_id === args.pageId && l.kind === "mention"),
      );
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      for (const l of (args.links ?? []) as any[]) {
        const target =
          l.target_page_id ||
          d.pages.find((p) => p.title === l.dst_title)?.id ||
          null;
        d.links.push({
          source_page_id: args.pageId,
          target_page_id: target,
          dst_title: l.dst_title ?? null,
          context: l.context ?? null,
          kind: "mention",
        });
      }
      // Mirror Rust: strip ALL leading '#', drop empties, dedupe.
      d.pageTags[args.pageId] = [
        ...new Set(
          ((args.tags ?? []) as string[])
            .map((t) => t.replace(/^#+/, "").trim())
            .filter(Boolean),
        ),
      ];
      commit();
      return undefined as T;
    }
    case "get_backlinks": {
      const out = d.links
        .filter((l) => l.target_page_id === args.pageId && l.kind === "mention")
        .map((l) => ({ l, src: find(l.source_page_id) }))
        .filter(({ src }) => !!src) // mirror Rust JOIN + deleted_at IS NULL
        .sort((a, b) => (b.src!.updated_at ?? 0) - (a.src!.updated_at ?? 0))
        .map(({ l, src }) => ({
          source_page_id: l.source_page_id,
          source_title: src!.title || "Untitled",
          source_icon: src!.icon ?? null,
          context: l.context,
        }));
      return out as T;
    }
    case "get_page_tags":
      // Mirror Rust: name-sorted.
      return (d.pageTags[args.pageId] ?? []).slice().sort() as T;
    case "get_graph": {
      const alive = new Set(d.pages.map((p) => p.id));
      const seen = new Set<string>();
      const edges: { source: string; target: string }[] = [];
      const degree: Record<string, number> = {};
      for (const l of d.links) {
        if (l.kind !== "mention" || !l.target_page_id) continue;
        if (!alive.has(l.source_page_id) || !alive.has(l.target_page_id)) continue;
        if (l.source_page_id === l.target_page_id) continue;
        const key = `${l.source_page_id}->${l.target_page_id}`;
        if (seen.has(key)) continue;
        seen.add(key);
        edges.push({ source: l.source_page_id, target: l.target_page_id });
        degree[l.source_page_id] = (degree[l.source_page_id] ?? 0) + 1;
        degree[l.target_page_id] = (degree[l.target_page_id] ?? 0) + 1;
      }
      const nodes = d.pages
        .slice()
        .sort(byTitle)
        .map((p) => ({
          id: p.id,
          title: p.title,
          icon: p.icon,
          type: p.type,
          degree: degree[p.id] ?? 0,
        }));
      return { nodes, edges } as T;
    }
    case "ingest_note": {
      // Mirror core::ingest_note: body from bodyJson (recorder) or a naive
      // fake-summary of rawText (paste/MCP), then file under client + tasks.
      let body: string;
      let actionItems: string[];
      let summaryText: string;
      if (args.bodyJson) {
        body = args.bodyJson;
        actionItems = (args.actionItems ?? []) as string[];
        summaryText = "";
      } else {
        const raw = String(args.rawText ?? "").trim();
        summaryText = raw ? raw.split("\n")[0].slice(0, 200) : "Meeting note";
        body = JSON.stringify([
          { type: "heading", props: { level: 2 }, content: "Summary" },
          { type: "paragraph", content: summaryText || "(no content)" },
        ]);
        actionItems = raw
          .split("\n")
          .map((l) => l.trim())
          .filter((l) => /^[-*]\s|^todo\b|^action\b/i.test(l))
          .map((l) => l.replace(/^[-*]\s*/, ""))
          .slice(0, 20);
      }
      // Client page: reuse a live doc by title, else create one.
      let clientId: string | null = null;
      const hint = String(args.clientHint ?? "").trim();
      if (hint) {
        const existing = d.pages.find((p) => p.title === hint && p.type === "doc");
        if (existing) {
          clientId = existing.id;
        } else {
          const cp: Page = {
            id: uid(),
            parent_id: null,
            title: hint,
            icon: null,
            type: "doc",
            position: nextPos(null),
            is_favorite: false,
            created_at: now(),
            updated_at: now(),
          };
          d.pages.push(cp);
          d.documents[cp.id] = "[]";
          clientId = cp.id;
        }
      }
      const title =
        String(args.title ?? "").trim() ||
        summaryText.split("\n")[0].slice(0, 80) ||
        "Meeting note";
      const note: Page = {
        id: uid(),
        parent_id: clientId,
        title,
        icon: null,
        type: "record",
        position: nextPos(clientId),
        is_favorite: false,
        created_at: now(),
        updated_at: now(),
      };
      d.pages.push(note);
      d.documents[note.id] = body;
      if (clientId)
        d.links.push({ source_page_id: note.id, target_page_id: clientId, dst_title: null, context: null, kind: "task_of" });
      if (args.meetingId)
        d.links.push({ source_page_id: note.id, target_page_id: args.meetingId, dst_title: null, context: null, kind: "meeting_ref" });
      const taskRowIds: string[] = [];
      const taskDb = args.taskDbId ? dbById(args.taskDbId) : undefined;
      if (taskDb) {
        const nameField = taskDb.fields.find((f) => f.type === "text");
        const statusField = taskDb.fields.find((f) => f.type === "select");
        const todo = statusField?.options?.choices?.find(
          (c) => c.name.toLowerCase() === "to do",
        );
        for (const item of actionItems) {
          const row: RowWithCells = {
            id: uid(),
            database_id: args.taskDbId,
            position: maxPos(taskDb.rows.map((r) => r.position)),
            created_at: now(),
            cells: {},
          };
          if (nameField) row.cells[nameField.id] = item;
          if (statusField && todo) row.cells[statusField.id] = todo.id;
          taskDb.rows.push(row);
          taskRowIds.push(row.id);
        }
      }
      commit();
      return {
        page_id: note.id,
        client_page_id: clientId,
        task_row_ids: taskRowIds,
        summary: summaryText,
      } as T;
    }
    // ---- vault (native; mock keeps a dir setting + counts) ----
    case "get_vault_dir":
      return (d.settings.vault_dir ?? null) as T;
    case "set_vault_dir": {
      d.settings.vault_dir = args.path;
      commit();
      return d.pages.filter((p) => ["doc", "record", "meeting"].includes(p.type)).length as T;
    }
    case "export_vault":
      return d.pages.filter((p) => ["doc", "record", "meeting"].includes(p.type)).length as T;
    case "flush_page":
      return undefined as T;

    // ---- mcp connection ----
    case "mcp_get_token":
      return (d.settings.mcp_token ?? null) as T;
    case "mcp_enable": {
      let token = d.settings.mcp_token;
      if (!token) {
        token = uid();
        d.settings.mcp_token = token;
        commit();
      }
      const sidecar = "/Applications/AppFlower.app/Contents/MacOS/appflower-mcp";
      return {
        token,
        sidecar_path: sidecar,
        claude_command: `claude mcp add --scope user --transport stdio --env APPFLOWER_MCP_TOKEN=${token} appflower -- "${sidecar}"`,
      } as T;
    }
    case "mcp_disable": {
      delete d.settings.mcp_token;
      commit();
      return undefined as T;
    }

    case "set_page_icon": {
      const p = find(args.id);
      if (!p) throw err("not_found", args.id);
      p.icon = args.icon ?? null;
      p.updated_at = now();
      commit();
      return p as T;
    }
    case "set_page_favorite": {
      const p = find(args.id);
      if (!p) throw err("not_found", args.id);
      p.is_favorite = !!args.isFavorite;
      p.updated_at = now();
      commit();
      return p as T;
    }
    case "move_page": {
      const p = find(args.id);
      if (!p) throw err("not_found", args.id);
      if (args.parentId === p.id) throw err("invalid", "self-parent");
      p.parent_id = args.parentId ?? null;
      p.position = args.position;
      p.updated_at = now();
      commit();
      return p as T;
    }
    case "delete_page": {
      if (!find(args.id)) throw err("not_found", args.id); // Rust errors on missing id
      const kill = descendants(args.id);
      d.pages = d.pages.filter((p) => !kill.has(p.id));
      for (const id of kill) {
        delete d.documents[id];
        delete d.pageTags[id];
      }
      for (const [dbId, mdb] of Object.entries(d.databases))
        if (kill.has(mdb.page_id)) delete d.databases[dbId];
      // drop outgoing links; unresolve incoming (FK: cascade source, set-null target)
      d.links = d.links.filter((l) => !kill.has(l.source_page_id));
      for (const l of d.links) if (l.target_page_id && kill.has(l.target_page_id)) l.target_page_id = null;
      commit();
      return undefined as T;
    }
    case "get_document":
      return (d.documents[args.id] ?? "[]") as T;
    case "update_document": {
      d.documents[args.id] = args.content;
      const p = find(args.id);
      if (p) p.updated_at = now();
      commit();
      return undefined as T;
    }

    // ---- databases ----
    case "get_database": {
      const mdb = dbByPage(args.pageId);
      if (!mdb) throw err("not_found", `database for page ${args.pageId}`);
      return bundleOf(mdb.database_id, mdb) as T;
    }
    case "get_database_by_id": {
      const mdb = dbById(args.databaseId);
      // Rust bundle_by_id returns an empty bundle for an unknown id (no error),
      // so a relation pointing at a deleted DB resolves to nothing, not a crash.
      if (!mdb)
        return {
          database_id: args.databaseId,
          fields: [],
          rows: [],
          views: [],
        } as T;
      return bundleOf(args.databaseId, mdb) as T;
    }
    case "list_databases": {
      const out = Object.entries(d.databases).map(([database_id, mdb]) => {
        const page = find(mdb.page_id);
        return {
          database_id,
          page_id: mdb.page_id,
          title: page?.title ?? "Untitled",
          icon: page?.icon ?? null,
          fields: mdb.fields.slice().sort((a, b) => a.position - b.position),
        };
      });
      out.sort(byTitle);
      return out as T;
    }
    case "promote_row": {
      const r = rowById(args.rowId);
      if (!r) throw err("not_found", args.rowId);
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const existing = (r as any).page_id as string | undefined;
      if (existing) {
        const p = find(existing);
        if (p) return p as T;
      }
      const page: Page = {
        id: uid(),
        parent_id: null,
        title: args.title ?? "", // verbatim, as Rust create() stores it
        icon: null,
        type: "record",
        position: nextPos(null),
        is_favorite: false,
        created_at: now(),
        updated_at: now(),
      };
      d.pages.push(page);
      d.documents[page.id] = "[]";
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (r as any).page_id = page.id;
      commit();
      return page as T;
    }
    case "create_field": {
      const mdb = dbById(args.databaseId);
      if (!mdb) throw err("not_found", args.databaseId);
      if (!FIELD_TYPES.has(args.kind)) throw err("invalid", `field type '${args.kind}'`);
      const f = mkField(
        args.databaseId,
        args.name,
        args.kind,
        maxPos(mdb.fields.map((x) => x.position)),
        args.options ?? null,
      );
      mdb.fields.push(f);
      commit();
      return f as T;
    }
    case "update_field": {
      for (const mdb of Object.values(d.databases)) {
        const f = mdb.fields.find((x) => x.id === args.id);
        if (f) {
          if (args.name !== undefined && args.name !== null) f.name = args.name;
          if (args.options !== undefined && args.options !== null)
            f.options = args.options;
          commit();
          return f as T;
        }
      }
      throw err("not_found", args.id);
    }
    case "delete_field": {
      for (const mdb of Object.values(d.databases)) {
        const i = mdb.fields.findIndex((x) => x.id === args.id);
        if (i >= 0) {
          mdb.fields.splice(i, 1);
          for (const r of mdb.rows) delete r.cells[args.id];
          commit();
          return undefined as T;
        }
      }
      return undefined as T;
    }
    case "create_row": {
      const mdb = dbById(args.databaseId);
      if (!mdb) throw err("not_found", args.databaseId);
      const r: RowWithCells = {
        id: uid(),
        database_id: args.databaseId,
        position: maxPos(mdb.rows.map((x) => x.position)),
        created_at: now(),
        cells: {},
      };
      mdb.rows.push(r);
      commit();
      return r as T;
    }
    case "delete_row": {
      for (const mdb of Object.values(d.databases)) {
        const i = mdb.rows.findIndex((x) => x.id === args.id);
        if (i >= 0) {
          mdb.rows.splice(i, 1);
          commit();
          return undefined as T;
        }
      }
      return undefined as T;
    }
    case "move_row": {
      const r = rowById(args.id);
      if (r) {
        r.position = args.position;
        commit();
      }
      return undefined as T;
    }
    case "set_cell": {
      const r = rowById(args.rowId);
      if (!r) throw err("not_found", args.rowId);
      if (args.value === null || args.value === undefined)
        delete r.cells[args.fieldId];
      else r.cells[args.fieldId] = args.value;
      commit();
      return undefined as T;
    }
    case "update_view": {
      for (const mdb of Object.values(d.databases)) {
        const v = mdb.views.find((x) => x.id === args.id);
        if (v) {
          v.config = args.config;
          commit();
          return v as T;
        }
      }
      throw err("not_found", args.id);
    }

    // ---- meeting (mock) ----
    case "is_recording":
      return mockRecording as T;
    case "start_recording":
      mockRecording = true;
      return undefined as T;
    case "stop_recording":
      mockRecording = false;
      return { audio_path: "mock://recording.wav", duration_ms: 142_000 } as T;
    case "record_meeting":
      return undefined as T; // mock: no-op (meeting metadata persistence)
    case "list_models":
      return mockModels() as T;
    case "download_model": {
      const m = mockModelState();
      m[args.id] = { downloaded: true, selected: m[args.id]?.selected ?? false };
      saveModelState(m);
      return undefined as T;
    }
    case "select_model": {
      const m = mockModelState();
      for (const k of Object.keys(m)) m[k].selected = false;
      m[args.id] = { downloaded: m[args.id]?.downloaded ?? true, selected: true };
      saveModelState(m);
      return undefined as T;
    }
    case "delete_model": {
      const m = mockModelState();
      m[args.id] = { downloaded: false, selected: false };
      saveModelState(m);
      return undefined as T;
    }
    case "transcribe":
      await new Promise((r) => setTimeout(r, 1200));
      return [
        { start_ms: 0, end_ms: 4000, text: "Thanks everyone for joining the kickoff." },
        { start_ms: 4000, end_ms: 9000, text: "Let's lock the v1 scope and owners this week." },
        { start_ms: 9000, end_ms: 14000, text: "Rhys will wire up the Whisper model manager." },
        { start_ms: 14000, end_ms: 19000, text: "We agreed to ship the recorder beta by Friday." },
      ] as T;
    case "diarization_available":
      return true as T;
    case "download_diarization_models":
      await new Promise((r) => setTimeout(r, 600));
      return undefined as T;
    case "diarize":
      await new Promise((r) => setTimeout(r, 800));
      return [
        { start_ms: 0, end_ms: 9000, speaker: 0 },
        { start_ms: 9000, end_ms: 20000, speaker: 1 },
      ] as T;
    case "ollama_status":
      return { available: true, models: ["llama3.2:3b"] } as T;
    case "ai_generate": {
      await new Promise((r) => setTimeout(r, 700));
      const ins = String(args.instruction ?? "").toLowerCase();
      const ctx = String(args.context ?? "");
      if (ins.includes("summar"))
        return ("Summary: " + ctx.slice(0, 120) + "…") as T;
      if (ins.includes("rewrite") || ins.includes("improve"))
        return (ctx ? ctx + " (rewritten for clarity)" : "Rewritten text.") as T;
      if (ins.includes("continue"))
        return "…and here is a thoughtfully continued paragraph generated locally." as T;
      return ("AI: a concise local response to “" +
        String(args.instruction ?? "") +
        "”.") as T;
    }
    case "summarize_transcript":
      await new Promise((r) => setTimeout(r, 900));
      return {
        summary:
          "Kickoff meeting to lock v1 scope and assign owners. The team aligned on shipping the meeting recorder beta by Friday.",
        action_items: [
          "Rhys to wire up the Whisper model manager UI",
          "Finalize v1 scope and owners this week",
        ],
        decisions: ["Ship the recorder beta by Friday"],
      } as T;

    case "search": {
      const q = String(args.query ?? "").trim().toLowerCase();
      if (!q) return { pages: [], rows: [] } as T;
      const pages = d.pages
        .filter(
          (p) =>
            p.title.toLowerCase().includes(q) ||
            (d.documents[p.id] ?? "").toLowerCase().includes(q),
        )
        .slice(0, 25)
        .map((p) => ({ id: p.id, title: p.title, icon: p.icon, type: p.type }));
      const rows: { row_id: string; page_id: string; text: string }[] = [];
      for (const mdb of Object.values(d.databases)) {
        for (const r of mdb.rows) {
          for (const v of Object.values(r.cells)) {
            if (String(v ?? "").toLowerCase().includes(q)) {
              rows.push({ row_id: r.id, page_id: mdb.page_id, text: String(v) });
              break;
            }
          }
          if (rows.length >= 25) break;
        }
      }
      return { pages, rows } as T;
    }

    default:
      throw err("other", `mock: unhandled command "${cmd}"`);
  }
}

function err(kind: string, message: string) {
  return { kind, message };
}

export function resetMockDb() {
  localStorage.removeItem(KEY);
  db = null;
}
