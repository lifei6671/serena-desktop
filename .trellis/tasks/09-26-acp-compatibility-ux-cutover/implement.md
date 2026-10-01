# Implementation Plan

1. Capture current frontend baseline.
2. Replace obsolete VERSION_UNSUPPORTED mapping in agentPresentation with ACP_INCOMPATIBLE.
3. Update AgentPanel tests: exact code positive case; near-match/version/generic unavailable negative cases.
4. Preserve provider-neutral static assertion.
5. Run focused AgentPanel/App, npm test, build, lint, diff-check.
6. Verify Rust/Remote protected hashes unchanged.
7. Freeze hashes and wait for Host Gate.