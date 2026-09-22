import { describe, it, expect } from "vitest";
import {
  NONE,
  appendPosition,
  columnOf,
  columnRows,
  planDropAtEnd,
  planDropBefore,
  renormalise,
} from "@/components/db/boardOrder";
import type { RowWithCells } from "@/lib/api";

const GID = "status";
const row = (id: string, position: number, status?: string): RowWithCells => ({
  id,
  database_id: "d",
  position,
  created_at: 0,
  cells: status === undefined ? {} : { [GID]: status },
});

describe("columnOf / columnRows", () => {
  it("treats null and empty as the 'no status' column", () => {
    expect(columnOf(row("a", 1), GID)).toBe(NONE);
    expect(columnOf(row("b", 1, ""), GID)).toBe(NONE);
    expect(columnOf(row("c", 1, "todo"), GID)).toBe("todo");
  });
  it("orders a column by position", () => {
    const rows = [row("a", 3, "todo"), row("b", 1, "todo"), row("c", 2, "done")];
    expect(columnRows(rows, GID, "todo").map((r) => r.id)).toEqual(["b", "a"]);
  });
});

describe("appendPosition", () => {
  it("is one past the highest position in the WHOLE database", () => {
    const all = [row("a", 1, "todo"), row("hidden", 9, "done"), row("b", 2, "todo")];
    expect(appendPosition(all)).toBe(10);
    // the visible subset alone would have collided with the hidden row
    expect(appendPosition(all.filter((r) => r.cells[GID] === "todo"))).toBe(3);
  });
  it("starts at 1 for an empty database", () => {
    expect(appendPosition([])).toBe(1);
    expect(planDropAtEnd([]).position).toBe(1);
  });
});

describe("planDropBefore", () => {
  it("takes the midpoint between the target and its previous neighbour", () => {
    const all = [row("a", 1, "todo"), row("b", 2, "todo"), row("z", 5, "done")];
    const plan = planDropBefore(all, GID, "z", "b")!;
    expect(plan.position).toBe(1.5);
    expect(plan.renormalise).toBeNull();
  });

  it("uses neighbours from the whole set, including filtered-out rows", () => {
    const all = [row("a", 1, "todo"), row("hidden", 2, "todo"), row("b", 3, "todo"), row("z", 9, "done")];
    // Seen through a filter, "a" looks like b's neighbour (midpoint 2 = hidden),
    // but the plan is computed from every row, so it lands after "hidden".
    expect(planDropBefore(all, GID, "z", "b")!.position).toBe(2.5);
  });

  it("goes before the first card when the target starts the column", () => {
    const all = [row("a", 4, "todo"), row("z", 9, "done")];
    expect(planDropBefore(all, GID, "z", "a")!.position).toBe(3);
  });

  it("ignores the dragged row when finding the neighbour", () => {
    const all = [row("a", 1, "todo"), row("z", 2, "todo"), row("b", 3, "todo")];
    // z sits between a and b already; dropping it before b must use a, not z.
    expect(planDropBefore(all, GID, "z", "b")!.position).toBe(2);
  });

  it("renumbers 1..n when the gap has collapsed, then slots into it", () => {
    const all = [
      row("a", 1, "todo"),
      row("b", 1 + 1e-9, "todo"),
      row("z", 7, "done"),
    ];
    const plan = planDropBefore(all, GID, "z", "b")!;
    expect(plan.renormalise).toEqual([
      { id: "a", position: 1 },
      { id: "b", position: 2 },
      { id: "z", position: 3 },
    ]);
    expect(plan.position).toBe(1.5);
  });

  it("refuses a drop onto itself or a missing target", () => {
    const all = [row("a", 1, "todo")];
    expect(planDropBefore(all, GID, "a", "a")).toBeNull();
    expect(planDropBefore(all, GID, "a", "nope")).toBeNull();
  });
});

describe("renormalise", () => {
  it("renumbers every row 1..n in its current order", () => {
    const all = [row("b", 2.5), row("a", 0.1), row("c", 99)];
    expect(renormalise(all)).toEqual([
      { id: "a", position: 1 },
      { id: "b", position: 2 },
      { id: "c", position: 3 },
    ]);
  });
});
