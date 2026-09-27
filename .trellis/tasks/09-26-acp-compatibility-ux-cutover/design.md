# ACP Compatibility UX Cutover Design

Compatibility authority comes from managed ACP initialize, not product metadata.

`ProviderCatalogEntry.diagnosticCode` remains an optional generic consumer seam. The only compatibility-specific presentation mapping in this task is exact `CODEBUDDY_ACP_INCOMPATIBLE`.

Product version and hashes remain visible/diagnostic metadata but never drive card blocking state.

No backend signal is added here; CB5/CB6 runtime/admission work will later produce stable ACP diagnostics.