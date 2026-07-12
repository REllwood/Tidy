import { invoke } from "@/lib/tauri";

export type PageType = "doc" | "database" | "record";

export interface Page {
  id: string;
  parent_id: string | null;
  title: string;
  icon: string | null;
  type: PageType;
  position: number;
  is_favorite: boolean;
  created_at: number;
  updated_at: number;
}

export interface AppErrorShape {
  kind: string;
  message: string;
}

// ---- Pages ----------------------------------------------------------------

export const pagesApi = {
  list: () => invoke<Page[]>("list_pages"),
  get: (id: string) => invoke<Page>("get_page", { id }),
  create: (parentId: string | null, title: string, kind: PageType = "doc") =>
    invoke<Page>("create_page", { parentId, title, kind }),
  rename: (id: string, title: string) =>
    invoke<Page>("rename_page", { id, title }),
  setIcon: (id: string, icon: string | null) =>
    invoke<Page>("set_page_icon", { id, icon }),
  setFavorite: (id: string, isFavorite: boolean) =>
    invoke<Page>("set_page_favorite", { id, isFavorite }),
  move: (id: string, parentId: string | null, position: number) =>
    invoke<Page>("move_page", { id, parentId, position }),
  delete: (id: string) => invoke<void>("delete_page", { id }),
};

// ---- Documents ------------------------------------------------------------

export const documentsApi = {
  get: (id: string) => invoke<string>("get_document", { id }),
  update: (id: string, content: string) =>
    invoke<void>("update_document", { id, content }),
};

// ---- Databases ------------------------------------------------------------

export type FieldType =
  | "text"
  | "number"
  | "select"
  | "date"
  | "checkbox"
  | "dependencies"
  | "relation"
  | "lookup"
  | "rollup"
  | "formula";

export type RollupFn = "count" | "sum" | "avg" | "min" | "max";

export interface SelectChoice {
  id: string;
  name: string;
  color: string;
}
export interface FieldOptions {
  choices?: SelectChoice[];
  // relation
  targetDatabaseId?: string;
  multi?: boolean;
  // lookup / rollup
  relationFieldId?: string;
  targetFieldId?: string;
  fn?: RollupFn;
  // formula
  expr?: string;
}
export interface Field {
  id: string;
  database_id: string;
  name: string;
  type: FieldType;
  options: FieldOptions | null;
  position: number;
}
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type CellValue = any;
export interface RowWithCells {
  id: string;
  database_id: string;
  position: number;
  created_at: number;
  cells: Record<string, CellValue>;
}
export interface ViewConfig {
  groupByFieldId?: string;
  dateFieldId?: string;
  startFieldId?: string;
  endFieldId?: string;
  dependenciesFieldId?: string;
  filters?: { fieldId: string; op: string; value: CellValue }[];
  sorts?: { fieldId: string; dir: "asc" | "desc" }[];
  columnWidths?: Record<string, number>;
}
export interface DbView {
  id: string;
  database_id: string;
  kind: "grid" | "board" | "calendar" | "gantt";
  config: ViewConfig | null;
  position: number;
}
export interface DatabaseBundle {
  database_id: string;
  fields: Field[];
  rows: RowWithCells[];
  views: DbView[];
}
export interface DatabaseSummary {
  database_id: string;
  page_id: string;
  title: string;
  icon: string | null;
  fields: Field[];
}

export const databasesApi = {
  get: (pageId: string) => invoke<DatabaseBundle>("get_database", { pageId }),
  getById: (databaseId: string) =>
    invoke<DatabaseBundle>("get_database_by_id", { databaseId }),
  list: () => invoke<DatabaseSummary[]>("list_databases"),
  promoteRow: (rowId: string, title: string) =>
    invoke<Page>("promote_row", { rowId, title }),
  createField: (
    databaseId: string,
    name: string,
    kind: FieldType,
    options?: FieldOptions,
  ) => invoke<Field>("create_field", { databaseId, name, kind, options }),
  updateField: (id: string, name?: string, options?: FieldOptions) =>
    invoke<Field>("update_field", { id, name, options }),
  deleteField: (id: string) => invoke<void>("delete_field", { id }),
  createRow: (databaseId: string) =>
    invoke<RowWithCells>("create_row", { databaseId }),
  deleteRow: (id: string) => invoke<void>("delete_row", { id }),
  moveRow: (id: string, position: number) =>
    invoke<void>("move_row", { id, position }),
  setCell: (rowId: string, fieldId: string, value: CellValue) =>
    invoke<void>("set_cell", { rowId, fieldId, value }),
  updateView: (id: string, config: ViewConfig) =>
    invoke<DbView>("update_view", { id, config }),
};

// ---- Knowledge: wiki-links / backlinks / tags -----------------------------

export interface LinkInput {
  target_page_id?: string | null;
  dst_title?: string | null;
  context?: string | null;
}
export interface Backlink {
  source_page_id: string;
  source_title: string;
  source_icon: string | null;
  context: string | null;
}

export interface GraphNode {
  id: string;
  title: string;
  icon: string | null;
  type: string;
  degree: number;
}
export interface GraphEdge {
  source: string;
  target: string;
}
export interface LinkGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export const knowledgeApi = {
  setPageLinks: (pageId: string, links: LinkInput[], tags: string[]) =>
    invoke<void>("set_page_links", { pageId, links, tags }),
  getBacklinks: (pageId: string) =>
    invoke<Backlink[]>("get_backlinks", { pageId }),
  getPageTags: (pageId: string) => invoke<string[]>("get_page_tags", { pageId }),
  getGraph: () => invoke<LinkGraph>("get_graph"),
};

// ---- Search ---------------------------------------------------------------

export interface PageHit {
  id: string;
  title: string;
  icon: string | null;
  type: PageType;
}
export interface RowHit {
  row_id: string;
  page_id: string;
  text: string;
}
export interface SearchResults {
  pages: PageHit[];
  rows: RowHit[];
}

export const searchApi = {
  query: (query: string) => invoke<SearchResults>("search", { query }),
};

// ---- Meeting: recording / models / transcription / summary ----------------

export interface Recording {
  audio_path: string;
  duration_ms: number;
}
export interface ModelInfo {
  id: string;
  name: string;
  size: number; // bytes
  downloaded: boolean;
  selected: boolean;
}
export interface TranscriptSegment {
  start_ms: number;
  end_ms: number;
  text: string;
}
export interface OllamaStatus {
  available: boolean;
  models: string[];
}
export interface MeetingSummary {
  summary: string;
  action_items: string[];
  decisions: string[];
}

export const recordingApi = {
  start: () => invoke<void>("start_recording"),
  stop: () => invoke<Recording>("stop_recording"),
  isRecording: () => invoke<boolean>("is_recording"),
  record: (
    pageId: string,
    durationMs: number,
    audioPath: string | null,
    modelUsed: string | null,
  ) =>
    invoke<void>("record_meeting", {
      pageId,
      durationMs,
      audioPath,
      modelUsed,
    }),
};

export const modelsApi = {
  list: () => invoke<ModelInfo[]>("list_models"),
  download: (id: string) => invoke<void>("download_model", { id }),
  select: (id: string) => invoke<void>("select_model", { id }),
  remove: (id: string) => invoke<void>("delete_model", { id }),
};

export const transcribeApi = {
  run: (audioPath: string) =>
    invoke<TranscriptSegment[]>("transcribe", { audioPath }),
};

export interface SpeakerSegment {
  start_ms: number;
  end_ms: number;
  speaker: number;
}
export const diarizeApi = {
  run: (audioPath: string, numSpeakers = 0) =>
    invoke<SpeakerSegment[]>("diarize", { audioPath, numSpeakers }),
  available: () => invoke<boolean>("diarization_available"),
  download: () => invoke<void>("download_diarization_models"),
};

export const ollamaApi = {
  status: () => invoke<OllamaStatus>("ollama_status"),
  summarize: (transcript: string) =>
    invoke<MeetingSummary>("summarize_transcript", { transcript }),
  generate: (instruction: string, context?: string) =>
    invoke<string>("ai_generate", { instruction, context }),
};

// ---- Ingest (flagship: blob -> filed note + tasks) ------------------------

export interface IngestResult {
  page_id: string;
  client_page_id: string | null;
  task_row_ids: string[];
  summary: string;
}
export interface IngestArgs {
  rawText: string;
  clientHint?: string;
  meetingId?: string;
  taskDbId?: string;
  title?: string;
  /** Pre-rendered BlockNote JSON, skips summarization (recorder path). */
  bodyJson?: string;
  /** Explicit action items → Task rows (used with bodyJson). */
  actionItems?: string[];
}

export const ingestApi = {
  ingestNote: (args: IngestArgs) =>
    invoke<IngestResult>("ingest_note", { ...args }),
};

// ---- Vault (hybrid Markdown mirror) ---------------------------------------

export const vaultApi = {
  getDir: () => invoke<string | null>("get_vault_dir"),
  setDir: (path: string) => invoke<number>("set_vault_dir", { path }),
  exportAll: () => invoke<number>("export_vault"),
  flushPage: (pageId: string) => invoke<void>("flush_page", { pageId }),
};

// ---- MCP connection -------------------------------------------------------

export interface McpInfo {
  token: string;
  sidecar_path: string;
  claude_command: string;
}

export const mcpApi = {
  getToken: () => invoke<string | null>("mcp_get_token"),
  enable: () => invoke<McpInfo>("mcp_enable"),
  disable: () => invoke<void>("mcp_disable"),
};

// ---- Tree helpers ---------------------------------------------------------

export interface PageNode extends Page {
  children: PageNode[];
}

/** Build a nested tree (sorted by position) from the flat page list. */
export function buildTree(pages: Page[]): PageNode[] {
  const byId = new Map<string, PageNode>();
  for (const p of pages) byId.set(p.id, { ...p, children: [] });
  const roots: PageNode[] = [];
  for (const node of byId.values()) {
    if (node.parent_id && byId.has(node.parent_id)) {
      byId.get(node.parent_id)!.children.push(node);
    } else {
      roots.push(node);
    }
  }
  const sort = (nodes: PageNode[]) => {
    nodes.sort((a, b) => a.position - b.position);
    nodes.forEach((n) => sort(n.children));
  };
  sort(roots);
  return roots;
}
