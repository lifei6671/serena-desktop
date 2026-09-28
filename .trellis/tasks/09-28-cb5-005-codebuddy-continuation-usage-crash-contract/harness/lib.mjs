import { createHash } from 'node:crypto';
import { appendFileSync, lstatSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { basename, relative, resolve, sep } from 'node:path';
import { tmpdir } from 'node:os';

export const RECOVERY_METHOD = 'session/load';
export const TASK_TEMP_PREFIX = 'cb5-005-20260928-';

/** Attempt 5 的 P3 不包含 marker 或 Workspace 字面值，只要求从上一 Runtime 的历史中回忆。 */
export const CONTINUATION_RECALL_PROMPT =
  'Without restating any earlier prompt or being given the literal again, reply with the literal you were asked to remember in the previous runtime, then a vertical bar, then your current absolute working directory. Do not use tools or modify files.';

const HOST_ENVIRONMENT_KEYS = [
  'CODEBUDDY_API_KEY',
  'CODEBUDDY_TOKEN',
  'CODEBUDDY_BASE_URL',
  'CODEBUDDY_ENDPOINT',
  'CODEBUDDY_MODEL',
  'CODEBUDDY_CONFIG_DIR',
  'CODEBUDDY_HOME',
  'HTTP_PROXY',
  'HTTPS_PROXY',
  'NO_PROXY',
  'ALL_PROXY',
  'http_proxy',
  'https_proxy',
  'no_proxy',
  'all_proxy',
];

/** 返回唯一 Host attempt-4 evidence 目录，避免覆盖前三次 Probe。 */
export function hostAttempt4EvidenceRoot(taskRoot) {
  return resolve(taskRoot, 'evidence', 'attempt-4');
}

/** 返回独立 Host attempt-5 Continuation evidence 目录，禁止覆盖 attempt 4。 */
export function hostAttempt5EvidenceRoot(taskRoot) {
  return resolve(taskRoot, 'evidence', 'attempt-5');
}

/** 将 Windows 路径统一为大小写无关、斜杠等价的包含判定形式。 */
function normalizeWindowsPathForContainment(value) {
  return String(value).replaceAll('/', '\\').toLowerCase();
}

/** 只返回 Continuation answer 的 hash/长度与语义事实，不保留 answer 正文。 */
export function continuationAnswerFacts(answer, marker, workspace) {
  const text = String(answer);
  return {
    answerSha256: sha256(text),
    answerUtf8Bytes: Buffer.byteLength(text, 'utf8'),
    markerPresent: text.includes(marker),
    cwdPresent: normalizeWindowsPathForContainment(text).includes(
      normalizeWindowsPathForContainment(workspace),
    ),
  };
}

/** 生成 allowlist-only Host 环境 provenance；只记录键是否存在，绝不保存值。 */
export function hostEnvironmentProvenance(env, runtime) {
  const variables = Object.fromEntries(
    HOST_ENVIRONMENT_KEYS.map((key) => [key, Object.hasOwn(env, key) ? 'present' : 'absent']),
  );
  return {
    runner: 'serena-command-host',
    hostGatePresent: env.SERENA_CB5_HOST_PROBE === '1',
    platform: runtime.platform,
    nodeVersion: runtime.nodeVersion,
    launchSpec: {
      nodePath: runtime.nodePath,
      nodeSha256: runtime.nodeSha256,
      scriptPath: runtime.scriptPath,
      scriptSha256: runtime.scriptSha256,
      argv: [runtime.nodePath, runtime.scriptPath, '--acp'],
    },
    proxyEnvironmentPresent: Object.entries(variables).some(
      ([key, status]) => key.toUpperCase().includes('PROXY') && status === 'present',
    ),
    codebuddyEnvironmentPresent: Object.entries(variables).some(
      ([key, status]) => key.startsWith('CODEBUDDY_') && status === 'present',
    ),
    variables,
    valuesRecorded: false,
  };
}

/** 生成 CB5-004 Host fresh-session PASS 使用的逐字段精确 initialize params。 */
export function cb5004InitializeParams() {
  return {
    protocolVersion: 1,
    clientCapabilities: {
      elicitation: { form: {} },
      _meta: {
        'subagent-transcript': true,
        parameterizedModelPicker: true,
      },
    },
    clientInfo: {
      name: 'serena-desktop-gold-band-repro',
      title: 'SerenaDesktop Gold Band Repro',
      version: '0.1',
    },
  };
}

const TEXT_KEYS = new Set([
  'text',
  'message',
  'thought',
  'prompt',
  'content',
  'title',
  'description',
  'label',
  'name',
]);

const SECRET_KEYS = new Set([
  'authorization',
  'token',
  'accessToken',
  'apiKey',
  'credential',
  'environment',
  'env',
  'stderr',
]);

/** 计算 Buffer 或字符串的 SHA256。 */
export function sha256(value) {
  return createHash('sha256').update(value).digest('hex');
}

/** 以摘要和 UTF-8 长度替换不应落盘的正文。 */
export function redactText(value) {
  const bytes = Buffer.from(value, 'utf8');
  return { redacted: true, sha256: sha256(bytes), utf8Bytes: bytes.length };
}

/** 递归清洗 wire，同时保留协议结构、标识与数值语义。 */
export function sanitizeWire(value, workspace = null, key = '') {
  if (value === null || value === undefined) return value;
  if (SECRET_KEYS.has(key)) return '<REDACTED_SECRET>';
  if (typeof value === 'string') {
    const replaced = workspace ? value.split(workspace).join('<TEMP_WORKSPACE>') : value;
    return TEXT_KEYS.has(key) ? redactText(replaced) : replaced;
  }
  if (Array.isArray(value)) {
    return value.map((entry) => sanitizeWire(entry, workspace, key));
  }
  if (typeof value !== 'object') return value;
  const sanitized = {};
  for (const [childKey, childValue] of Object.entries(value)) {
    sanitized[childKey] = sanitizeWire(childValue, workspace, childKey);
  }
  return sanitized;
}

/** 解析单行 JSON-RPC，并拒绝空行、数组和非对象。 */
export function parseJsonRpcLine(line) {
  if (!line.trim()) throw new Error('EMPTY_JSON_RPC_LINE');
  const value = JSON.parse(line);
  if (!value || Array.isArray(value) || typeof value !== 'object') {
    throw new Error('JSON_RPC_FRAME_NOT_OBJECT');
  }
  return value;
}

/** 提取 ACP session/update 的 typed update 对象。 */
export function sessionUpdateOf(message) {
  if (message?.method !== 'session/update') return null;
  const update = message?.params?.update;
  return update && typeof update === 'object' ? update : null;
}

/** 提取 usage_update，保留真实字段而不预设 token 语义。 */
export function usageUpdateOf(message) {
  const update = sessionUpdateOf(message);
  return update?.sessionUpdate === 'usage_update' ? update : null;
}

/** 从 agent_message_chunk 组合可见回答正文，仅保存在内存中做断言。 */
export function agentTextOf(message) {
  const update = sessionUpdateOf(message);
  if (update?.sessionUpdate !== 'agent_message_chunk') return '';
  const content = update.content;
  if (typeof content?.text === 'string') return content.text;
  if (typeof update.text === 'string') return update.text;
  return '';
}

/** 递归收集可能的 Provider request identity，供绑定能力判定。 */
export function collectRequestIdentities(value, found = new Set()) {
  if (!value || typeof value !== 'object') return found;
  for (const [key, child] of Object.entries(value)) {
    if (
      typeof child === 'string' &&
      ['conversationRequestId', 'providerRequestId', 'requestId', 'promptRequestId'].includes(key)
    ) {
      found.add(`${key}:${child}`);
    } else if (child && typeof child === 'object') {
      collectRequestIdentities(child, found);
    }
  }
  return found;
}

/** 判断 R2 是否真的重新收到 original prompt RPC id 的 typed terminal，而非历史 update 或 error。 */
export function hasExactRecoveredPromptResponse(messages, promptRpcId, afterSequence) {
  if (!promptRpcId) return false;
  return messages.some(
    (row) =>
      row.sequence > afterSequence &&
      row.direction === 'provider_to_client' &&
      String(row.message?.id) === String(promptRpcId) &&
      row.message?.method === undefined &&
      typeof row.message?.result?.stopReason === 'string',
  );
}

/** 创建 append-only JSONL writer；每个运行只允许新建目标文件。 */
export class JsonlWriter {
  constructor(path) {
    mkdirSync(resolve(path, '..'), { recursive: true });
    writeFileSync(path, '', { encoding: 'utf8', flag: 'wx' });
    this.path = path;
  }

  /** 追加单条 JSON，并立即同步到文件系统。 */
  append(value) {
    appendFileSync(this.path, `${JSON.stringify(value)}\n`, { encoding: 'utf8', flush: true });
  }
}

/** 生成完整、相对路径化的 Workspace manifest。 */
export function workspaceManifest(root) {
  const manifest = {};
  const visit = (absolute) => {
    for (const entry of readdirSync(absolute, { withFileTypes: true })) {
      const path = resolve(absolute, entry.name);
      const key = relative(root, path).split(sep).join('/');
      const info = lstatSync(path);
      if (info.isSymbolicLink()) throw new Error(`WORKSPACE_LINK_REJECTED:${key}`);
      if (entry.isDirectory()) {
        manifest[key] = { kind: 'directory' };
        visit(path);
      } else if (entry.isFile()) {
        const bytes = readFileSync(path);
        manifest[key] = { kind: 'file', size: bytes.length, sha256: sha256(bytes) };
      } else {
        throw new Error(`WORKSPACE_SPECIAL_ENTRY_REJECTED:${key}`);
      }
    }
  };
  visit(root);
  return manifest;
}

/** 验证路径是本任务在系统临时目录下创建的独立 Workspace。 */
export function assertOwnedTempWorkspace(root) {
  const canonicalRoot = resolve(root);
  const canonicalTemp = resolve(tmpdir());
  if (!canonicalRoot.startsWith(`${canonicalTemp}${sep}`)) {
    throw new Error(`WORKSPACE_OUTSIDE_TEMP:${canonicalRoot}`);
  }
  if (!basename(canonicalRoot).startsWith(TASK_TEMP_PREFIX)) {
    throw new Error(`WORKSPACE_PREFIX_MISMATCH:${basename(canonicalRoot)}`);
  }
  return canonicalRoot;
}

/** 删除已验证归属的临时 Workspace；调用前必须先完成 final manifest。 */
export function removeOwnedTempWorkspace(root) {
  const canonicalRoot = assertOwnedTempWorkspace(root);
  rmSync(canonicalRoot, { recursive: true, force: false, maxRetries: 0 });
  return !statExists(canonicalRoot);
}

/** 只读判断路径是否存在。 */
export function statExists(path) {
  try {
    statSync(path);
    return true;
  } catch (error) {
    if (error?.code === 'ENOENT') return false;
    throw error;
  }
}

/** 把 usage 字段的 JS 类型映射为证据中的稳定名称。 */
export function valueType(value) {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  if (Number.isInteger(value)) return 'integer';
  return typeof value;
}

/** 保守汇总真实 usage 事件；不从字段名推断 public token 语义。 */
export function analyzeUsage(events, promptWindows) {
  const windowsWithIdentity = promptWindows.map((window) => ({
    ...window,
    identities: new Set([
      ...(window.rpcId ? [`requestId:${window.rpcId}`, `promptRequestId:${window.rpcId}`] : []),
      ...(window.providerRequestIdentities ?? []),
    ]),
  }));
  const eventBindings = new Map();
  for (const event of events) {
    const eventIdentities = new Set(event.requestIdentities);
    const matches = windowsWithIdentity.filter(
      (window) =>
        event.sequence > window.promptSequence &&
        event.sequence <= window.graceEndSequence &&
        (!window.sessionId || event.sessionId === window.sessionId) &&
        [...window.identities].some((identity) => eventIdentities.has(identity)),
    );
    eventBindings.set(event, matches.length === 1 ? matches[0] : null);
  }
  const exactPromptBound = events.length > 0 && events.every((event) => eventBindings.get(event));
  const fields = new Map();
  for (const event of events) {
    for (const [name, value] of Object.entries(event.update)) {
      if (name === 'sessionUpdate') continue;
      const field = fields.get(name) ?? {
        name,
        types: new Set(),
        samples: [],
        promptIdentities: new Set(),
      };
      field.types.add(valueType(value));
      field.samples.push({ sequence: event.sequence, value });
      const binding = eventBindings.get(event);
      if (binding) {
        for (const identity of event.requestIdentities) {
          if (binding.identities.has(identity)) field.promptIdentities.add(identity);
        }
      }
      fields.set(name, field);
    }
  }

  const windows = promptWindows.map((window) => {
    const related = events.filter(
      (event) => event.sequence > window.promptSequence && event.sequence <= window.graceEndSequence,
    );
    return {
      label: window.label,
      promptSequence: window.promptSequence,
      terminalSequence: window.terminalSequence,
      graceEndSequence: window.graceEndSequence,
      beforeTerminal: related.filter((event) => event.sequence < window.terminalSequence).map((event) => event.sequence),
      afterTerminal: related.filter((event) => event.sequence > window.terminalSequence).map((event) => event.sequence),
      atTerminal: related.filter((event) => event.sequence === window.terminalSequence).map((event) => event.sequence),
    };
  });

  const normalizedFields = [...fields.values()].map((field) => {
    const boundSamples = field.samples.map((sample) => {
      const event = events.find((candidate) => candidate.sequence === sample.sequence);
      return { sample, binding: event ? eventBindings.get(event) : null };
    });
    const fieldWindows = windows.filter((window) =>
      boundSamples.some(({ binding }) => binding?.label === window.label),
    );
    const hasLateSample = boundSamples.some(
      ({ sample, binding }) =>
        binding && sample.sequence > binding.terminalSequence && sample.sequence <= binding.graceEndSequence,
    );
    return {
      name: field.name,
      types: [...field.types].sort(),
      sampleSequence: field.samples.slice(0, 12),
      // 数值增减无法排除 per-turn、delta 或 gauge；没有 typed 语义时必须保持 unknown。
      scope: 'unknown',
      resetBehavior: 'unknown',
      terminalCoverage:
        exactPromptBound && fieldWindows.length === windows.length && windows.length > 0
          ? 'all_observed_prompt_windows'
          : exactPromptBound
            ? 'partial_or_none'
            : 'unknown_unbound',
      lateBehavior: exactPromptBound
        ? hasLateSample
          ? 'observed_after_terminal'
          : 'not_observed_in_bounded_grace'
        : 'unknown_unbound',
      exactPromptIdentity: field.promptIdentities.size > 0 ? [...field.promptIdentities].sort() : null,
    };
  });
  return {
    eventCount: events.length,
    fields: normalizedFields,
    windows,
    exactPromptBound,
    publicConclusion: 'INCONCLUSIVE',
    tokenUsageCapability: false,
    reason:
      events.length === 0
        ? 'NO_REAL_USAGE_EVENT_REQUIRES_CALLER_PREREQUISITE_ASSESSMENT'
        : exactPromptBound
          ? 'FIELD_SCOPE_RESET_AND_TERMINAL_COVERAGE_REQUIRE_DCR'
          : 'USAGE_EVENTS_NOT_BOUND_TO_EXACT_PROMPT_IDENTITY',
  };
}

/** 将对象稳定写成带换行的 pretty JSON。 */
export function writeJson(path, value) {
  mkdirSync(resolve(path, '..'), { recursive: true });
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, { encoding: 'utf8', flag: 'wx', flush: true });
}
