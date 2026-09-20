export type McpLogEntry = {
  id: string;
  raw: string;
  timestamp: string | null;
  level: string | null;
  source: string | null;
  message: string;
  details: McpLogDetails | null;
};

export type McpLogDetails = Record<string, unknown>;

export type McpLogFilter = {
  source: string;
  level: string;
  query: string;
};

const structuredLog = /^(?<level>[A-Z]+)\s+(?<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}) \[(?<source>[^\]]+)\] (?<message>[\s\S]*)$/;
const detailMarker = "\t@serena-details=";

// 结构化诊断仅供详情面板使用，无法解析时必须保留历史原文。
function extractDetails(message: string): { message: string; details: McpLogDetails | null } {
  const markerIndex = message.lastIndexOf(detailMarker);
  if (markerIndex < 0) return { message, details: null };

  try {
    const details: unknown = JSON.parse(message.slice(markerIndex + detailMarker.length));
    if (details && typeof details === "object" && !Array.isArray(details)) {
      return { message: message.slice(0, markerIndex), details: details as McpLogDetails };
    }
  } catch {
    // 无法识别的旧日志或手工日志必须完整保留，不能因详情标记丢失正文。
  }
  return { message, details: null };
}

export function parseMcpLogLine(raw: string, index: number, id = `${index}:${raw}`): McpLogEntry {
  const match = structuredLog.exec(raw);
  const groups = match?.groups;
  const parsed = extractDetails(groups?.message ?? raw);

  return {
    id,
    raw,
    timestamp: groups?.timestamp ?? null,
    level: groups?.level ?? null,
    source: groups?.source ?? null,
    message: parsed.message,
    details: parsed.details,
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

    return [entry.raw, entry.message, entry.source, entry.level, entry.details && JSON.stringify(entry.details)]
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
