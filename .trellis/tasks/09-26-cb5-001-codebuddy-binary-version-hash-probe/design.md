# CB5-001 Probe Design

## Discovery model

Probe, not product runtime:
1. `where.exe codebuddy` — record result because current design assumed it.
2. `where.exe buddycn` — record actual CN CLI discovery.
3. Resolve shim contents to canonical real entry: `CodeBuddy CN.exe` + `resources/app/out/cli.js`.
4. Canonical identity is the real EXE plus CLI JS, not the `.cmd` shim alone.

## Version evidence

Collect independently:
- CLI raw `--version` stdout/stderr/exit.
- PE VersionInfo: FileVersion, ProductVersion, ProductName.
- `resources/app/product.json`: applicationName, version, commit, quality.
- `resources/app/package.json`: name, version.

Do not collapse these fields. Current Host preflight suggests:
- ProductVersion 4.12.0 = likely CodeBuddy release version.
- product/package 1.106.1 = likely embedded VS Code base.
This is a hypothesis until probe evidence confirms/rejects it.

Parser contract:
- raw CLI semver-like output parses only when non-empty and structurally valid;
- empty/malformed raw output is a parse failure, not version `0` or unknown-success;
- if a fallback to PE ProductVersion is adopted, its use must be explicit in result `{source:'pe_product_version'}` and tested.

## Binary identity

SHA256 real EXE and CLI JS separately. Evidence should bind:
`{productVersion, exeSha256, cliSha256, commit, applicationName}`.

## Scope

Implementation may add only task-local probe harness/scripts/evidence under the CB5-001 Trellis task or a temporary directory. No src/ product code, Cargo dependency, Runtime, Provider, supported-version table, install/upgrade/login behavior.