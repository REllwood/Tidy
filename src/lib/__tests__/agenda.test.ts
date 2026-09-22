import { describe, it, expect } from "vitest";
import {
  extractTasks,
  buildAgenda,
  bucketOf,
  dueLabel,
  greeting,
  toISODate,
  snoozeDate,
} from "@/lib/agenda";
import type { DatabaseBundle, Field, RowWithCells } from "@/lib/api";

const f = (id: string, type: Field["type"], name: string, extra: Partial<Field> = {}): Field => ({
  id,
  database_id: "d",
  name,
  type,
  options: null,
  position: 1,
  ...extra,
});
const row = (id: string, cells: Record<string, unknown>): RowWithCells => ({
  id,
  database_id: "d",
  position: 1,
  created_at: 0,
  cells,
});

const fields: Field[] = [
  f("name", "text", "Name"),
  f("status", "select", "Status", {
    options: {
      choices: [
        { id: "todo", name: "To do", color: "grey" },
        { id: "done", name: "Done", color: "green" },
      ],
    },
  }),
  f("start", "date", "Start"),
  f("due", "date", "Due"),
];
const bundle: DatabaseBundle = {
  database_id: "roadmap",
  fields,
  rows: [
    row("r1", { name: "Overdue task", status: "todo", due: "2026-07-05" }),
    row("r2", { name: "Today task", status: "todo", due: "2026-07-12" }),
    row("r3", { name: "Week task", status: "todo", due: "2026-07-15" }),
    row("r4", { name: "Later task", status: "todo", due: "2026-08-30" }),
    row("r5", { name: "Done task", status: "done", due: "2026-07-12" }),
    row("r6", { name: "No date", status: "todo" }),
  ],
  views: [],
};
const TODAY = "2026-07-12";

describe("extractTasks", () => {
  it("pulls dated rows, resolves status + done, prefers the Due field", () => {
    const tasks = extractTasks(bundle, { pageId: "p", title: "Roadmap" });
    expect(tasks).toHaveLength(5); // r6 has no due date
    const t1 = tasks.find((t) => t.rowId === "r1")!;
    expect(t1.title).toBe("Overdue task");
    expect(t1.due).toBe("2026-07-05");
    expect(t1.status).toBe("To do");
    expect(t1.done).toBe(false);
    expect(tasks.find((t) => t.rowId === "r5")!.done).toBe(true);
  });
  it("returns nothing for a database with no date field", () => {
    const noDate: DatabaseBundle = { ...bundle, fields: [fields[0], fields[1]] };
    expect(extractTasks(noDate, { pageId: "p", title: "x" })).toEqual([]);
  });
  it("resolves the mutation ids (due field, status field, done/todo choices)", () => {
    const t = extractTasks(bundle, { pageId: "p", title: "Roadmap" })[0];
    expect(t.dueFieldId).toBe("due");
    expect(t.statusFieldId).toBe("status");
    expect(t.doneChoiceId).toBe("done");
    expect(t.todoChoiceId).toBe("todo");
  });
  it("picks the real Status select even when another select (Priority) comes first", () => {
    const b: DatabaseBundle = {
      database_id: "d",
      fields: [
        f("name", "text", "Name"),
        f("prio", "select", "Priority", {
          options: { choices: [{ id: "hi", name: "High", color: "red" }] },
        }),
        f("due", "date", "Due"),
        f("st", "select", "Status", {
          options: {
            choices: [
              { id: "todo", name: "To do", color: "grey" },
              { id: "done", name: "Done", color: "green" },
            ],
          },
        }),
      ],
      rows: [row("r", { name: "X", due: "2026-07-12", st: "done" })],
      views: [],
    };
    const t = extractTasks(b, { pageId: "p", title: "t" })[0];
    expect(t.statusFieldId).toBe("st"); // not "prio"
    expect(t.doneChoiceId).toBe("done");
    expect(t.done).toBe(true);
  });
  it("todo fallback is never the done choice (un-complete would be a no-op)", () => {
    const b: DatabaseBundle = {
      database_id: "d",
      fields: [
        f("name", "text", "Name"),
        f("st", "select", "Status", {
          options: {
            choices: [
              { id: "c", name: "Completed", color: "green" },
              { id: "v", name: "Verified", color: "blue" },
            ],
          },
        }),
        f("due", "date", "Due"),
      ],
      rows: [row("r", { name: "X", due: "2026-07-12", st: "c" })],
      views: [],
    };
    const t = extractTasks(b, { pageId: "p", title: "t" })[0];
    expect(t.doneChoiceId).toBe("c");
    expect(t.todoChoiceId).toBe("v");
    expect(t.todoChoiceId).not.toBe(t.doneChoiceId);
  });
});

describe("snoozeDate", () => {
  const wed = new Date("2026-07-15T09:00:00"); // a Wednesday
  it("today = same day, tomorrow = +1", () => {
    expect(snoozeDate("today", wed)).toBe("2026-07-15");
    expect(snoozeDate("tomorrow", wed)).toBe("2026-07-16");
  });
  it("weekend = upcoming Saturday, nextweek = next Monday", () => {
    expect(snoozeDate("weekend", wed)).toBe("2026-07-18"); // Sat
    expect(snoozeDate("nextweek", wed)).toBe("2026-07-20"); // Mon
  });
});

describe("bucketOf", () => {
  it("classifies relative to today", () => {
    expect(bucketOf("2026-07-05", TODAY)).toBe("overdue");
    expect(bucketOf("2026-07-12", TODAY)).toBe("today");
    expect(bucketOf("2026-07-15", TODAY)).toBe("week");
    expect(bucketOf("2026-07-19", TODAY)).toBe("week"); // exactly +7
    expect(bucketOf("2026-07-20", TODAY)).toBe("later");
  });
});

describe("buildAgenda", () => {
  it("buckets open tasks and drops done ones", () => {
    const tasks = extractTasks(bundle, { pageId: "p", title: "Roadmap" });
    const a = buildAgenda(tasks, TODAY);
    expect(a.overdue.map((t) => t.title)).toEqual(["Overdue task"]);
    expect(a.today.map((t) => t.title)).toEqual(["Today task"]); // "Done task" excluded
    expect(a.week.map((t) => t.title)).toEqual(["Week task"]);
    expect(a.later.map((t) => t.title)).toEqual(["Later task"]);
  });
});

describe("dueLabel", () => {
  it("gives friendly relative labels", () => {
    expect(dueLabel("2026-07-12", TODAY)).toBe("Today");
    expect(dueLabel("2026-07-13", TODAY)).toBe("Tomorrow");
    expect(dueLabel("2026-07-11", TODAY)).toBe("Yesterday");
    expect(dueLabel("2026-07-05", TODAY)).toBe("7 days ago");
  });
});

describe("greeting / toISODate", () => {
  it("greets by hour", () => {
    expect(greeting(new Date("2026-07-12T09:00:00"))).toBe("Good morning");
    expect(greeting(new Date("2026-07-12T14:00:00"))).toBe("Good afternoon");
    expect(greeting(new Date("2026-07-12T20:00:00"))).toBe("Good evening");
  });
  it("formats a local date without UTC drift", () => {
    expect(toISODate(new Date("2026-07-12T23:30:00"))).toBe("2026-07-12");
  });
});

describe("done detection", () => {
  const withChoices = (
    fieldName: string,
    choices: { id: string; name: string }[],
    selected: string,
  ): DatabaseBundle => ({
    database_id: "d",
    fields: [
      f("name", "text", "Name"),
      f("st", "select", fieldName, {
        options: { choices: choices.map((c) => ({ ...c, color: "grey" })) },
      }),
      f("due", "date", "Due"),
    ],
    rows: [row("r", { name: "X", due: "2026-07-12", st: selected })],
    views: [],
  });

  it('treats "Complete" as done and "Incomplete" as open', () => {
    const choices = [
      { id: "i", name: "Incomplete" },
      { id: "c", name: "Complete" },
    ];
    const open = extractTasks(withChoices("Status", choices, "i"), {
      pageId: "p",
      title: "t",
    })[0];
    expect(open.doneChoiceId).toBe("c");
    expect(open.done).toBe(false);
    expect(open.todoChoiceId).toBe("i");

    const finished = extractTasks(withChoices("Status", choices, "c"), {
      pageId: "p",
      title: "t",
    })[0];
    expect(finished.done).toBe(true);
  });

  it('treats "Not done" as open and "Done" as done', () => {
    const choices = [
      { id: "n", name: "Not done" },
      { id: "d", name: " Done " },
    ];
    const open = extractTasks(withChoices("Status", choices, "n"), {
      pageId: "p",
      title: "t",
    })[0];
    expect(open.doneChoiceId).toBe("d"); // trimmed, anchored match
    expect(open.done).toBe(false);

    const finished = extractTasks(withChoices("Status", choices, "d"), {
      pageId: "p",
      title: "t",
    })[0];
    expect(finished.done).toBe(true);
  });

  it("falls back to the last choice for a non-English status field", () => {
    const choices = [
      { id: "o", name: "Offen" },
      { id: "b", name: "In Bearbeitung" },
      { id: "e", name: "Erledigt" },
    ];
    const t = extractTasks(withChoices("Status", choices, "e"), {
      pageId: "p",
      title: "t",
    })[0];
    expect(t.doneChoiceId).toBe("e");
    expect(t.done).toBe(true);
    expect(t.todoChoiceId).toBe("o");
  });

  it("marks nothing done when the field isn't clearly a status field", () => {
    const choices = [
      { id: "p", name: "Petit" },
      { id: "g", name: "Grand" },
    ];
    const t = extractTasks(withChoices("Taille", choices, "g"), {
      pageId: "p",
      title: "t",
    })[0];
    expect(t.doneChoiceId).toBeNull();
    expect(t.done).toBe(false);
    expect(t.todoChoiceId).toBe("p");
  });

  it("never guesses done from a single-choice status field", () => {
    const t = extractTasks(
      withChoices("Status", [{ id: "only", name: "Bezig" }], "only"),
      { pageId: "p", title: "t" },
    )[0];
    expect(t.doneChoiceId).toBeNull();
    expect(t.done).toBe(false);
  });

  it('prefers an exact match over a later "done-ish" name', () => {
    const choices = [
      { id: "d", name: "Done" },
      { id: "a", name: "Archived" },
    ];
    const t = extractTasks(withChoices("Progress", choices, "d"), {
      pageId: "p",
      title: "t",
    })[0];
    expect(t.doneChoiceId).toBe("d"); // not the last choice
    expect(t.done).toBe(true);
  });
});
