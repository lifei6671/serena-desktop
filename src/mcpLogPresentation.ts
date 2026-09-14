export type McpLogEntry = {
  id: string;
  raw: string;
  timestamp: string | null;
  level: string | null;
  source: string | null;
  message: string;
};

export type McpLogFilter = {
  source: string;
  level: string;
  query: string;
};

const structuredLog = /^(?<level>[A-Z]+)\s+(?<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}) \[(?<source>[^\]]+)\] (?<message>[\s\S]*)$/;

export function parseMcpLogLine(raw: string, index: number, id = `${index}:${raw}`): McpLogEntry {
  const match = structuredLog.exec(raw);
  const groups = match?.groups;

  return {
    id,
    raw,
    timestamp: groups?.timestamp ?? null,
    level: groups?.level ?? null,
    source: groups?.source ?? null,
    message: groups?.message ?? raw,
  };
}

export function reconcileMcpLogEntries(
  previous: McpLogEntry[],
  nextLines: string[],
  createId: (raw: string) => string,
) {
  const maximumOverlap = Math.min(previous.length, nextLines.length);
  let overlap = 0;
  for (let size = maximumOverlap; size > 0; size -= 1) {
    const previousStart = previous.length - size;
    if (previous.slice(previousStart).every((entry, index) => entry.raw === nextLines[index])) {
      overlap = size;
      break;
    }
  }

  const reused = previous.slice(previous.length - overlap);
  return [
    ...reused,
    ...nextLines.slice(overlap).map((raw, index) => parseMcpLogLine(raw, overlap + index, createId(raw))),
  ];
}

export function filterMcpLogs(entries: McpLogEntry[], filter: McpLogFilter) {
  const query = filter.query.trim().toLocaleLowerCase();

  return entries.filter((entry) => {
    if (filter.source && entry.source !== filter.source) return false;
    if (filter.level && entry.level !== filter.level) return false;
    if (!query) return true;

    return [entry.raw, entry.message, entry.source, entry.level]
      .filter((value): value is string => value !== null)
      .some((value) => value.toLocaleLowerCase().includes(query));
  });
}

export function countNewLogLines(previous: string[] | null, next: string[]) {
  if (previous === null) return 0;

  const maximumOverlap = Math.min(previous.length, next.length);
  for (let overlap = maximumOverlap; overlap > 0; overlap -= 1) {
    const previousStart = previous.length - overlap;
    let matches = true;
    for (let index = 0; index < overlap; index += 1) {
      if (previous[previousStart + index] !== next[index]) {
        matches = false;
        break;
      }
    }
    if (matches) return next.length - overlap;
  }

  return next.length;
}

export function sameLogLines(previous: string[] | null, next: string[]) {
  return previous?.length === next.length && previous.every((line, index) => line === next[index]);
}
