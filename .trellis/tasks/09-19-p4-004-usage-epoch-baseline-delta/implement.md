# P4-004 Implementation Plan

1. Add the crate-private baseline intent and atomic Store operations, including turn synchronization and private/public records.
2. Route public Usage events through the projector, preserving its execution-ID-only boundary.
3. Add the provider baseline freeze and late-turn invalidation around the existing P4-003 adapter identity flow.
4. Add focused deterministic Store, projector, and adapter tests; run the authorized Rust checks.
5. Freeze the owned diff, run an independent read-only review, and record evidence without commit or push.
