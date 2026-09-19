# P4-007 Authority Snapshot

- User-supplied authoritative contract: `docs/technical-design-agent-platform-v0.2.md` §52, §53, §56.37～§56.42, §101; Host SHA256 `42c0f86bb294dc0425c5a8deee5d9f3f36586704218a832e8a9fe89e5f1d9f19`.
- Task breakdown: `docs/implementation-task-breakdown-agent-platform-v0.2-revision003.md` P4-007; Host SHA256 `d86aedbe60ac8701fbcbff06369566d33f282454c6eef5f4aae198db1908132a`.
- Host frozen full-lib historical failures: `quick_manual_probe_failure_hides_url_and_retry_restores_ready`, `metadata_and_401_without_working_handler_cannot_be_ready`, and `public_vertical_work_source_start_continue_acceptance_e2e`.
- Acceptance requires named failure/signature comparison, not a fixed pass count; P4 test additions may increase pass totals.
