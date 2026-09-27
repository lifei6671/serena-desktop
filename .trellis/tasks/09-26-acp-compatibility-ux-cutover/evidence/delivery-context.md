# Delivery context

- Task: existing 09-26-acp-compatibility-ux-cutover, started with `python .trellis/scripts/task.py start 09-26-acp-compatibility-ux-cutover` (exit 0).
- CWD: E:\wx_lifeilin\github.com\lifei6671\serena-desktop
- Scope: task-owned delta in src/agentPresentation.ts and src/AgentPanel.test.mjs plus task-local evidence. Existing dirty changes excluded from ownership.
- Baseline: baseline-head.txt, baseline-status.txt, baseline-hashes.json and baseline source snapshots. Hash inventory includes tracked and nonignored untracked files outside this task directory; protects all Rust/Remote files and pre-existing frontend work.
- Authority: user exact ACP diagnostic/wording requirements, task PRD/design/implement/manifests, frozen source documents, AGENTS.md, docs/ui/DESIGN.md, .trellis/spec/frontend/index.md and applicable frontend guidance.
- Frozen design SHA256: 25f71439f5b361ee7f612f4baad3b17e18b523646f129a249d10fecbc5c4ea0c
- Frozen breakdown SHA256: 0cc8d9318c6a637a880b36e99ee88355118775cd36bdb26108569062b877029f
- Required checks: focused AgentPanel/App tests, npm test, npm run build, npm run lint, git diff --check, protected-file baseline hash comparison.
- Verification platform: native Windows frontend checks only; no Linux validation claim and no WSL.
- Delivery review: Standard Mode, Tier 1 focused full-scope independent child review after validation. Correctness, contract compatibility and meaningful regression testing; JS/TS language profiles.
- No backend diagnostic producer or ACP handshake is implemented or validated here. No commits, archive or next-task advancement; stop for Host Gate.
- Skill availability: trellis-start absent from workspace/user skill directories and plugin cache. Existing workflow.md and task manifests used directly.
- Spec update judgment: policy already frozen in host design and task artifacts; no additional convention discovered and shared spec edits outside allowed scope.
