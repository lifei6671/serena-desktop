import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import ts from 'typescript';
import { resultText, taskSummary, executionStatus, executionTime, executionDuration, executionWorkspace } from './agentPresentation.ts';

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
    // @ts-expect-error provider invocation can be uncertain
    const booleanOnly: boolean = control.providerInvoked;
    // @ts-expect-error dispatching is projected as uncertain, not public certainty
    const invalidCertainty: ControlReceipt['dispatchCertainty'] = 'dispatching';
    void [invoked, booleanOnly, invalidCertainty];
    declare const row: ExecutionView;
    declare const nullableString: string | null;
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
    // @ts-expect-error activity is a hint, never a lifecycle phase
    const invalidPhase: ExecutionView['progress']['phase'] = 'stalled';
    void [threadName, errorCode, errorMessage, threadNameAcceptsNullable, errorCodeAcceptsNullable,
      errorMessageAcceptsNullable, noThreadName, noErrorCode, noErrorMessage,
      pendingPhase, activityPhase, toolCategory, lastActivityAt, activityAgeMs, silenceLevel, invalidPhase];
    const prompt: string = row.prompt;
    const revision: string = row.revision;
    const observation: AgentAction = {action:'observe',executionId:'E',knownRevision:revision,waitMs:0,includeResult:true};
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
    void [prompt, root, noPrompt, noRoot, resume];`;
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

test('pending dispatch labels never imply a provider invocation', () => {
  const pending = {status:'dispatch_pending',attention:'none',dispatchState:'not_dispatched',progress:{phase:'pending'}};
  assert.equal(executionStatus(pending).label, '等待执行');
  assert.equal(executionStatus({...pending,attention:'pending_explicit_resume'}).label, '等待恢复');
  assert.equal(executionStatus({...pending,dispatchState:'dispatching',progress:{phase:'dispatching'}}).label, '正在派发');
  assert.equal(executionStatus({...pending,dispatchState:'uncertain',progress:{phase:'reconciling'}}).label, '正在恢复执行状态');
});
