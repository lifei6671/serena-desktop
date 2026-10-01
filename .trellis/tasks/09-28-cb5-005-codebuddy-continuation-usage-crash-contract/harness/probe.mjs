import { createInterface } from 'node:readline';
import { randomUUID } from 'node:crypto';
import {
  closeSync,
  existsSync,
  fsyncSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import {
  CONTINUATION_RECALL_PROMPT,
  JsonlWriter,
  RECOVERY_METHOD,
  TASK_TEMP_PREFIX,
  agentTextOf,
  analyzeUsage,
  assertOwnedTempWorkspace,
  cb5004InitializeParams,
  collectRequestIdentities,
  continuationAnswerFacts,
  hasExactRecoveredPromptResponse,
  hostAttempt4EvidenceRoot,
  hostAttempt5EvidenceRoot,
  hostEnvironmentProvenance,
  parseJsonRpcLine,
  removeOwnedTempWorkspace,
  sanitizeWire,
  sessionUpdateOf,
  sha256,
  statExists,
  usageUpdateOf,
  workspaceManifest,
  writeJson,
} from './lib.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const TASK_ROOT = resolve(HERE, '..');
let EVIDENCE_ROOT = join(TASK_ROOT, 'evidence');
const NODE_PATH = 'C:\\nvm4w\\nodejs\\node.exe';
const CODEBUDDY_SCRIPT =
  'C:\\Users\\lifei\\AppData\\Roaming\\npm\\node_modules\\@tencent-ai\\codebuddy-code\\bin\\codebuddy';
const CODEBUDDY_PACKAGE =
  'C:\\Users\\lifei\\AppData\\Roaming\\npm\\node_modules\\@tencent-ai\\codebuddy-code\\package.json';
const SDK_AGENT_SCHEMA =
  'C:\\Users\\lifei\\.cargo\\registry\\src\\index.crates.io-1949cf8c6b5b557f\\agent-client-protocol-schema-1.9.1\\src\\v1\\agent.rs';
const SDK_CLIENT_SCHEMA =
  'C:\\Users\\lifei\\.cargo\\registry\\src\\index.crates.io-1949cf8c6b5b557f\\agent-client-protocol-schema-1.9.1\\src\\v1\\client.rs';
const CB5_004_DIRECT_WIRE = resolve(
  TASK_ROOT,
  '..',
  '09-26-cb5-004-cancel-permission-contract',
  'evidence',
  'host-direct-codebuddy-proof',
  'wire.jsonl',
);
const CB5_004_EXACT_LAUNCHER_WIRE = resolve(
  TASK_ROOT,
  '..',
  '09-26-cb5-004-cancel-permission-contract',
  'evidence',
  'host-exact-launcher-proof',
  'wire.jsonl',
);
const CB5_004_DIRECT_WIRE_SHA256 = 'bf193e9d353357a1114293307107a4d5b6a1e671e6b2a6cb4c4fe45ae04b1b01';
const CB5_004_EXACT_LAUNCHER_WIRE_SHA256 = '5f2c4df063d214627f17e8a5bf77975d6b4e935d648a2127d0b2d16ec81b9e9f';
const CONTINUATION_MARKER = 'CB5_CONTINUATION_MARKER_20260928_V1';
const GRACE_MS = 3000;
const REQUEST_TIMEOUT_MS = 120000;
const CRASH_RECOVERY_WAIT_MS = 3000;
const MARKER_BYTES = Buffer.from('CB5_005_CRASH_MARKER_V1\n', 'utf8');
const PIPE_WRITE_TIMEOUT_MS = 10000;
const PROBE_WATCHDOG_MS = 1200000;
const ACTIVE_RUNTIMES = new Set();
const ACTIVE_WORKSPACES = new Set();
let PROBE_ABORTED = false;

/** 返回当前 ISO 时间，作为诊断字段而非排序 authority。 */
function nowIso() {
  return new Date().toISOString();
}

/** 等待指定毫秒，所有调用都使用固定上限。 */
function delay(ms) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
}

/** 计算文件 SHA256，不读取或输出文件正文。 */
function hashFile(path) {
  return sha256(readFileSync(path));
}

/** 冻结并记录真实 Probe 所依赖的 CB5-004 Host initialize 基线。 */
function writeInitializeBaselineEvidence() {
  const directWireSha256 = hashFile(CB5_004_DIRECT_WIRE);
  const exactLauncherWireSha256 = hashFile(CB5_004_EXACT_LAUNCHER_WIRE);
  if (directWireSha256 !== CB5_004_DIRECT_WIRE_SHA256) throw new Error('CB5_004_DIRECT_WIRE_HASH_MISMATCH');
  if (exactLauncherWireSha256 !== CB5_004_EXACT_LAUNCHER_WIRE_SHA256) {
    throw new Error('CB5_004_EXACT_LAUNCHER_WIRE_HASH_MISMATCH');
  }
  const params = cb5004InitializeParams();
  writeJson(join(EVIDENCE_ROOT, 'initialize-baseline.json'), {
    directWireSha256,
    exactLauncherWireSha256,
    params,
    paramsSha256: sha256(JSON.stringify(params)),
    equalityRequired: 'STRUCTURAL_EXACT',
  });
}

/** Host-only Probe 必须由 Serena command Host 显式开启，避免 Agent shell 误跑。 */
function assertHostProbeGate() {
  if (process.env.SERENA_CB5_HOST_PROBE !== '1') throw new Error('HOST_PROBE_GATE_REQUIRED');
  if (process.platform !== 'win32') throw new Error('HOST_PROBE_REQUIRES_WINDOWS');
}

/** 写入 allowlist-only 环境 provenance，不输出 PATH、env 值或凭据。 */
function writeHostEnvironmentProvenance() {
  writeJson(
    join(EVIDENCE_ROOT, 'environment-provenance.json'),
    hostEnvironmentProvenance(process.env, {
      platform: process.platform,
      nodeVersion: process.version,
      nodePath: NODE_PATH,
      nodeSha256: hashFile(NODE_PATH),
      scriptPath: CODEBUDDY_SCRIPT,
      scriptSha256: hashFile(CODEBUDDY_SCRIPT),
    }),
  );
}

/** 创建一次性临时 Workspace，并立即验证其归属边界。 */
function makeWorkspace(label) {
  const root = mkdtempSync(join(tmpdir(), `${TASK_TEMP_PREFIX}${label}-`));
  const workspace = assertOwnedTempWorkspace(root);
  ACTIVE_WORKSPACES.add(workspace);
  return workspace;
}

/** 删除当前运行创建的 Workspace，并同步移出 watchdog 跟踪集合。 */
function removeTrackedWorkspace(workspace) {
  const deleted = removeOwnedTempWorkspace(workspace);
  if (deleted) ACTIVE_WORKSPACES.delete(resolve(workspace));
  return deleted;
}

/** 用 create-new + fsync 创建真实 Probe 一次性 sentinel。 */
function createSentinel(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  const fd = openSync(path, 'wx');
  try {
    writeFileSync(fd, `${JSON.stringify(value)}\n`, { encoding: 'utf8' });
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
}

/** 清理本任务遗留的空临时目录；非空目录只记录，绝不删除。 */
function cleanupEmptyAttemptWorkspaces(outputName) {
  const entries = [];
  for (const item of readdirSync(tmpdir(), { withFileTypes: true })) {
    if (!item.isDirectory() || !item.name.startsWith(TASK_TEMP_PREFIX)) continue;
    const workspace = assertOwnedTempWorkspace(join(tmpdir(), item.name));
    const manifest = workspaceManifest(workspace);
    if (Object.keys(manifest).length > 0) {
      entries.push({ workspace, manifest, removed: false, reason: 'NON_EMPTY_PRESERVED' });
      continue;
    }
    entries.push({ workspace, manifest, removed: removeOwnedTempWorkspace(workspace), reason: 'EMPTY_TASK_WORKSPACE' });
  }
  const result = { generatedAt: nowIso(), entries };
  writeJson(join(TASK_ROOT, 'evidence', outputName), result);
  return result;
}

/** 为 promise 增加超时；超时不触发自动 retry。 */
function withTimeout(promise, ms, code) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(code)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

/** 从任意 permission params 中寻找唯一 typed reject_once option。 */
function findRejectOnce(value, found = []) {
  if (!value || typeof value !== 'object') return found;
  if (value.kind === 'reject_once' && typeof (value.optionId ?? value.id) === 'string') {
    found.push(value.optionId ?? value.id);
  }
  for (const child of Object.values(value)) {
    if (child && typeof child === 'object') findRejectOnce(child, found);
  }
  return found;
}

/** 管理一个独立 CodeBuddy ACP Runtime；PID 仅作为诊断，不作为安全证明。 */
class RpcRuntime {
  constructor({ label, workspace, recorder, processEvidence, usageEvents, usageWriter = null }) {
    this.label = label;
    this.workspace = workspace;
    this.recorder = recorder;
    this.processEvidence = processEvidence;
    this.usageEvents = usageEvents;
    this.usageWriter = usageWriter;
    this.pending = new Map();
    this.messages = [];
    this.stderrBytes = 0;
    this.stderrHash = null;
    this.stderrChunks = [];
    this.permissionDecisions = [];
    this.exit = null;
  }

  /** 启动 absolute node + installed CodeBuddy script + --acp。 */
  async start() {
    this.child = spawn(NODE_PATH, [CODEBUDDY_SCRIPT, '--acp'], {
      cwd: this.workspace,
      windowsHide: true,
      stdio: ['pipe', 'pipe', 'pipe'],
      shell: false,
      env: process.env,
    });
    this.startedAt = nowIso();
    this.exitPromise = new Promise((resolveExit) => {
      this.child.once('exit', (code, signal) => {
        this.exit = { code, signal, at: nowIso() };
        for (const pending of this.pending.values()) {
          pending.reject(new Error(`RUNTIME_EXITED:${code}:${signal}`));
        }
        this.pending.clear();
        resolveExit(this.exit);
      });
    });
    this.child.stderr.on('data', (chunk) => {
      this.stderrBytes += chunk.length;
      if (this.stderrChunks.reduce((sum, item) => sum + item.length, 0) < 8192) {
        this.stderrChunks.push(Buffer.from(chunk).subarray(0, 8192 - this.stderrBytes + chunk.length));
      }
    });
    const lines = createInterface({ input: this.child.stdout, crlfDelay: Infinity });
    this.lines = lines;
    lines.on('line', (line) => {
      try {
        if (Buffer.byteLength(line, 'utf8') > 1024 * 1024) throw new Error('FRAME_TOO_LARGE');
        this.handleIncoming(parseJsonRpcLine(line));
      } catch (error) {
        this.protocolError = String(error?.message ?? error);
        void this.crashKill('protocol_error');
      }
    });
    await new Promise((resolveSpawn, rejectSpawn) => {
      this.child.once('spawn', resolveSpawn);
      this.child.once('error', rejectSpawn);
    });
    ACTIVE_RUNTIMES.add(this);
    return this;
  }

  /** 记录一条 sanitized wire 并返回全局 sequence。 */
  record(direction, message, correlatedMethod = null) {
    const sequence = ++this.recorder.sequence;
    const row = {
      sequence,
      at: nowIso(),
      runtime: this.label,
      direction,
      correlatedMethod,
      message: sanitizeWire(message, this.workspace),
    };
    this.recorder.writer.append(row);
    this.messages.push({ sequence, direction, correlatedMethod, message });
    return sequence;
  }

  /** 处理 Provider 发来的 response、notification 或 client request。 */
  handleIncoming(message) {
    const pending = message.id !== undefined ? this.pending.get(String(message.id)) : null;
    const sequence = this.record('provider_to_client', message, pending?.method ?? null);
    const usage = usageUpdateOf(message);
    if (usage) {
      const event = {
        sequence,
        runtime: this.label,
        sessionId: message?.params?.sessionId ?? null,
        update: usage,
        requestIdentities: [...collectRequestIdentities(message)].sort(),
      };
      this.usageEvents.push(event);
      this.usageWriter?.append({ kind: 'usage_update', ...sanitizeWire(event, this.workspace) });
    }
    if (message.method === 'session/request_permission' && message.id !== undefined) {
      void this.rejectPermission(message);
      return;
    }
    if (message.method && message.id !== undefined) {
      void this.sendResponse(message.id, null, { code: -32601, message: 'TASK_LOCAL_CLIENT_METHOD_UNSUPPORTED' });
      return;
    }
    if (!pending) return;
    this.pending.delete(String(message.id));
    if (message.error) pending.reject(Object.assign(new Error('JSON_RPC_ERROR'), { rpcError: message.error }));
    else pending.resolve({ result: message.result, responseSequence: sequence, rpcId: message.id });
  }

  /** 对 permission request 只选择 typed reject_once；缺失时 fail closed。 */
  async rejectPermission(message) {
    const options = [...new Set(findRejectOnce(message.params))];
    if (options.length !== 1) {
      await this.sendResponse(message.id, null, { code: -32000, message: 'REJECT_ONCE_NOT_UNIQUE' });
      this.permissionDecisions.push({ requestId: message.id, outcome: 'fail_closed', options });
      return;
    }
    const optionId = options[0];
    await this.sendResponse(message.id, { outcome: { outcome: 'selected', optionId } }, null);
    this.permissionDecisions.push({ requestId: message.id, outcome: 'reject_once', optionId });
  }

  /** 写出 JSON-RPC response，并等待 pipe write callback。 */
  async sendResponse(id, result, error) {
    const message = { jsonrpc: '2.0', id, ...(error ? { error } : { result }) };
    this.record('client_to_provider', message, 'client/response');
    await this.writeLine(message);
  }

  /** 写出单帧 JSONL，并以 write callback 作为本 harness 的 flush 边界。 */
  async writeLine(message) {
    if (!this.child?.stdin?.writable) throw new Error('ACP_STDIN_NOT_WRITABLE');
    const line = `${JSON.stringify(message)}\n`;
    if (Buffer.byteLength(line, 'utf8') > 1024 * 1024) throw new Error('OUTBOUND_FRAME_TOO_LARGE');
    await withTimeout(
      new Promise((resolveWrite, rejectWrite) => {
        this.child.stdin.write(line, 'utf8', (error) => (error ? rejectWrite(error) : resolveWrite()));
      }),
      PIPE_WRITE_TIMEOUT_MS,
      'PIPE_WRITE_TIMEOUT',
    );
  }

  /** 启动一个 request，返回可在 crash 窗口中独立观察的 write/response handle。 */
  async startRequest(method, params) {
    const id = randomUUID();
    let resolveResponse;
    let rejectResponse;
    const promise = new Promise((resolvePending, rejectPending) => {
      resolveResponse = resolvePending;
      rejectResponse = rejectPending;
    });
    this.pending.set(id, { method, resolve: resolveResponse, reject: rejectResponse });
    const message = { jsonrpc: '2.0', id, method, params };
    const requestSequence = this.record('client_to_provider', message, method);
    try {
      await this.writeLine(message);
    } catch (error) {
      this.pending.delete(id);
      rejectResponse(error);
      throw error;
    }
    return { id, method, requestSequence, writeCallbackCompleted: true, promise };
  }

  /** 发送 request 并等待 bounded correlated response。 */
  async request(method, params, timeoutMs = REQUEST_TIMEOUT_MS) {
    const handle = await this.startRequest(method, params);
    try {
      return { ...handle, ...(await withTimeout(handle.promise, timeoutMs, `REQUEST_TIMEOUT:${method}`)) };
    } catch (error) {
      this.pending.delete(handle.id);
      throw Object.assign(error, { method, rpcId: handle.id, requestSequence: handle.requestSequence });
    }
  }

  /** 逐字段复用 CB5-004 Host fresh-session PASS 的 initialize params。 */
  async initialize() {
    const params = cb5004InitializeParams();
    this.initializeParamsSha256 = sha256(JSON.stringify(params));
    return this.request('initialize', params);
  }

  /** 创建 fresh session，cwd 只使用已验证临时 Workspace。 */
  async newSession() {
    return this.request('session/new', { cwd: this.workspace, mcpServers: [] });
  }

  /** 只调用静态冻结的唯一 session/load method。 */
  async loadSession(sessionId) {
    return this.request(RECOVERY_METHOD, { sessionId, cwd: this.workspace, mcpServers: [] });
  }

  /** 发送固定文本 prompt；正文只保留在内存和 ACP pipe。 */
  async prompt(sessionId, text, timeoutMs = REQUEST_TIMEOUT_MS) {
    return this.request('session/prompt', {
      sessionId,
      prompt: [{ type: 'text', text }],
    }, timeoutMs);
  }

  /** 汇总指定 sequence 窗口的 assistant 可见正文。 */
  answerBetween(startSequence, endSequence = Number.MAX_SAFE_INTEGER) {
    return this.messages
      .filter((row) => row.sequence > startSequence && row.sequence <= endSequence)
      .map((row) => agentTextOf(row.message))
      .join('');
  }

  /** 正常结束 stdin；超时后仅做 task-local tree cleanup。 */
  async gracefulStop(reason = 'normal_cleanup') {
    if (!this.exit && this.child.stdin.writable) this.child.stdin.end();
    try {
      await withTimeout(this.exitPromise, 5000, 'GRACEFUL_EXIT_TIMEOUT');
    } catch {
      await this.crashKill(reason);
    }
    this.closeLocalHandles();
    return this.captureProcessEvidence(reason, false);
  }

  /** 用 taskkill 终止 direct provider PID/tree；不声称 Windows Job 或 Host proof。 */
  async crashKill(reason) {
    if (!this.child || this.exit) return this.captureProcessEvidence(reason, true);
    const taskkill = spawnSync('taskkill.exe', ['/PID', String(this.child.pid), '/T', '/F'], {
      windowsHide: true,
      shell: false,
      timeout: 10000,
      encoding: 'utf8',
    });
    this.taskkill = {
      status: taskkill.status,
      signal: taskkill.signal,
      errorCode: taskkill.error?.code ?? null,
      stdoutSha256: sha256(taskkill.stdout ?? ''),
      stderrSha256: sha256(taskkill.stderr ?? ''),
    };
    try {
      await withTimeout(this.exitPromise, 10000, 'TASKKILL_REAP_TIMEOUT');
    } catch (error) {
      this.reapError = String(error?.message ?? error);
      try {
        this.child.kill('SIGKILL');
        await withTimeout(this.exitPromise, 5000, 'DIRECT_KILL_REAP_TIMEOUT');
      } catch (fallbackError) {
        this.directKillError = String(fallbackError?.message ?? fallbackError);
      }
    }
    this.closeLocalHandles();
    return this.captureProcessEvidence(reason, true);
  }

  /** 关闭本 harness 持有的 readline/stdio handle，避免 descendant 继承 pipe 导致 event loop 悬挂。 */
  closeLocalHandles() {
    try {
      this.lines?.close();
    } catch {}
    for (const stream of [this.child?.stdin, this.child?.stdout, this.child?.stderr]) {
      try {
        stream?.destroy();
      } catch {}
    }
    ACTIVE_RUNTIMES.delete(this);
  }

  /** 记录 PID/exit/cleanup 诊断，不把它投影为 Runtime safety authority。 */
  captureProcessEvidence(reason, abrupt) {
    if (this.processCaptured) return this.processCaptured;
    const stderrSample = Buffer.concat(this.stderrChunks);
    this.stderrHash = sha256(stderrSample);
    this.processCaptured = {
      runtime: this.label,
      pid: this.child?.pid ?? null,
      startedAt: this.startedAt,
      exit: this.exit,
      abrupt,
      reason,
      taskkill: this.taskkill ?? null,
      directChildReaped: Boolean(this.exit),
      stderrBytes: this.stderrBytes,
      stderrPrefixSha256: this.stderrHash,
      stderrContentSaved: false,
      protocolError: this.protocolError ?? null,
      reapError: this.reapError ?? null,
      directKillError: this.directKillError ?? null,
      initializeParamsSha256: this.initializeParamsSha256 ?? null,
      windowsJobAtCreationProven: false,
      provesOldRuntimeTerminatedForProduction: false,
    };
    this.processEvidence.push(this.processCaptured);
    return this.processCaptured;
  }
}

/** 运行 prompt 并捕获 terminal、answer hash 与 bounded late usage window。 */
async function runPrompt(runtime, usageWriter, windows, label, sessionId, text) {
  const request = await runtime.startRequest('session/prompt', {
    sessionId,
    prompt: [{ type: 'text', text }],
  });
  usageWriter.append({
    kind: 'prompt_request',
    label,
    runtime: runtime.label,
    sequence: request.requestSequence,
    rpcId: request.id,
    sessionId,
  });
  const terminal = await withTimeout(request.promise, REQUEST_TIMEOUT_MS, `PROMPT_TIMEOUT:${label}`);
  const terminalSequence = terminal.responseSequence;
  const answer = runtime.answerBetween(request.requestSequence, terminalSequence);
  usageWriter.append({
    kind: 'prompt_terminal',
    label,
    runtime: runtime.label,
    sequence: terminalSequence,
    rpcId: request.id,
    sessionId,
    stopReason: terminal.result?.stopReason ?? null,
    answerSha256: sha256(answer),
    answerUtf8Bytes: Buffer.byteLength(answer, 'utf8'),
  });
  await delay(GRACE_MS);
  const providerRequestIdentities = new Set(collectRequestIdentities(terminal.result));
  for (const row of runtime.messages) {
    if (
      row.sequence > request.requestSequence &&
      row.sequence <= terminalSequence &&
      !usageUpdateOf(row.message)
    ) {
      collectRequestIdentities(row.message, providerRequestIdentities);
    }
  }
  const window = {
    label,
    rpcId: request.id,
    sessionId,
    providerRequestIdentities: [...providerRequestIdentities].sort(),
    promptSequence: request.requestSequence,
    terminalSequence,
    graceEndSequence: runtime.recorder.sequence,
  };
  windows.push(window);
  return { request, terminal, answer, window };
}

/** 静态检查真实安装 provenance、pinned schema 和唯一 recovery method。 */
function runInspection() {
  const packageJson = JSON.parse(readFileSync(CODEBUDDY_PACKAGE, 'utf8'));
  const agentSchema = readFileSync(SDK_AGENT_SCHEMA, 'utf8');
  const clientSchema = readFileSync(SDK_CLIENT_SCHEMA, 'utf8');
  const bundleRoot = resolve(CODEBUDDY_PACKAGE, '..');
  const bundleFiles = [
    join(bundleRoot, 'dist', 'codebuddy.js'),
    join(bundleRoot, 'dist', 'codebuddy-headless.js'),
    join(bundleRoot, 'dist', 'codebuddy-lite-wb.mjs'),
  ];
  const bundleCounts = bundleFiles.map((path) => {
    const text = readFileSync(path, 'utf8');
    return {
      path,
      sha256: sha256(text),
      sessionLoadOccurrences: text.split('session/load').length - 1,
      sessionResumeOccurrences: text.split('session/resume').length - 1,
      loadSessionCapabilityOccurrences: text.split('loadSession').length - 1,
    };
  });
  const inspection = {
    generatedAt: nowIso(),
    conclusion: {
      uniqueRecoveryMethod: RECOVERY_METHOD,
      reason: 'ACP_V1_EXPLICIT_LOAD_SESSION_CAPABILITY_AND_CODEBUDDY_OWN_CONTINUE_PATH',
      fallbackMethods: [],
      sessionResumeWillBeProbed: false,
    },
    provenance: {
      packageName: packageJson.name,
      packageVersion: packageJson.version,
      nodePath: NODE_PATH,
      nodeSha256: hashFile(NODE_PATH),
      scriptPath: CODEBUDDY_SCRIPT,
      scriptSha256: hashFile(CODEBUDDY_SCRIPT),
      packageJsonPath: CODEBUDDY_PACKAGE,
      packageJsonSha256: hashFile(CODEBUDDY_PACKAGE),
      argv: [NODE_PATH, CODEBUDDY_SCRIPT, '--acp'],
    },
    sdk: {
      agentSchemaPath: SDK_AGENT_SCHEMA,
      agentSchemaSha256: hashFile(SDK_AGENT_SCHEMA),
      clientSchemaPath: SDK_CLIENT_SCHEMA,
      clientSchemaSha256: hashFile(SDK_CLIENT_SCHEMA),
      hasTopLevelLoadSessionCapability: agentSchema.includes('pub load_session: bool'),
      hasSessionLoadMethod: agentSchema.includes('SESSION_LOAD_METHOD_NAME: &str = "session/load"'),
      loadReplaysHistoryContract: agentSchema.includes('unlike `session/load`'),
      hasTypedUsageUpdate: clientSchema.includes('pub struct UsageUpdate'),
    },
    bundleCounts,
  };
  writeJson(join(EVIDENCE_ROOT, 'capability-schema-inspection.json'), inspection);
  return inspection;
}

/** 执行 combined continuation + usage real Probe。 */
async function runContinuationAndUsage(processEvidence) {
  const workspace = makeWorkspace('continuation');
  const sentinel = join(EVIDENCE_ROOT, 'continuation.attempt-started.json');
  createSentinel(sentinel, { startedAt: nowIso(), workspace, recoveryMethod: RECOVERY_METHOD });
  const before = workspaceManifest(workspace);
  const writer = new JsonlWriter(join(EVIDENCE_ROOT, 'continuation.jsonl'));
  const usageWriter = new JsonlWriter(join(EVIDENCE_ROOT, 'usage.jsonl'));
  const recorder = { writer, sequence: 0 };
  const usageEvents = [];
  const windows = [];
  let r1;
  let r2;
  let result;
  let initializeSucceeded = false;
  let sessionNewSucceeded = false;
  try {
    r1 = await new RpcRuntime({
      label: 'continuation-r1',
      workspace,
      recorder,
      processEvidence,
      usageEvents,
      usageWriter,
    }).start();
    const initializeR1 = await r1.initialize();
    initializeSucceeded = true;
    const sessionNew = await r1.newSession();
    const sessionId = sessionNew.result?.sessionId;
    if (typeof sessionId !== 'string' || !sessionId) throw new Error('SESSION_NEW_MISSING_SESSION_ID');
    sessionNewSucceeded = true;

    const p1 = await runPrompt(
      r1,
      usageWriter,
      windows,
      'P1',
      sessionId,
      `This is a contract fixture. Remember the literal ${CONTINUATION_MARKER}. Reply exactly P1_OK. Do not use tools or modify files.`,
    );
    const p2 = await runPrompt(
      r1,
      usageWriter,
      windows,
      'P2',
      sessionId,
      'Reply exactly P2_OK. Do not use tools or modify files.',
    );
    const afterR1Prompts = workspaceManifest(workspace);
    const r1Process = await r1.gracefulStop('continuation_r1_complete');
    if (!r1Process.directChildReaped) throw new Error('R1_NOT_REAPED_BEFORE_R2');

    r2 = await new RpcRuntime({
      label: 'continuation-r2',
      workspace,
      recorder,
      processEvidence,
      usageEvents,
      usageWriter,
    }).start();
    const initializeR2 = await r2.initialize();
    const loadAdvertised = initializeR2.result?.agentCapabilities?.loadSession === true;
    if (!loadAdvertised) throw Object.assign(new Error('LOAD_SESSION_CAPABILITY_NOT_ADVERTISED'), { explicitUnsupported: true });
    const load = await r2.loadSession(sessionId);
    await delay(1000);
    const loadReplayEndSequence = r2.recorder.sequence;
    const loadReplayRows = r2.messages.filter(
      (row) =>
        row.sequence > load.requestSequence &&
        row.sequence <= loadReplayEndSequence &&
        row.message?.method === 'session/update',
    );
    const replayAllExactSession =
      loadReplayRows.length > 0 && loadReplayRows.every((row) => row.message?.params?.sessionId === sessionId);
    const replayedText = loadReplayRows.map((row) => agentTextOf(row.message)).join('');
    const exactTargetTerminalRecovered = hasExactRecoveredPromptResponse(
      r2.messages,
      p2.request.id,
      load.requestSequence,
    );
    const p3 = await runPrompt(
      r2,
      usageWriter,
      windows,
      'P3_AFTER_RESTART',
      sessionId,
      'Without restating any earlier prompt, reply with one line containing the remembered literal, then a vertical bar, then the absolute current working directory. Do not use tools or modify files.',
    );
    const expectedP3 = `${CONTINUATION_MARKER}|${workspace}`;
    const p3Matched = p3.answer.trim() === expectedP3;
    const loadResponseSessionId = load.result?.sessionId ?? null;
    const loadResponseSessionStatus =
      loadResponseSessionId === null
        ? 'NOT_PRESENT'
        : loadResponseSessionId === sessionId
          ? 'EXACT_S1'
          : 'MISMATCH';
    const wrongSessionUpdates = [...r1.messages, ...r2.messages]
      .filter((row) => row.message?.method === 'session/update')
      .filter((row) => row.message?.params?.sessionId !== sessionId)
      .map((row) => row.sequence);
    const afterR2 = workspaceManifest(workspace);
    const r2Process = await r2.gracefulStop('continuation_r2_complete');
    const continuationStatus =
      r1Process.directChildReaped &&
      r2Process.directChildReaped &&
      loadAdvertised &&
      loadResponseSessionStatus !== 'MISMATCH' &&
      (loadReplayRows.length === 0 || replayAllExactSession) &&
      p3.terminal.result?.stopReason === 'end_turn' &&
      p3Matched &&
      wrongSessionUpdates.length === 0 &&
      JSON.stringify(before) === JSON.stringify(afterR1Prompts) &&
      JSON.stringify(before) === JSON.stringify(afterR2)
        ? 'PROVEN_SUPPORTED'
        : 'INCONCLUSIVE';
    const usage = analyzeUsage(usageEvents, windows);
    result = {
      status: 'COMPLETED',
      freshSessionPrerequisite: {
        status: 'PASSED',
        initializeExactCb5004Params: true,
        initializeSucceeded,
        sessionNewSucceeded,
      },
      continuation: {
        conclusion: continuationStatus,
        recoveryMethod: RECOVERY_METHOD,
        params: { sessionId, cwd: workspace, mcpServers: [] },
        initializeR1: sanitizeWire(initializeR1.result, workspace),
        initializeR2: sanitizeWire(initializeR2.result, workspace),
        loadResponse: sanitizeWire(load.result, workspace),
        sessionId,
        r1ExitedBeforeR2: r1Process.directChildReaped,
        loadAdvertised,
        replayUpdateCountBeforeP3: loadReplayRows.length,
        replayUpdateTypes: loadReplayRows.map((row) => sessionUpdateOf(row.message)?.sessionUpdate ?? 'unknown'),
        loadReplayEndSequence,
        loadResponseSessionStatus,
        replayAllExactSession,
        p1Terminal: { rpcId: p1.request.id, stopReason: p1.terminal.result?.stopReason ?? null },
        p2Terminal: { rpcId: p2.request.id, stopReason: p2.terminal.result?.stopReason ?? null },
        p3Terminal: { rpcId: p3.request.id, stopReason: p3.terminal.result?.stopReason ?? null },
        p3ExpectedSha256: sha256(expectedP3),
        p3AnswerSha256: sha256(p3.answer.trim()),
        p3Matched,
        hostReplayedP1: false,
        wrongSessionUpdates,
        cwdLineage: p3Matched ? 'EXACT_MARKER_AND_CWD_RETURNED' : 'NOT_PROVEN',
        sessionLineage:
          p3Matched &&
          wrongSessionUpdates.length === 0 &&
          loadResponseSessionStatus !== 'MISMATCH' &&
          (loadReplayRows.length === 0 || replayAllExactSession)
          ? 'EXACT_S1_AND_CWD_AFTER_LOAD'
          : 'NOT_PROVEN',
        resultRecovery: {
          targetPrompt: 'P2',
          exactTargetTerminalRecovered,
          replayedTextSha256: replayedText ? sha256(replayedText) : null,
          replayedTextUtf8Bytes: Buffer.byteLength(replayedText, 'utf8'),
          recoveryStrength: exactTargetTerminalRecovered
            ? 'exact-result'
            : replayedText
              ? 'session+partial-result'
              : 'session',
          resultCompleteness: exactTargetTerminalRecovered ? 'complete' : replayedText ? 'partial' : 'unknown',
        },
      },
      usage,
      workspace: { before, afterR1Prompts, afterR2 },
      permissions: [...r1.permissionDecisions, ...r2.permissionDecisions],
      boundaries: {
        windowsJobAtCreationProven: false,
        r2ProvesR1Termination: false,
        productionCapabilityChanged: false,
      },
    };
  } catch (error) {
    if (r1 && !r1.processCaptured) await r1.gracefulStop('continuation_error_cleanup');
    if (r2 && !r2.processCaptured) await r2.gracefulStop('continuation_error_cleanup');
    result = {
      status: 'FAILED',
      freshSessionPrerequisite: {
        status: sessionNewSucceeded ? 'PASSED' : 'FAILED',
        initializeExactCb5004Params: true,
        initializeSucceeded,
        sessionNewSucceeded,
      },
      continuation: {
        conclusion: error?.explicitUnsupported ? 'EXPLICITLY_UNSUPPORTED' : 'INCONCLUSIVE',
        recoveryMethod: RECOVERY_METHOD,
      },
      usage: analyzeUsage(usageEvents, windows),
      error: {
        message: String(error?.message ?? error),
        rpcCode: error?.rpcError?.code ?? null,
        rpcMessageSha256: error?.rpcError?.message ? sha256(String(error.rpcError.message)) : null,
      },
    };
  }
  result.workspaceBeforeManifest = before;
  const finalManifest = statExists(workspace) ? workspaceManifest(workspace) : null;
  let workspaceDeleted = false;
  try {
    workspaceDeleted = statExists(workspace) ? removeTrackedWorkspace(workspace) : true;
    if (workspaceDeleted) ACTIVE_WORKSPACES.delete(resolve(workspace));
  } catch (error) {
    result.cleanupError = String(error?.message ?? error);
  }
  result.workspaceFinalManifest = finalManifest;
  result.workspaceDeleted = workspaceDeleted;
  writeJson(join(EVIDENCE_ROOT, 'continuation-result.json'), result);
  writeJson(join(EVIDENCE_ROOT, 'usage-analysis.json'), result.usage);
  return result;
}

/** 执行 attempt-5 的单一 Continuation lineage Probe，不运行 Usage 分析或 crash 窗口。 */
async function runContinuationLineageProbe(processEvidence) {
  const workspace = makeWorkspace('continuation-attempt5');
  const sentinel = join(EVIDENCE_ROOT, 'continuation.attempt-started.json');
  createSentinel(sentinel, { startedAt: nowIso(), workspace, recoveryMethod: RECOVERY_METHOD });
  const before = workspaceManifest(workspace);
  const wireWriter = new JsonlWriter(join(EVIDENCE_ROOT, 'continuation.jsonl'));
  const eventWriter = new JsonlWriter(join(EVIDENCE_ROOT, 'continuation-events.jsonl'));
  const recorder = { writer: wireWriter, sequence: 0 };
  const promptWindows = [];
  const p3PromptFacts = continuationAnswerFacts(CONTINUATION_RECALL_PROMPT, CONTINUATION_MARKER, workspace);
  let r1;
  let r2;
  let result;
  let initializeSucceeded = false;
  let sessionNewSucceeded = false;
  try {
    if (p3PromptFacts.markerPresent || p3PromptFacts.cwdPresent) {
      throw new Error('P3_PROMPT_DISCLOSES_LINEAGE_FIXTURE');
    }
    r1 = await new RpcRuntime({
      label: 'continuation-attempt5-r1',
      workspace,
      recorder,
      processEvidence,
      usageEvents: [],
    }).start();
    const initializeR1 = await r1.initialize();
    initializeSucceeded = true;
    const sessionNew = await r1.newSession();
    const sessionId = sessionNew.result?.sessionId;
    if (typeof sessionId !== 'string' || !sessionId) throw new Error('SESSION_NEW_MISSING_SESSION_ID');
    sessionNewSucceeded = true;

    const p1 = await runPrompt(
      r1,
      eventWriter,
      promptWindows,
      'P1_REMEMBER_MARKER',
      sessionId,
      `This is a contract fixture. Remember the literal ${CONTINUATION_MARKER}. Reply exactly P1_OK. Do not use tools or modify files.`,
    );
    const afterR1 = workspaceManifest(workspace);
    const r1Process = await r1.gracefulStop('continuation_attempt5_r1_complete');
    if (!r1Process.directChildReaped) throw new Error('R1_NOT_REAPED_BEFORE_R2');

    r2 = await new RpcRuntime({
      label: 'continuation-attempt5-r2',
      workspace,
      recorder,
      processEvidence,
      usageEvents: [],
    }).start();
    const initializeR2 = await r2.initialize();
    const loadAdvertised = initializeR2.result?.agentCapabilities?.loadSession === true;
    if (!loadAdvertised) {
      throw Object.assign(new Error('LOAD_SESSION_CAPABILITY_NOT_ADVERTISED'), { explicitUnsupported: true });
    }
    const load = await r2.loadSession(sessionId);
    await delay(1000);
    const loadReplayEndSequence = r2.recorder.sequence;
    const loadReplayRows = r2.messages.filter(
      (row) =>
        row.sequence > load.requestSequence &&
        row.sequence <= loadReplayEndSequence &&
        row.message?.method === 'session/update',
    );
    const replayExists = loadReplayRows.length > 0;
    const replayAllExactSession =
      replayExists && loadReplayRows.every((row) => row.message?.params?.sessionId === sessionId);
    const replayedText = loadReplayRows.map((row) => agentTextOf(row.message)).join('');
    const exactTargetTerminalRecovered = hasExactRecoveredPromptResponse(
      r2.messages,
      p1.request.id,
      load.requestSequence,
    );

    const p3 = await runPrompt(
      r2,
      eventWriter,
      promptWindows,
      'P3_AFTER_RESTART',
      sessionId,
      CONTINUATION_RECALL_PROMPT,
    );
    const p3Semantic = continuationAnswerFacts(p3.answer, CONTINUATION_MARKER, workspace);
    const r2PromptTexts = r2.messages
      .filter(
        (row) =>
          row.direction === 'client_to_provider' &&
          row.message?.method === 'session/prompt',
      )
      .flatMap((row) => row.message.params?.prompt ?? [])
      .filter((item) => item?.type === 'text' && typeof item.text === 'string')
      .map((item) => item.text);
    const hostReplayedP1 = r2PromptTexts.some((text) => text.includes(CONTINUATION_MARKER));
    const wrongSessionUpdates = r2.messages
      .filter((row) => row.message?.method === 'session/update')
      .filter((row) => row.message?.params?.sessionId !== sessionId)
      .map((row) => row.sequence);
    const loadResponseSessionId = load.result?.sessionId ?? null;
    const loadResponseSessionStatus =
      loadResponseSessionId === null
        ? 'NOT_PRESENT'
        : loadResponseSessionId === sessionId
          ? 'EXACT_S1'
          : 'MISMATCH';
    const afterR2 = workspaceManifest(workspace);
    const r2Process = await r2.gracefulStop('continuation_attempt5_r2_complete');
    const workspaceUnchanged =
      JSON.stringify(before) === JSON.stringify(afterR1) &&
      JSON.stringify(before) === JSON.stringify(afterR2);
    const continuationStatus =
      r1Process.directChildReaped &&
      p1.terminal.result?.stopReason === 'end_turn' &&
      loadAdvertised &&
      loadResponseSessionStatus !== 'MISMATCH' &&
      replayExists &&
      replayAllExactSession &&
      wrongSessionUpdates.length === 0 &&
      p3.terminal.result?.stopReason === 'end_turn' &&
      p3Semantic.markerPresent &&
      p3Semantic.cwdPresent &&
      !hostReplayedP1 &&
      workspaceUnchanged
        ? 'PROVEN_SUPPORTED'
        : 'INCONCLUSIVE';
    result = {
      status: 'COMPLETED',
      scope: 'CONTINUATION_ONLY',
      freshSessionPrerequisite: {
        status: 'PASSED',
        initializeExactCb5004Params: true,
        initializeSucceeded,
        sessionNewSucceeded,
      },
      continuation: {
        conclusion: continuationStatus,
        recoveryMethod: RECOVERY_METHOD,
        params: { sessionId, cwd: workspace, mcpServers: [] },
        initializeR1: sanitizeWire(initializeR1.result, workspace),
        initializeR2: sanitizeWire(initializeR2.result, workspace),
        loadResponse: sanitizeWire(load.result, workspace),
        sessionId,
        r1ExitedBeforeR2: r1Process.directChildReaped,
        r2DirectChildReaped: r2Process.directChildReaped,
        loadAdvertised,
        replayUpdateCountBeforeP3: loadReplayRows.length,
        replayUpdateTypes: loadReplayRows.map(
          (row) => sessionUpdateOf(row.message)?.sessionUpdate ?? 'unknown',
        ),
        loadReplayEndSequence,
        loadResponseSessionStatus,
        replayExists,
        replayAllExactSession,
        wrongSessionUpdates,
        p1Terminal: { rpcId: p1.request.id, stopReason: p1.terminal.result?.stopReason ?? null },
        p3Terminal: { rpcId: p3.request.id, stopReason: p3.terminal.result?.stopReason ?? null },
        p3Semantic,
        p3PromptContainsMarker: p3PromptFacts.markerPresent,
        p3PromptContainsCwd: p3PromptFacts.cwdPresent,
        hostReplayedP1,
        workspaceUnchanged,
        sessionLineage:
          continuationStatus === 'PROVEN_SUPPORTED'
            ? 'EXACT_S1_HISTORY_REPLAY_AND_MARKER'
            : 'NOT_PROVEN',
        cwdLineage:
          continuationStatus === 'PROVEN_SUPPORTED'
            ? 'EXACT_WORKSPACE_PATH_OBSERVED_IN_P3'
            : 'NOT_PROVEN',
        resultRecovery: {
          targetPrompt: 'P1',
          exactTargetTerminalRecovered,
          replayedTextSha256: replayedText ? sha256(replayedText) : null,
          replayedTextUtf8Bytes: Buffer.byteLength(replayedText, 'utf8'),
          recoveryStrength: exactTargetTerminalRecovered
            ? 'exact-result'
            : replayedText
              ? 'session+partial-result'
              : 'session',
          resultCompleteness: exactTargetTerminalRecovered ? 'complete' : replayedText ? 'partial' : 'unknown',
        },
      },
      workspace: { before, afterR1, afterR2 },
      permissions: [...r1.permissionDecisions, ...r2.permissionDecisions],
      boundaries: {
        usageProbeRun: false,
        crashProbeRun: false,
        windowsJobAtCreationProven: false,
        r2ProvesR1Termination: false,
        productionCapabilityChanged: false,
      },
    };
  } catch (error) {
    if (r1 && !r1.processCaptured) await r1.gracefulStop('continuation_attempt5_error_cleanup');
    if (r2 && !r2.processCaptured) await r2.gracefulStop('continuation_attempt5_error_cleanup');
    result = {
      status: 'FAILED',
      scope: 'CONTINUATION_ONLY',
      freshSessionPrerequisite: {
        status: sessionNewSucceeded ? 'PASSED' : 'FAILED',
        initializeExactCb5004Params: true,
        initializeSucceeded,
        sessionNewSucceeded,
      },
      continuation: {
        conclusion: error?.explicitUnsupported ? 'EXPLICITLY_UNSUPPORTED' : 'INCONCLUSIVE',
        recoveryMethod: RECOVERY_METHOD,
      },
      error: {
        message: String(error?.message ?? error),
        rpcCode: error?.rpcError?.code ?? null,
        rpcMessageSha256: error?.rpcError?.message ? sha256(String(error.rpcError.message)) : null,
      },
      boundaries: { usageProbeRun: false, crashProbeRun: false },
    };
  }
  result.workspaceBeforeManifest = before;
  const finalManifest = statExists(workspace) ? workspaceManifest(workspace) : null;
  try {
    result.workspaceDeleted = statExists(workspace) ? removeTrackedWorkspace(workspace) : true;
    if (result.workspaceDeleted) ACTIVE_WORKSPACES.delete(resolve(workspace));
  } catch (error) {
    result.workspaceDeleted = false;
    result.cleanupError = String(error?.message ?? error);
  }
  result.workspaceFinalManifest = finalManifest;
  writeJson(join(EVIDENCE_ROOT, 'continuation-result.json'), result);
  return result;
}

/** 等待 marker 或 prompt terminal，先到者决定 side-effect crash 窗口是否成立。 */
async function waitForMarkerOrTerminal(markerPath, promptState, timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (existsSync(markerPath) && readFileSync(markerPath).equals(MARKER_BYTES)) return 'marker';
    if (promptState.settled) return 'terminal';
    await delay(100);
  }
  return 'timeout';
}

/** 执行一个独立 crash/restart 窗口。 */
async function runCrashWindow(kind, processEvidence) {
  const workspace = makeWorkspace(`crash-${kind}`);
  const crashDir = join(EVIDENCE_ROOT, 'crash');
  createSentinel(join(crashDir, `${kind}.attempt-started.json`), {
    startedAt: nowIso(),
    workspace,
    recoveryMethod: RECOVERY_METHOD,
  });
  const writer = new JsonlWriter(join(crashDir, `${kind}.jsonl`));
  const recorder = { writer, sequence: 0 };
  const usageEvents = [];
  const before = workspaceManifest(workspace);
  let r1;
  let r2;
  let sessionId = null;
  let prompt = null;
  let promptState = { settled: false, terminal: null, error: null };
  let windowObserved = false;
  let markerObserved = false;
  let result;
  try {
    r1 = await new RpcRuntime({
      label: `${kind}-r1`,
      workspace,
      recorder,
      processEvidence,
      usageEvents,
    }).start();
    const initializeR1 = await r1.initialize();
    const created = await r1.newSession();
    sessionId = created.result?.sessionId;
    if (typeof sessionId !== 'string' || !sessionId) throw new Error('CRASH_SESSION_ID_MISSING');

    if (kind === 'after_session_new_before_prompt') {
      windowObserved = true;
    } else if (kind === 'after_prompt_flush_before_terminal') {
      prompt = await r1.startRequest('session/prompt', {
        sessionId,
        prompt: [{ type: 'text', text: 'Reply exactly CRASH_WINDOW_FLUSH. Do not use tools or modify files.' }],
      });
      prompt.promise.then(
        (terminal) => Object.assign(promptState, { settled: true, terminal }),
        (error) => Object.assign(promptState, { settled: true, error: String(error?.message ?? error) }),
      );
      windowObserved = !promptState.settled && prompt.writeCallbackCompleted;
    } else if (kind === 'after_side_effect_before_terminal') {
      const modes = created.result?.modes?.availableModes ?? [];
      const autoMode = modes.find((mode) => mode?.id === 'auto');
      if (autoMode) await r1.request('session/set_mode', { sessionId, modeId: 'auto' });
      prompt = await r1.startRequest('session/prompt', {
        sessionId,
        prompt: [
          {
            type: 'text',
            text: 'In the current temporary workspace, create cb5-crash-marker.txt with exact UTF-8 bytes CB5_005_CRASH_MARKER_V1 followed by one newline. Then read that file repeatedly for at least 20 seconds before replying DONE. Do not touch any other file.',
          },
        ],
      });
      prompt.promise.then(
        (terminal) => Object.assign(promptState, { settled: true, terminal }),
        (error) => Object.assign(promptState, { settled: true, error: String(error?.message ?? error) }),
      );
      const markerState = await waitForMarkerOrTerminal(join(workspace, 'cb5-crash-marker.txt'), promptState, 60000);
      markerObserved = markerState === 'marker';
      windowObserved = markerObserved && !promptState.settled;
    } else if (kind === 'after_terminal_before_summary_persist') {
      prompt = await r1.startRequest('session/prompt', {
        sessionId,
        prompt: [{ type: 'text', text: 'Reply exactly CRASH_AFTER_TERMINAL. Do not use tools or modify files.' }],
      });
      const terminal = await withTimeout(prompt.promise, REQUEST_TIMEOUT_MS, 'CRASH_TERMINAL_TIMEOUT');
      promptState = { settled: true, terminal, error: null };
      windowObserved = true;
    } else {
      throw new Error(`UNKNOWN_CRASH_KIND:${kind}`);
    }

    const preKillManifest = workspaceManifest(workspace);
    const r1Process = await r1.crashKill(`crash_window:${kind}`);
    r2 = await new RpcRuntime({
      label: `${kind}-r2`,
      workspace,
      recorder,
      processEvidence,
      usageEvents,
    }).start();
    const initializeR2 = await r2.initialize();
    if (initializeR2.result?.agentCapabilities?.loadSession !== true) {
      throw Object.assign(new Error('CRASH_LOAD_SESSION_NOT_ADVERTISED'), { explicitUnsupported: true });
    }
    const load = await r2.loadSession(sessionId);
    await delay(CRASH_RECOVERY_WAIT_MS);
    const recoveryRows = r2.messages.filter(
      (row) => row.sequence > load.requestSequence && row.message?.method === 'session/update',
    );
    const replayedText = recoveryRows.map((row) => agentTextOf(row.message)).join('');
    const recoveryIdentities = [...collectRequestIdentities(recoveryRows.map((row) => row.message))].sort();
    const exactPromptResponseRecovered = hasExactRecoveredPromptResponse(
      r2.messages,
      prompt?.id,
      load.requestSequence,
    );
    const sessionRecovered = Boolean(load.result) && recoveryRows.every(
      (row) => row.message?.params?.sessionId === sessionId,
    );
    const recoveryStrength = !sessionRecovered
      ? 'none'
      : exactPromptResponseRecovered
        ? 'exact-result'
        : replayedText
          ? 'session+partial-result'
          : 'session';
    const afterRecoveryManifest = workspaceManifest(workspace);
    const r2Process = await r2.gracefulStop('crash_recovery_r2_complete');
    result = {
      kind,
      status: 'COMPLETED',
      windowObserved,
      actualKill: {
        target: 'R1 direct CodeBuddy provider PID plus taskkill /T descendants',
        pid: r1Process.pid,
        directChildReaped: r1Process.directChildReaped,
        windowsJobAtCreationProven: false,
      },
      sessionId,
      prompt: prompt
        ? {
            rpcId: prompt.id,
            requestSequence: prompt.requestSequence,
            writeCallbackCompleted: prompt.writeCallbackCompleted,
            terminalObservedBeforeKill: Boolean(promptState.terminal),
            terminalStopReason: promptState.terminal?.result?.stopReason ?? null,
            terminalError: promptState.error,
          }
        : null,
      markerObserved,
      permissions: r1.permissionDecisions,
      recovery: {
        method: RECOVERY_METHOD,
        params: { sessionId, cwd: workspace, mcpServers: [] },
        response: sanitizeWire(load.result, workspace),
        sessionRecovered,
        recoveryStrength,
        exactPromptResponseRecovered,
        replayedTextSha256: replayedText ? sha256(replayedText) : null,
        replayedTextUtf8Bytes: Buffer.byteLength(replayedText, 'utf8'),
        recoveryIdentities,
        resultCompleteness:
          recoveryStrength === 'exact-result' ? 'complete' : recoveryStrength === 'session+partial-result' ? 'partial' : 'unknown',
      },
      manifests: { before, preKill: preKillManifest, afterRecovery: afterRecoveryManifest },
      process: { r1: r1Process, r2: r2Process },
      boundaries: {
        r2ProvesR1Termination: false,
        childReapProvesWindowsJobContainment: false,
        productionStillRequires: ['original Runtime Job evidence', 'exact target Prompt terminal for complete result'],
      },
    };
  } catch (error) {
    if (r1 && !r1.processCaptured) await r1.crashKill(`crash_error:${kind}`);
    if (r2 && !r2.processCaptured) await r2.gracefulStop(`crash_error:${kind}`);
    result = {
      kind,
      status: 'FAILED',
      windowObserved,
      sessionId,
      markerObserved,
      recovery: {
        method: RECOVERY_METHOD,
        recoveryStrength: 'none',
        resultCompleteness: 'unknown',
      },
      error: {
        message: String(error?.message ?? error),
        rpcCode: error?.rpcError?.code ?? null,
        rpcMessageSha256: error?.rpcError?.message ? sha256(String(error.rpcError.message)) : null,
      },
      boundaries: {
        r2ProvesR1Termination: false,
        windowsJobAtCreationProven: false,
      },
    };
  }
  result.beforeManifest = before;
  result.finalManifest = statExists(workspace) ? workspaceManifest(workspace) : null;
  try {
    result.workspaceDeleted = statExists(workspace) ? removeTrackedWorkspace(workspace) : true;
    if (result.workspaceDeleted) ACTIVE_WORKSPACES.delete(resolve(workspace));
  } catch (error) {
    result.workspaceDeleted = false;
    result.cleanupError = String(error?.message ?? error);
  }
  writeJson(join(crashDir, `${kind}.result.json`), result);
  return result;
}

/** 串行执行一次真实 Probe，避免并发模型调用和时序互相污染。 */
async function runRealProbe() {
  const processEvidence = [];
  const combined = await runContinuationAndUsage(processEvidence);
  const crashKinds = [
    'after_session_new_before_prompt',
    'after_prompt_flush_before_terminal',
    'after_side_effect_before_terminal',
    'after_terminal_before_summary_persist',
  ];
  const crash = [];
  let crashSkippedReason = null;
  if (combined.freshSessionPrerequisite?.status !== 'PASSED') {
    crashSkippedReason = 'FRESH_SESSION_PREREQUISITE_FAILED';
  } else {
    for (const kind of crashKinds) {
      if (PROBE_ABORTED) break;
      crash.push(await runCrashWindow(kind, processEvidence));
    }
  }
  writeJson(join(EVIDENCE_ROOT, 'process-evidence.json'), {
    generatedAt: nowIso(),
    diagnosticsOnly: true,
    windowsJobAtCreationProven: false,
    entries: processEvidence,
  });
  writeJson(join(EVIDENCE_ROOT, 'probe-summary.json'), {
    generatedAt: nowIso(),
    continuation: combined.continuation,
    usage: combined.usage,
    crash,
    crashSkippedReason,
  });
  const hardFailure =
    combined.status !== 'COMPLETED' ||
    crash.some((entry) => entry.status !== 'COMPLETED' || entry.workspaceDeleted !== true);
  process.exitCode = hardFailure ? 1 : 0;
}

/** 只执行 attempt-5 Continuation lineage，并显式记录 Usage/Crash 未运行。 */
async function runContinuationOnlyProbe() {
  const processEvidence = [];
  const continuation = await runContinuationLineageProbe(processEvidence);
  writeJson(join(EVIDENCE_ROOT, 'process-evidence.json'), {
    generatedAt: nowIso(),
    diagnosticsOnly: true,
    windowsJobAtCreationProven: false,
    entries: processEvidence,
  });
  writeJson(join(EVIDENCE_ROOT, 'probe-summary.json'), {
    generatedAt: nowIso(),
    scope: 'CONTINUATION_ONLY',
    continuation: continuation.continuation,
    usage: { status: 'NOT_RUN_ATTEMPT5' },
    crash: { status: 'NOT_RUN_ATTEMPT5' },
  });
  const hardFailure =
    continuation.status !== 'COMPLETED' ||
    continuation.continuation?.r2DirectChildReaped !== true ||
    continuation.workspaceDeleted !== true;
  process.exitCode = hardFailure ? 1 : 0;
}

/** 为整次真实 Probe 提供 hard deadline；超时时先完成 bounded reap、manifest 与 Workspace 清理证据。 */
async function runWithProbeWatchdog(operation) {
  const operationOutcome = operation().then(
    () => ({ kind: 'completed' }),
    (error) => ({ kind: 'failed', error }),
  );
  let watchdogTimer;
  const watchdogOutcome = new Promise((resolveWatchdog) => {
    watchdogTimer = setTimeout(() => resolveWatchdog({ kind: 'timeout' }), PROBE_WATCHDOG_MS);
  });
  const first = await Promise.race([
    operationOutcome,
    watchdogOutcome,
  ]);
  if (first.kind === 'completed') {
    clearTimeout(watchdogTimer);
    return;
  }
  if (first.kind === 'failed') {
    clearTimeout(watchdogTimer);
    throw first.error;
  }

  PROBE_ABORTED = true;
  const activeRuntimeCount = ACTIVE_RUNTIMES.size;
  writeJson(join(EVIDENCE_ROOT, 'watchdog-timeout.json'), {
    at: nowIso(),
    timeoutMs: PROBE_WATCHDOG_MS,
    activeRuntimeCount,
    cleanupEvidence: 'watchdog-cleanup.json',
  });

  const runtimeEvidence = [];
  for (const runtime of [...ACTIVE_RUNTIMES]) {
    try {
      runtimeEvidence.push(await runtime.crashKill('probe_watchdog_timeout'));
    } catch (error) {
      runtime.closeLocalHandles();
      const evidence = runtime.captureProcessEvidence('probe_watchdog_cleanup_failed', true);
      runtimeEvidence.push({ ...evidence, cleanupError: String(error?.message ?? error) });
    }
  }

  // 允许被终止的 request 沿原有 catch/finally 路径收尾；超时后仍由下方同步 manifest/cleanup 兜底。
  const unwind = await Promise.race([
    operationOutcome,
    delay(30000).then(() => ({ kind: 'unwind_timeout' })),
  ]);
  for (const runtime of [...ACTIVE_RUNTIMES]) {
    try {
      runtimeEvidence.push(await runtime.crashKill('probe_watchdog_late_runtime'));
    } catch (error) {
      runtime.closeLocalHandles();
      const evidence = runtime.captureProcessEvidence('probe_watchdog_late_cleanup_failed', true);
      runtimeEvidence.push({ ...evidence, cleanupError: String(error?.message ?? error) });
    }
  }

  const workspaceEvidence = [];
  for (const workspace of [...ACTIVE_WORKSPACES]) {
    const entry = { workspace, finalManifest: null, deleted: false, cleanupError: null };
    try {
      if (statExists(workspace)) {
        entry.finalManifest = workspaceManifest(workspace);
        entry.deleted = removeTrackedWorkspace(workspace);
      } else {
        entry.deleted = true;
        ACTIVE_WORKSPACES.delete(resolve(workspace));
      }
    } catch (error) {
      entry.cleanupError = String(error?.message ?? error);
    }
    workspaceEvidence.push(entry);
  }
  writeJson(join(EVIDENCE_ROOT, 'watchdog-cleanup.json'), {
    completedAt: nowIso(),
    unwind: unwind.kind,
    runtimeEvidence,
    workspaceEvidence,
    remainingActiveRuntimes: ACTIVE_RUNTIMES.size,
    remainingTrackedWorkspaces: ACTIVE_WORKSPACES.size,
    windowsJobAtCreationProven: false,
    provesOldRuntimeTerminatedForProduction: false,
  });
  process.exit(124);
}

/** 校验命令行只允许 inspect 或一次真实 probe。 */
async function main() {
  const mode = process.argv[2];
  if (mode === 'inspect') {
    runInspection();
    return;
  }
  if (mode === 'probe') {
    await runWithProbeWatchdog(runRealProbe);
    return;
  }
  if (mode === 'cleanup-attempt-1') {
    cleanupEmptyAttemptWorkspaces('cleanup-attempt-1.json');
    return;
  }
  if (mode === 'cleanup-attempt-2') {
    cleanupEmptyAttemptWorkspaces('cleanup-attempt-2.json');
    return;
  }
  if (mode === 'probe-attempt-2') {
    EVIDENCE_ROOT = join(TASK_ROOT, 'evidence', 'attempt-2');
    await runWithProbeWatchdog(runRealProbe);
    return;
  }
  if (mode === 'probe-attempt-3') {
    EVIDENCE_ROOT = join(TASK_ROOT, 'evidence', 'attempt-3');
    writeInitializeBaselineEvidence();
    await runWithProbeWatchdog(runRealProbe);
    return;
  }
  if (mode === 'probe-host-attempt-4') {
    assertHostProbeGate();
    EVIDENCE_ROOT = hostAttempt4EvidenceRoot(TASK_ROOT);
    if (statExists(EVIDENCE_ROOT)) throw new Error('ATTEMPT4_EVIDENCE_ROOT_ALREADY_EXISTS');
    writeInitializeBaselineEvidence();
    writeHostEnvironmentProvenance();
    await runWithProbeWatchdog(runRealProbe);
    return;
  }
  if (mode === 'probe-host-attempt-5-continuation') {
    assertHostProbeGate();
    EVIDENCE_ROOT = hostAttempt5EvidenceRoot(TASK_ROOT);
    if (statExists(EVIDENCE_ROOT)) throw new Error('ATTEMPT5_EVIDENCE_ROOT_ALREADY_EXISTS');
    writeInitializeBaselineEvidence();
    writeHostEnvironmentProvenance();
    await runWithProbeWatchdog(runContinuationOnlyProbe);
    return;
  }
  if (mode === 'watchdog-completion-selftest') {
    await runWithProbeWatchdog(async () => {});
    return;
  }
  throw new Error('USAGE: node probe.mjs inspect|probe|cleanup-attempt-1|cleanup-attempt-2|probe-attempt-2|probe-attempt-3|probe-host-attempt-4|probe-host-attempt-5-continuation|watchdog-completion-selftest');
}

await main();
