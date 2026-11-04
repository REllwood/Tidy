import { describe, expect, it } from "vitest";
import { lineStamp, shortStamp, stampSeconds } from "../playback";

describe("transcript stamps", () => {
  it("reads both stamp formats as seconds", () => {
    expect(stampSeconds("00:12:34")).toBe(754);
    expect(stampSeconds("12:34")).toBe(754);
    expect(stampSeconds("1:02:03")).toBe(3723);
    expect(stampSeconds("soon")).toBeNull();
  });
  it("shows the short form and finds a line's stamp", () => {
    expect(shortStamp("00:12:34")).toBe("12:34");
    expect(shortStamp("01:02:03")).toBe("1:02:03");
    expect(lineStamp("[00:01:12] We moved the launch.")).toBe("00:01:12");
    expect(lineStamp("You")).toBeNull();
  });
});
