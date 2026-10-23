import { describe, expect, it } from "vitest";
import { formatLocalDateTime, formatTimestampOrFallback, parseTimestamp } from "./dateTime";

describe("local date-time display", () => {
  it("formats epoch milliseconds and ISO timestamps as local time", () => {
    const now = new Date(2026, 9, 12, 15, 0).getTime();
    expect(formatLocalDateTime(new Date(2026, 9, 12, 14, 5).getTime(), now)).toBe("今天 14:05");
    expect(formatLocalDateTime(new Date(2026, 9, 9, 9, 7).toISOString(), now)).toBe("10-09 09:07");
  });

  it("keeps empty and invalid timestamp states explicit", () => {
    expect(parseTimestamp("")).toBeNull();
    expect(formatLocalDateTime("not-a-date")).toBe("时间未知");
    expect(formatLocalDateTime(Number.MAX_VALUE)).toBe("时间未知");
    expect(formatTimestampOrFallback("", "尚未生成")).toBe("尚未生成");
  });
});
