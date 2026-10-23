export function parseTimestamp(value?: string | number | null): number | null {
  if (value === undefined || value === null || String(value).trim() === "") return null;
  const numeric = Number(value);
  if (Number.isFinite(numeric) && numeric > 0) return numeric;
  const parsed = Date.parse(String(value));
  return Number.isFinite(parsed) ? parsed : null;
}

export function formatLocalDateTime(value: string | number, now = Date.now()): string {
  const timestamp = parseTimestamp(value);
  if (!timestamp) return "时间未知";
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return "时间未知";
  const current = new Date(now);
  const pad = (part: number) => String(part).padStart(2, "0");
  if (
    date.getFullYear() === current.getFullYear() &&
    date.getMonth() === current.getMonth() &&
    date.getDate() === current.getDate()
  ) {
    return `今天 ${pad(date.getHours())}:${pad(date.getMinutes())}`;
  }
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function formatTimestampOrFallback(value: string | null | undefined, fallback: string): string {
  return value?.trim() ? formatLocalDateTime(value) : fallback;
}
