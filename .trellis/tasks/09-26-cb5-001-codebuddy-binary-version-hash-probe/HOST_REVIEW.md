# CB5-001 Host Review

Status: ACCEPTED as binary identity / diagnostics evidence.

## Host rerun

- Deterministic probe tests: 11/11 PASS.
- Actual explicit-shim probe: PASS.
- Current Host discovery:
  - `where.exe codebuddy`: NOT_FOUND.
  - `where.exe buddycn`: FOUND (`buddycn`, `buddycn.cmd`).
- Canonical real entry:
  - EXE: `%LOCALAPPDATA%\Programs\CodeBuddy CN\CodeBuddy CN.exe`
  - CLI JS: `%LOCALAPPDATA%\Programs\CodeBuddy CN\resources\app\out\cli.js`
  - `ELECTRON_RUN_AS_NODE=1`
- Current CLI `--version`: `1.106.1 / b4c35ed08ffb428910211608831a314565c1256e / x64`.
- PE ProductVersion: `4.12.0`.
- EXE SHA256: `d289e2a508dece84064ffb3243b817faf9c429877b5f022a726002463b428ed2`.
- CLI JS SHA256: `5dd40efc70561675207cde74b2970ac532721944e996a683ea62888bd4a98904`.
- Product source baseline: unchanged.

## Superseding compatibility decision

User explicitly changed the compatibility policy after this probe:

- CodeBuddy product version MUST NOT be used as an admission whitelist.
- PE ProductVersion, CLI/base version, commit and binary hashes are diagnostics/reproducibility evidence only.
- A new CodeBuddy product version or binary hash MUST NOT be rejected merely because it is new/unseen.
- Connection compatibility is determined at managed ACP initialize time by negotiated `protocolVersion`.
- A matching supported ACP protocol version permits connection regardless of CodeBuddy product version.
- Provider feature capabilities are enabled only when the negotiated protocol/capability surface proves the required feature; missing optional capabilities disable those features rather than creating a product-version block.
- Product hash may key cached evidence/diagnostics but is not a trust whitelist.

Therefore any `supported-version candidate`, `supported-version table`, or `CODEBUDDY_VERSION_UNSUPPORTED` language in earlier task evidence is historical and MUST NOT be used by subsequent implementation.

CB5-002 must probe ACP protocol compatibility directly.
