# Implementation

1. Read task/design, frontend specs, current Rust wire and save authority, frontend controller/API and fixture patterns.
2. Update only types.ts, api.ts if needed, useAppController.ts and directly related frontend tests/fixtures. Add Chinese comments for new logic.
3. Run focused config/controller tests, npm test, npm run build, scoped ESLint and diff checks on Windows. No Rust or Linux validation required.
4. Freeze delivery file hashes and obtain independent review; compare baseline hashes to ensure all previous changes preserved.
5. Record verification/review here and stop before CB2-005, without commits. No new spec conventions anticipated; existing frozen design suffices.

trellis-start skill was searched in project and installed skill directories and not found; workflow.md supplies task lifecycle. This optional absence does not block implementation.
