import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import ts from 'typescript';
import { resultText, taskSummary, executionStatus } from './agentPresentation.ts';

test('ExecutionView requires snapshot strings; actions cannot accept output-only root', () => {
  const file = path.resolve('src/__agent_contract_check.ts');
  const source = `import type { ExecutionView, AgentAction } from './types';
    declare const row: ExecutionView;
    const prompt: string = row.prompt;
    const root: string = row.canonicalWorkspaceRoot;
    // @ts-expect-error prompt is mandatory
    const noPrompt: ExecutionView = {} as Omit<ExecutionView, 'prompt'>;
    // @ts-expect-error root is mandatory
    const noRoot: ExecutionView = {} as Omit<ExecutionView, 'canonicalWorkspaceRoot'>;
    // @ts-expect-error read-only root cannot be an action input
    const resume: AgentAction = {action:'resume_pending',executionId:'E',canonicalWorkspaceRoot:root};
    // @ts-expect-error no new runtime field
    row.runtimeInstanceId;
    // @ts-expect-error no new revision field
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
