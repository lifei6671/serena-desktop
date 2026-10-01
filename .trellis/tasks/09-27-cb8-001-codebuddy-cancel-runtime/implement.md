# Implementation plan

Baseline feat/codebuddy 9316bb85dc50de873843d6d73cfce95b7229b1a4, clean. User authorized implementation. Native Windows validation; Linux only project Docker, never WSL.

1. Generic Store and provider control regression.
2. Typed cancel permit, physical flush observer and transport tests.
3. Durable owner polling, exact identity, terminal priority, bounded failure convergence.
4. Native fake ACP cancellation/safety matrix and existing runtime recovery regression.
5. Capability/catalog only after implementation Gate.
6. Record verification, freeze and independent read-only FULL_SCOPE review. No commit/push.
