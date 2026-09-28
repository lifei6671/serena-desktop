import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import {
  CONTINUATION_RECALL_PROMPT,
  RECOVERY_METHOD,
  TASK_TEMP_PREFIX,
  analyzeUsage,
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
  sha256,
  usageUpdateOf,
  workspaceManifest,
} from './lib.mjs';

const HARNESS_ROOT = fileURLToPath(new URL('.', import.meta.url));
const REPO_ROOT = resolve(HARNESS_ROOT, '..', '..', '..', '..');
const TASK_ROOT = resolve(HARNESS_ROOT, '..');

/** 从冻结 Host wire 中取得唯一 initialize request params。 */
function initializeParamsFromWire(path) {
  const rows = readFileSync(path, 'utf8')
    .split(/\r?\n/u)
    .filter(Boolean)
    .map((line) => JSON.parse(line));
  const requests = rows.filter((row) => row.message?.method === 'initialize');
  assert.equal(requests.length, 1);
  return requests[0].message.params;
}

// 防止 harness initialize params 再次偏离两份冻结的 CB5-004 Host PASS wire。
test('initialize params exactly match frozen CB5-004 Host baseline', () => {
  const directWire = join(
    REPO_ROOT,
    '.trellis/tasks/09-26-cb5-004-cancel-permission-contract/evidence/host-direct-codebuddy-proof/wire.jsonl',
  );
  const exactLauncherWire = join(
    REPO_ROOT,
    '.trellis/tasks/09-26-cb5-004-cancel-permission-contract/evidence/host-exact-launcher-proof/wire.jsonl',
  );
  assert.equal(
    sha256(readFileSync(directWire)),
    'bf193e9d353357a1114293307107a4d5b6a1e671e6b2a6cb4c4fe45ae04b1b01',
  );
  assert.equal(
    sha256(readFileSync(exactLauncherWire)),
    '5f2c4df063d214627f17e8a5bf77975d6b4e935d648a2127d0b2d16ec81b9e9f',
  );
  const generated = cb5004InitializeParams();
  assert.deepEqual(generated, initializeParamsFromWire(directWire));
  assert.deepEqual(generated, initializeParamsFromWire(exactLauncherWire));
});

// 验证 operation 正常完成后 watchdog timer 被取消，子进程不会空等 20 分钟。
test('completed watchdog operation releases its timer', () => {
  const child = spawnSync(process.execPath, [join(HARNESS_ROOT, 'probe.mjs'), 'watchdog-completion-selftest'], {
    cwd: REPO_ROOT,
    windowsHide: true,
    timeout: 2000,
    encoding: 'utf8',
  });
  assert.equal(child.error, undefined);
  assert.equal(child.status, 0, child.stderr);
});

// 验证 Host mode 没有显式门禁时在任何 evidence 写入和 Provider spawn 前拒绝。
test('host attempt4 mode rejects without explicit gate and preserves evidence tree', () => {
  const evidenceRoot = hostAttempt4EvidenceRoot(TASK_ROOT);
  const before = existsSync(evidenceRoot) ? workspaceManifest(evidenceRoot) : null;
  const env = { ...process.env };
  delete env.SERENA_CB5_HOST_PROBE;
  const child = spawnSync(process.execPath, [join(HARNESS_ROOT, 'probe.mjs'), 'probe-host-attempt-4'], {
    cwd: REPO_ROOT,
    windowsHide: true,
    timeout: 2000,
    encoding: 'utf8',
    env,
  });
  assert.notEqual(child.status, 0);
  assert.match(child.stderr, /HOST_PROBE_GATE_REQUIRED/u);
  const after = existsSync(evidenceRoot) ? workspaceManifest(evidenceRoot) : null;
  assert.deepEqual(after, before);
});

// 验证 attempt4 evidence 固定在独立 task-local 目录，不指向生产源码或前三次目录。
test('host attempt4 evidence path is independent and outside production paths', () => {
  const evidenceRoot = hostAttempt4EvidenceRoot(TASK_ROOT);
  assert.equal(evidenceRoot, resolve(TASK_ROOT, 'evidence', 'attempt-4'));
  assert.notEqual(evidenceRoot, resolve(TASK_ROOT, 'evidence'));
  assert.notEqual(evidenceRoot, resolve(TASK_ROOT, 'evidence', 'attempt-2'));
  assert.notEqual(evidenceRoot, resolve(TASK_ROOT, 'evidence', 'attempt-3'));
  for (const productionRoot of ['src', 'src-tauri', 'docs']) {
    assert.equal(evidenceRoot.startsWith(`${resolve(REPO_ROOT, productionRoot)}\\`), false);
  }
});

// 验证 attempt-5 Host mode 没有显式门禁时，在 evidence 写入和 Provider spawn 前拒绝。
test('host attempt5 continuation mode rejects without explicit gate and preserves evidence tree', () => {
  const evidenceRoot = hostAttempt5EvidenceRoot(TASK_ROOT);
  const before = existsSync(evidenceRoot) ? workspaceManifest(evidenceRoot) : null;
  const env = { ...process.env };
  delete env.SERENA_CB5_HOST_PROBE;
  const child = spawnSync(
    process.execPath,
    [join(HARNESS_ROOT, 'probe.mjs'), 'probe-host-attempt-5-continuation'],
    {
      cwd: REPO_ROOT,
      windowsHide: true,
      timeout: 2000,
      encoding: 'utf8',
      env,
    },
  );
  assert.notEqual(child.status, 0);
  assert.match(child.stderr, /HOST_PROBE_GATE_REQUIRED/u);
  const after = existsSync(evidenceRoot) ? workspaceManifest(evidenceRoot) : null;
  assert.deepEqual(after, before);
});

// 验证 attempt-5 固定使用独立 task-local evidence 目录。
test('host attempt5 evidence path is independent from all earlier attempts and production', () => {
  const evidenceRoot = hostAttempt5EvidenceRoot(TASK_ROOT);
  assert.equal(evidenceRoot, resolve(TASK_ROOT, 'evidence', 'attempt-5'));
  for (const prior of ['attempt-2', 'attempt-3', 'attempt-4']) {
    assert.notEqual(evidenceRoot, resolve(TASK_ROOT, 'evidence', prior));
  }
  for (const productionRoot of ['src', 'src-tauri', 'docs']) {
    assert.equal(evidenceRoot.startsWith(`${resolve(REPO_ROOT, productionRoot)}\\`), false);
  }
});

// 验证 markdown、反引号和说明文字不会破坏 marker/cwd 语义判定。
test('continuation semantic facts accept decorated answer without storing its text', () => {
  const marker = 'CB5_CONTINUATION_MARKER_20260928_V1';
  const workspace = 'C:\\Users\\Lifei\\AppData\\Local\\Temp\\cb5-attempt5';
  const answer = `Recovered: \`${marker}\`\nDirectory: \`c:/users/lifei/appdata/local/temp/cb5-attempt5\`.`;
  const facts = continuationAnswerFacts(answer, marker, workspace);
  assert.equal(facts.markerPresent, true);
  assert.equal(facts.cwdPresent, true);
  assert.equal(facts.answerSha256, sha256(answer));
  assert.equal(facts.answerUtf8Bytes, Buffer.byteLength(answer, 'utf8'));
  assert.equal(Object.hasOwn(facts, 'answer'), false);
});

// 验证错误 marker 不能通过语义 lineage。
test('continuation semantic facts reject wrong marker', () => {
  const facts = continuationAnswerFacts(
    'WRONG_MARKER | C:/Temp/cb5-attempt5',
    'EXPECTED_MARKER',
    'C:\\Temp\\cb5-attempt5',
  );
  assert.equal(facts.markerPresent, false);
  assert.equal(facts.cwdPresent, true);
});

// 验证错误 cwd 不能通过语义 lineage。
test('continuation semantic facts reject wrong cwd', () => {
  const facts = continuationAnswerFacts(
    'EXPECTED_MARKER | C:/Temp/wrong-workspace',
    'EXPECTED_MARKER',
    'C:\\Temp\\cb5-attempt5',
  );
  assert.equal(facts.markerPresent, true);
  assert.equal(facts.cwdPresent, false);
});

// 验证 Windows 路径判定大小写无关且正反斜杠等价。
test('continuation cwd containment normalizes case and slash direction', () => {
  const facts = continuationAnswerFacts(
    'MARKER | c:/users/LIFEI/temp/WORKSPACE',
    'MARKER',
    'C:\\Users\\lifei\\Temp\\workspace',
  );
  assert.equal(facts.cwdPresent, true);
});

// 验证 P3 只要求回忆，不再次提供 marker 或当前 Workspace 字面值。
test('attempt5 P3 prompt contains neither marker nor workspace literal', () => {
  const marker = 'CB5_CONTINUATION_MARKER_20260928_V1';
  const workspace = 'C:\\Users\\Lifei\\Temp\\cb5-attempt5';
  const promptFacts = continuationAnswerFacts(CONTINUATION_RECALL_PROMPT, marker, workspace);
  assert.equal(promptFacts.markerPresent, false);
  assert.equal(promptFacts.cwdPresent, false);
  assert.match(CONTINUATION_RECALL_PROMPT, /previous runtime/u);
  assert.match(CONTINUATION_RECALL_PROMPT, /being given the literal again/u);
});

// 验证 attempt-5 CLI 分支只路由 Continuation-only operation，不复跑完整 Usage/Crash Probe。
test('attempt5 mode invokes continuation-only probe', () => {
  const source = readFileSync(join(HARNESS_ROOT, 'probe.mjs'), 'utf8');
  const start = source.indexOf("if (mode === 'probe-host-attempt-5-continuation')");
  const end = source.indexOf("if (mode === 'watchdog-completion-selftest')", start);
  assert.notEqual(start, -1);
  assert.notEqual(end, -1);
  const modeBlock = source.slice(start, end);
  assert.equal(modeBlock.includes('runWithProbeWatchdog(runContinuationOnlyProbe)'), true);
  assert.equal(modeBlock.includes('runRealProbe'), false);
});

// 验证环境 provenance 只包含 allowlist 键的 present/absent，不泄露任何值或额外键。
test('host environment provenance never records environment values', () => {
  const provenance = hostEnvironmentProvenance(
    {
      SERENA_CB5_HOST_PROBE: '1',
      HTTPS_PROXY: 'https://secret-proxy.example',
      CODEBUDDY_TOKEN: 'secret-token',
      UNLISTED_SECRET: 'must-not-appear',
    },
    {
      platform: 'win32',
      nodeVersion: 'v24.test',
      nodePath: 'C:\\node.exe',
      nodeSha256: 'node-hash',
      scriptPath: 'C:\\codebuddy',
      scriptSha256: 'script-hash',
    },
  );
  assert.equal(provenance.runner, 'serena-command-host');
  assert.equal(provenance.hostGatePresent, true);
  assert.equal(provenance.variables.HTTPS_PROXY, 'present');
  assert.equal(provenance.variables.CODEBUDDY_TOKEN, 'present');
  assert.equal(provenance.variables.HTTP_PROXY, 'absent');
  assert.equal(provenance.proxyEnvironmentPresent, true);
  assert.equal(provenance.codebuddyEnvironmentPresent, true);
  assert.equal(provenance.valuesRecorded, false);
  const serialized = JSON.stringify(provenance);
  assert.equal(serialized.includes('secret-proxy.example'), false);
  assert.equal(serialized.includes('secret-token'), false);
  assert.equal(serialized.includes('UNLISTED_SECRET'), false);
});

// 验证清洗保留协议结构和 usage 数值，但不保存正文、token 或真实临时路径。
test('sanitizeWire redacts sensitive text while preserving contract fields', () => {
  const workspace = 'C:\\Temp\\cb5-005-secret';
  const input = {
    jsonrpc: '2.0',
    method: 'session/update',
    params: {
      sessionId: 'S1',
      cwd: workspace,
      authorization: 'Bearer secret',
      update: {
        sessionUpdate: 'usage_update',
        used: 42,
        size: null,
        content: { type: 'text', text: 'sensitive model output' },
      },
    },
  };
  const output = sanitizeWire(input, workspace);
  assert.equal(output.method, 'session/update');
  assert.equal(output.params.cwd, '<TEMP_WORKSPACE>');
  assert.equal(output.params.authorization, '<REDACTED_SECRET>');
  assert.equal(output.params.update.used, 42);
  assert.equal(output.params.update.size, null);
  assert.equal(output.params.update.content.redacted, undefined);
  assert.equal(output.params.update.content.text.redacted, true);
  assert.equal(output.params.update.content.text.utf8Bytes, 22);
});

// 验证 parser 只接受单个 JSON-RPC 对象，避免把空行或数组当证据。
test('parseJsonRpcLine rejects non-object frames', () => {
  assert.deepEqual(parseJsonRpcLine('{"jsonrpc":"2.0","id":1,"result":{}}'), {
    jsonrpc: '2.0',
    id: 1,
    result: {},
  });
  assert.throws(() => parseJsonRpcLine('   '), /EMPTY_JSON_RPC_LINE/);
  assert.throws(() => parseJsonRpcLine('[]'), /JSON_RPC_FRAME_NOT_OBJECT/);
});

// 验证 usage parser 仅接受 typed usage_update，不凭普通字段名猜测。
test('usageUpdateOf requires typed usage_update discriminator', () => {
  const typed = {
    method: 'session/update',
    params: { update: { sessionUpdate: 'usage_update', used: 10 } },
  };
  const untyped = {
    method: 'session/update',
    params: { update: { used: 10 } },
  };
  assert.deepEqual(usageUpdateOf(typed), { sessionUpdate: 'usage_update', used: 10 });
  assert.equal(usageUpdateOf(untyped), null);
});

// 验证 request identity 递归提取，不从无关字符串推断绑定关系。
test('collectRequestIdentities extracts only named identity fields', () => {
  const identities = collectRequestIdentities({
    params: {
      conversationRequestId: 'C1',
      update: { requestId: 'R1', text: 'providerRequestId:FAKE' },
    },
  });
  assert.deepEqual([...identities].sort(), ['conversationRequestId:C1', 'requestId:R1']);
});

// 验证 normalization 保留 0/null 区别，并在缺 exact Prompt identity 时维持 INCONCLUSIVE。
test('analyzeUsage keeps unknown semantics without prompt identity', () => {
  const analysis = analyzeUsage(
    [
      {
        sequence: 3,
        runtime: 'r1',
        sessionId: 'S1',
        update: { sessionUpdate: 'usage_update', used: 0, size: null },
        requestIdentities: [],
      },
      {
        sequence: 8,
        runtime: 'r1',
        sessionId: 'S1',
        update: { sessionUpdate: 'usage_update', used: 15, size: 100 },
        requestIdentities: [],
      },
    ],
    [{ label: 'P1', promptSequence: 4, terminalSequence: 7, graceEndSequence: 9 }],
  );
  assert.equal(analysis.eventCount, 2);
  assert.equal(analysis.publicConclusion, 'INCONCLUSIVE');
  assert.equal(analysis.tokenUsageCapability, false);
  assert.equal(analysis.fields.find((field) => field.name === 'used').types[0], 'integer');
  assert.deepEqual(analysis.fields.find((field) => field.name === 'size').types, ['integer', 'null']);
  assert.deepEqual(analysis.windows[0].afterTerminal, [8]);
});

// 验证零事件本身不能区分“不支持”和“Probe 未到达 prompt”，必须保持 INCONCLUSIVE。
test('analyzeUsage does not turn missing events into unsupported', () => {
  const analysis = analyzeUsage([], []);
  assert.equal(analysis.publicConclusion, 'INCONCLUSIVE');
  assert.equal(analysis.tokenUsageCapability, false);
  assert.equal(analysis.reason, 'NO_REAL_USAGE_EVENT_REQUIRES_CALLER_PREREQUISITE_ASSESSMENT');
});

// 验证 usage identity 必须与对应 prompt window 相等，数值走势本身不授权 scope/reset 语义。
test('analyzeUsage binds exact prompt identity without guessing numeric semantics', () => {
  const events = [
    {
      sequence: 5,
      runtime: 'r1',
      sessionId: 'S1',
      update: { sessionUpdate: 'usage_update', used: 10 },
      requestIdentities: ['conversationRequestId:C1'],
    },
    {
      sequence: 10,
      runtime: 'r1',
      sessionId: 'S1',
      update: { sessionUpdate: 'usage_update', used: 20 },
      requestIdentities: ['conversationRequestId:C2'],
    },
    {
      sequence: 15,
      runtime: 'r2',
      sessionId: 'S1',
      update: { sessionUpdate: 'usage_update', used: 24 },
      requestIdentities: ['conversationRequestId:C3'],
    },
  ];
  const windows = [
    { label: 'P1', sessionId: 'S1', promptSequence: 3, terminalSequence: 5, graceEndSequence: 6, providerRequestIdentities: ['conversationRequestId:C1'] },
    { label: 'P2', sessionId: 'S1', promptSequence: 8, terminalSequence: 10, graceEndSequence: 11, providerRequestIdentities: ['conversationRequestId:C2'] },
    { label: 'P3_AFTER_RESTART', sessionId: 'S1', promptSequence: 13, terminalSequence: 15, graceEndSequence: 16, providerRequestIdentities: ['conversationRequestId:C3'] },
  ];
  const analysis = analyzeUsage(events, windows);
  assert.equal(analysis.exactPromptBound, true);
  assert.equal(analysis.fields[0].scope, 'unknown');
  assert.equal(analysis.fields[0].resetBehavior, 'unknown');
  assert.deepEqual(analysis.fields[0].exactPromptIdentity, [
    'conversationRequestId:C1',
    'conversationRequestId:C2',
    'conversationRequestId:C3',
  ]);
  const mismatched = analyzeUsage(
    [{ ...events[0], requestIdentities: ['conversationRequestId:OTHER'] }],
    [windows[0]],
  );
  assert.equal(mismatched.exactPromptBound, false);
});

// 验证 late/terminal coverage 只按字段自身的 exact-bound sample 归类，不由其他字段连带推断。
test('analyzeUsage classifies late behavior per exact-bound field', () => {
  const events = [
    {
      sequence: 5,
      runtime: 'r1',
      sessionId: 'S1',
      update: { sessionUpdate: 'usage_update', used: 10, cost: 1 },
      requestIdentities: ['conversationRequestId:C1'],
    },
    {
      sequence: 7,
      runtime: 'r1',
      sessionId: 'S1',
      update: { sessionUpdate: 'usage_update', used: 12 },
      requestIdentities: ['conversationRequestId:C1'],
    },
  ];
  const windows = [
    { label: 'P1', sessionId: 'S1', promptSequence: 3, terminalSequence: 6, graceEndSequence: 8, providerRequestIdentities: ['conversationRequestId:C1'] },
  ];
  const analysis = analyzeUsage(events, windows);
  assert.equal(analysis.fields.find((field) => field.name === 'used').lateBehavior, 'observed_after_terminal');
  assert.equal(analysis.fields.find((field) => field.name === 'cost').lateBehavior, 'not_observed_in_bounded_grace');
  assert.equal(analysis.fields.find((field) => field.name === 'cost').terminalCoverage, 'all_observed_prompt_windows');
});

// 验证 crash exact-result 从 R2 全消息找 original prompt response，不把 replay update 当 terminal。
test('hasExactRecoveredPromptResponse distinguishes response from replay update', () => {
  const replay = {
    sequence: 20,
    direction: 'provider_to_client',
    message: { method: 'session/update', params: { sessionId: 'S1', update: { sessionUpdate: 'agent_message_chunk' } } },
  };
  const response = {
    sequence: 21,
    direction: 'provider_to_client',
    message: { jsonrpc: '2.0', id: 'PROMPT-1', result: { stopReason: 'end_turn' } },
  };
  const rpcError = {
    sequence: 22,
    direction: 'provider_to_client',
    message: { jsonrpc: '2.0', id: 'PROMPT-1', error: { code: -32603, message: 'failed' } },
  };
  const malformedResult = {
    sequence: 23,
    direction: 'provider_to_client',
    message: { jsonrpc: '2.0', id: 'PROMPT-1', result: {} },
  };
  assert.equal(hasExactRecoveredPromptResponse([replay], 'PROMPT-1', 10), false);
  assert.equal(hasExactRecoveredPromptResponse([replay, rpcError], 'PROMPT-1', 10), false);
  assert.equal(hasExactRecoveredPromptResponse([replay, malformedResult], 'PROMPT-1', 10), false);
  assert.equal(hasExactRecoveredPromptResponse([replay, response], 'PROMPT-1', 10), true);
});

// 验证 manifest 对文件内容取 hash，并且只清理受任务前缀约束的系统临时目录。
test('workspace manifest and cleanup remain inside owned temp root', () => {
  const workspace = mkdtempSync(join(tmpdir(), `${TASK_TEMP_PREFIX}unit-`));
  writeFileSync(join(workspace, 'marker.txt'), 'fixture\n', 'utf8');
  const manifest = workspaceManifest(workspace);
  assert.equal(manifest['marker.txt'].kind, 'file');
  assert.equal(manifest['marker.txt'].size, 8);
  assert.equal(removeOwnedTempWorkspace(workspace), true);
  assert.throws(() => removeOwnedTempWorkspace(tmpdir()), /WORKSPACE_OUTSIDE_TEMP|WORKSPACE_PREFIX_MISMATCH/);
});

// 验证代码只暴露 session/load，不含猜测型 recovery fallback。
test('recovery method is uniquely frozen to session/load', () => {
  assert.equal(RECOVERY_METHOD, 'session/load');
  const probeSource = readFileSync(
    join(fileURLToPath(new URL('.', import.meta.url)), 'probe.mjs'),
    'utf8',
  );
  assert.equal(probeSource.includes("request('session/resume'"), false);
  assert.equal(probeSource.includes("startRequest('session/resume'"), false);
});
