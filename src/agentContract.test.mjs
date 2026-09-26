import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import ts from 'typescript';
import { activityLabel, activitySilenceLabel, formatTokenCount, providerLabel, recentActivity, resultText, taskSummary, usageCompletenessLabel, executionStatus, executionTime, executionDuration, executionWorkspace } from './agentPresentation.ts';

test('history uses local full dates, friendly elapsed time and frozen workspace identity', () => {
  assert.equal(executionTime(new Date(2026, 8, 9, 22, 14).getTime()), '2026-09-09 22:14');
  assert.equal(executionDuration({createdAt:0,completedAt:138000},999999), '2分18秒');
  assert.equal(executionDuration({createdAt:0,completedAt:null},90061000), '1天1小时1分1秒');
  assert.equal(executionDuration({createdAt:1000,completedAt:null},1001), '不足1秒');
  assert.equal(executionWorkspace({canonicalWorkspaceRoot:'E:\\old'},[{id:'project-4',name:'New',root:'E:\\new'}]), 'old');
  assert.equal(executionWorkspace({canonicalWorkspaceRoot:'E:\\Repo'},[{id:'project-4',name:'项目名称',root:'E:/repo'}]), '项目名称');
});

test('ExecutionView requires snapshot strings; actions cannot accept output-only root', () => {
  const file = path.resolve('src/__agent_contract_check.ts');
  const source = `import type { ExecutionView, AgentAction, ControlReceipt } from './types';
    declare const control: ControlReceipt;
    const invoked: boolean | null = control.providerInvoked;
    const continueControl: ControlReceipt['nextAction'] = { action: 'continue', executionId: 'E' };
    const continueView: ExecutionView['nextAction'] = { action: 'continue' };
    // @ts-expect-error provider invocation can be uncertain
    const booleanOnly: boolean = control.providerInvoked;
    // @ts-expect-error dispatching is projected as uncertain, not public certainty
    const invalidCertainty: ControlReceipt['dispatchCertainty'] = 'dispatching';
    void [invoked, continueControl, continueView, booleanOnly, invalidCertainty];
    declare const row: ExecutionView;
    declare const nullableString: string | null;
    declare const nullableNumber: number | null;
    const threadName: string | null = row.threadName;
    const errorCode: string | null = row.errorCode;
    const errorMessage: string | null = row.errorMessage;
    const threadNameAcceptsNullable: ExecutionView['threadName'] = nullableString;
    const errorCodeAcceptsNullable: ExecutionView['errorCode'] = nullableString;
    const errorMessageAcceptsNullable: ExecutionView['errorMessage'] = nullableString;
    // @ts-expect-error threadName is always present, though its value may be null
    const noThreadName: ExecutionView = {} as Omit<ExecutionView, 'threadName'>;
    // @ts-expect-error errorCode is always present, though its value may be null
    const noErrorCode: ExecutionView = {} as Omit<ExecutionView, 'errorCode'>;
    // @ts-expect-error errorMessage is always present, though its value may be null
    const noErrorMessage: ExecutionView = {} as Omit<ExecutionView, 'errorMessage'>;
    const pendingPhase: ExecutionView['progress']['phase'] = 'pending';
    const activityPhase: 'provider' | 'tool' | null = row.progress.activityPhase;
    const toolCategory: 'build' | 'test' | 'command' | 'read' | 'edit' | 'tool' | null = row.progress.toolCategory;
    const lastActivityAt: number | null = row.progress.lastActivityAt;
    const activityAgeMs: number | null = row.progress.activityAgeMs;
    const silenceLevel: 'fresh' | 'quiet' | 'prolonged' | null = row.progress.silenceLevel;
    const summaryCode: string | null = row.progress.summaryCode;
    const providerId: string = row.provider.id;
    const providerVersion: string | null = row.provider.version;
    const providerSessionLabel: string | null = row.providerSessionLabel;
    const providerVersionAcceptsNullable: ExecutionView['provider']['version'] = nullableString;
    const providerSessionLabelAcceptsNullable: ExecutionView['providerSessionLabel'] = nullableString;
    const inputTokens: number | null = row.usage.inputTokens;
    const cachedInputTokens: number | null = row.usage.cachedInputTokens;
    const cacheWriteInputTokens: number | null = row.usage.cacheWriteInputTokens;
    const outputTokens: number | null = row.usage.outputTokens;
    const reasoningTokens: number | null = row.usage.reasoningTokens;
    const totalTokens: number | null = row.usage.totalTokens;
    const modelContextWindow: number | null = row.usage.modelContextWindow;
    const completeness: 'unknown' | 'partial' | 'complete' = row.usage.completeness;
    const usageRevision: number = row.usage.usageRevision;
    const usageUpdatedAt: number | null = row.usage.updatedAt;
    const inputTokensAcceptNullable: ExecutionView['usage']['inputTokens'] = nullableNumber;
    const cachedInputTokensAcceptNullable: ExecutionView['usage']['cachedInputTokens'] = nullableNumber;
    const cacheWriteInputTokensAcceptNullable: ExecutionView['usage']['cacheWriteInputTokens'] = nullableNumber;
    const outputTokensAcceptNullable: ExecutionView['usage']['outputTokens'] = nullableNumber;
    const reasoningTokensAcceptNullable: ExecutionView['usage']['reasoningTokens'] = nullableNumber;
    const totalTokensAcceptNullable: ExecutionView['usage']['totalTokens'] = nullableNumber;
    const modelContextWindowAcceptNullable: ExecutionView['usage']['modelContextWindow'] = nullableNumber;
    const usageUpdatedAtAcceptsNullable: ExecutionView['usage']['updatedAt'] = nullableNumber;
    const summaryCodeAcceptsNullable: ExecutionView['progress']['summaryCode'] = nullableString;
    // @ts-expect-error provider 是必填 Product descriptor
    const noProvider: ExecutionView = {} as Omit<ExecutionView, 'provider'>;
    // @ts-expect-error usage 是必填 Product projection
    const noUsage: ExecutionView = {} as Omit<ExecutionView, 'usage'>;
    // @ts-expect-error summaryCode 是 required nullable Product 字段
    const noSummaryCode: ExecutionView['progress'] = {} as Omit<ExecutionView['progress'], 'summaryCode'>;
    // @ts-expect-error activity is a hint, never a lifecycle phase
    const invalidPhase: ExecutionView['progress']['phase'] = 'stalled';
    void [threadName, errorCode, errorMessage, threadNameAcceptsNullable, errorCodeAcceptsNullable,
      errorMessageAcceptsNullable, noThreadName, noErrorCode, noErrorMessage,
      pendingPhase, activityPhase, toolCategory, lastActivityAt, activityAgeMs, silenceLevel, summaryCode,
      providerId, providerVersion, providerSessionLabel, inputTokens, cachedInputTokens, cacheWriteInputTokens, outputTokens, reasoningTokens,
      totalTokens, modelContextWindow, completeness, usageRevision, usageUpdatedAt, providerVersionAcceptsNullable, providerSessionLabelAcceptsNullable,
      inputTokensAcceptNullable, cachedInputTokensAcceptNullable, cacheWriteInputTokensAcceptNullable, outputTokensAcceptNullable, reasoningTokensAcceptNullable,
      totalTokensAcceptNullable, modelContextWindowAcceptNullable, usageUpdatedAtAcceptsNullable, summaryCodeAcceptsNullable, noProvider, noUsage, noSummaryCode, invalidPhase];
    const prompt: string = row.prompt;
    const revision: string = row.revision;
    const controlRevision: string = row.controlRevision;
    const activityRevision: string = row.activityRevision;
    const observation: AgentAction = {action:'observe',executionId:'E',knownRevision:revision,waitMs:0,includeResult:true};
    const activityObservation: AgentAction = {action:'observe',executionId:'E',knownControlRevision:controlRevision,wakeOn:'activity'};
    // @ts-expect-error observation revision is opaque, not numeric
    const numeric: number = row.revision;
    // @ts-expect-error result projection flag is observe-only
    const list: AgentAction = {action:'list',includeResult:true};
    const root: string = row.canonicalWorkspaceRoot;
    // @ts-expect-error prompt is mandatory
    const noPrompt: ExecutionView = {} as Omit<ExecutionView, 'prompt'>;
    // @ts-expect-error root is mandatory
    const noRoot: ExecutionView = {} as Omit<ExecutionView, 'canonicalWorkspaceRoot'>;
    // @ts-expect-error read-only root cannot be an action input
    const resume: AgentAction = {action:'resume_pending',executionId:'E',canonicalWorkspaceRoot:root};
    // @ts-expect-error no new runtime field
    row.runtimeInstanceId;
    // @ts-expect-error internal CAS revision stays private
    row.executionRevision;
    // @ts-expect-error no new diagnostics field
    row.diagnostics;
    void [prompt, root, noPrompt, noRoot, resume, activityRevision, activityObservation];`;
  const options = { strict:true, noEmit:true, skipLibCheck:true, target:ts.ScriptTarget.ES2022, module:ts.ModuleKind.ESNext, moduleResolution:ts.ModuleResolutionKind.Bundler };
  const host = ts.createCompilerHost(options);
  const original = host.getSourceFile.bind(host);
  host.getSourceFile = (name, ...args) => path.resolve(name) === file ? ts.createSourceFile(name, source, options.target, true) : original(name, ...args);
  const diagnostics = ts.getPreEmitDiagnostics(ts.createProgram([file], options, host));
  assert.deepEqual(diagnostics.map(d => ts.flattenDiagnosticMessageText(d.messageText, '\n')), []);
});

test('result rendering respects final phase and never invents text from arbitrary JSON', () => {
  assert.equal(resultText({ finalResult:[{type:'agentMessage',phase:'commentary',text:'not final'}] }), '');
  assert.equal(resultText({ finalResult:[{type:'agentMessage',text:'legacy text'}] }), 'legacy text');
  assert.equal(resultText({ finalResult:[{type:'agentMessage',phase:'final_answer',text:'final'},{type:'agentMessage',phase:'commentary',text:'progress'}] }), 'final');
  assert.equal(resultText({ text:'arbitrary', prompt:'never result' }), '');
  assert.equal(resultText(null), '');
});

test('summary truncates Unicode safely and attention states remain distinct', () => {
  assert.equal(Array.from(taskSummary('🙂'.repeat(101))).length, 101);
  assert.equal(taskSummary('  原文\n第二行 '), '原文 第二行');
  assert.equal(executionStatus({status:'dispatch_pending',attention:'none'}).label, '等待执行');
  assert.equal(executionStatus({status:'dispatch_pending',attention:'pending_explicit_resume'}).label, '等待恢复');
  assert.equal(executionStatus({status:'reconciling',attention:'none'}).label, '正在恢复执行状态');
  assert.equal(executionStatus({status:'unknown',attention:'manual_resolution_required'}).label, '需要处理');
});

test('provider, activity and usage presentation preserve Product facts', () => {
  assert.equal(providerLabel({ provider: { id: 'codex', displayName: ' Codex Local ', version: '0.153.4' } }), 'Codex Local · v0.153.4');
  assert.equal(providerLabel({ provider: { id: 'legacy', displayName: ' ', version: null } }), 'legacy');
  assert.equal(providerLabel({ provider: null }), '未知 Provider');
  const baseProgress = { phase: 'running', activityPhase: 'tool', toolCategory: 'command', lastActivityAt: 900_000, activityAgeMs: null, silenceLevel: 'fresh' };
  const known = ['execution.finalizing', 'execution.reconciling', 'provider.processing', 'tool.read', 'tool.edit', 'tool.command', 'tool.build', 'tool.test', 'tool.other'];
  const labels = ['正在整理结果', '正在恢复执行状态', 'Agent 处理中', '正在读取', '正在修改文件', '正在执行命令', '正在构建', '正在测试', '正在调用工具'];
  assert.deepEqual(known.map(summaryCode => activityLabel({ progress: { ...baseProgress, summaryCode } })), labels);
  assert.equal(activityLabel({ progress: { ...baseProgress, summaryCode: 'provider.private' } }), '执行中');
  assert.equal(activityLabel({ progress: { ...baseProgress, phase: 'mystery', toolCategory: null, summaryCode: null } }), '暂无活动数据');
  assert.deepEqual(['fresh', 'quiet', 'prolonged', null].map(silenceLevel => activitySilenceLabel({ progress: { ...baseProgress, silenceLevel } })), ['刚刚有活动', '暂时没有新活动', '一段时间没有新活动', '暂无活动数据']);
  assert.deepEqual([recentActivity({ progress: { ...baseProgress, activityAgeMs: 0 } }, 1_000_000), recentActivity({ progress: { ...baseProgress, activityAgeMs: 5_000 } }, 1_000_000), recentActivity({ progress: { ...baseProgress, activityAgeMs: 60_000 } }, 1_000_000), recentActivity({ progress: { ...baseProgress, activityAgeMs: 3_600_000 } }, 1_000_000)], ['刚刚', '5秒前', '1分钟前', '1小时前']);
  assert.deepEqual([formatTokenCount(null), formatTokenCount(0), formatTokenCount(12_531)], ['—', '0', '12,531']);
  assert.deepEqual(['unknown', 'partial', 'complete'].map(usageCompletenessLabel), ['未知', '统计不完整', '完整']);
  assert.doesNotMatch(`${activitySilenceLabel({ progress: { ...baseProgress, silenceLevel: 'prolonged' } })} ${activityLabel({ progress: { ...baseProgress, summaryCode: null } })}`, /stalled|卡住|失败/iu);
});

test('pending dispatch labels never imply a provider invocation', () => {
  const pending = {status:'dispatch_pending',attention:'none',dispatchState:'not_dispatched',progress:{phase:'pending'}};
  assert.equal(executionStatus(pending).label, '等待执行');
  assert.equal(executionStatus({...pending,attention:'pending_explicit_resume'}).label, '等待恢复');
  assert.equal(executionStatus({...pending,dispatchState:'dispatching',progress:{phase:'dispatching'}}).label, '正在派发');
  assert.equal(executionStatus({...pending,dispatchState:'uncertain',progress:{phase:'reconciling'}}).label, '正在恢复执行状态');
});
