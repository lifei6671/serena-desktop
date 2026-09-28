# Independent FULL_SCOPE review round 0

Mode CHILD_AGENT, target FE7BF7ACC568032DC4EE42A3CA2F8A75D29ACB06DE9CB63DC14E41E97CB55BD7, coverage COMPLETE (18 frozen files, 13 full tracked diffs plus 5 contracts), freshness verified by reviewer hashes. No source writes or test runs by reviewer. Gate BLOCKED / CHANGES_REQUESTED; P0 0, P1 1, P2 0.

P1: first cancel can increment generic revision between exact response's generic snapshot and ExactProviderRequest/ObserveTerminal persistence. The old two-step private write then fails OCC, marks Sent uncertain, discards real terminal/text, and recovers Interrupted. New native natural-terminal test had cancelled only after prompt completion and missed this interval.

Required repair: narrow atomic Store operation validating original Runtime, full private identity/revision and current terminal/lifecycle in one transaction, combining optional provider request ID and terminal. Do not relax ordinary OCC or retry terminal. Deterministic handshake must put first cancel after exact response and before private commit, preserve natural outcome/text and zero cancel wire, retain Claim until Job evidence.

All other inspected responsibilities had no findings: generic/Codex, registered control, typed wire/permit/flush, exact owner polling, deadlines/no retry, Job evidence/recovery, capabilities/platform snapshots, forbidden scope. Clippy baseline and unavailable Linux reported honestly.
