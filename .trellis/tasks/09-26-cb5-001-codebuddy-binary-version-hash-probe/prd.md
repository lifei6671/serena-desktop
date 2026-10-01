# CB5-001 CodeBuddy Binary / Version / Hash Probe

## Goal

冻结当前 Windows 主机上真实安装的 CodeBuddy CN binary identity、discovery command、版本来源和 SHA-256 证据，为后续 ACP Contract Probe 提供可引用的 binary anchor。只做 contract probe/evidence，不修改 Serena Desktop 产品 Runtime、Provider、安装或登录行为。

## Host preflight facts to verify, not blindly trust

- `where.exe codebuddy` 当前返回 not found。
- 用户级 PATH 与 Serena/MCP 受管 PATH 均包含 `C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\bin`。
- 该目录实际暴露 `buddycn` / `buddycn.cmd`，不是 `codebuddy`。
- `where.exe buddycn` 返回：
  - `C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\bin\buddycn`
  - `C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\bin\buddycn.cmd`
- `buddycn.cmd` 解析到真实入口：
  - EXE: `C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\CodeBuddy CN.exe`
  - CLI JS: `C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\resources\app\out\cli.js`
  - env: `ELECTRON_RUN_AS_NODE=1`
- Host 预探测：
  - EXE SHA256 `d289e2a508dece84064ffb3243b817faf9c429877b5f022a726002463b428ed2`
  - CLI JS SHA256 `5dd40efc70561675207cde74b2970ac532721944e996a683ea62888bd4a98904`
  - EXE FileVersion `1.106.1.0`
  - EXE ProductVersion `4.12.0`
  - ProductName `CodeBuddy CN`
  - `resources/app/product.json`: applicationName=`buddycn`, version=`1.106.1`, commit=`b4c35ed08ffb428910211608831a314565c1256e`, quality=`stable`
  - direct real-entry `--version` and `--help`: exit 0 with empty stdout in Host probe.

## Requirements

- Probe must record absolute paths for shim, real EXE, CLI JS.
- Record raw `where.exe` outputs for both `codebuddy` and `buddycn`.
- Record raw `--version` stdout/stderr/exit code from the real entry path; empty output must be preserved as evidence, not silently replaced.
- Distinguish CodeBuddy product version from embedded VS Code base version.
- Define and test a version discovery rule grounded in actual evidence:
  - primary candidate must be a CodeBuddy product-owned version source;
  - do not silently treat `product.json.version=1.106.1` as CodeBuddy product version if EXE ProductVersion says `4.12.0`;
  - if CLI raw version is empty/malformed, parser path must fail closed and an explicitly documented metadata fallback may be used only if its semantics are proven.
- Compute SHA256 for EXE and CLI JS; optionally shim hash.
- Evidence must be sanitized: user-specific absolute path may be recorded only as required local evidence; do not include credentials/tokens.
- No ACP initialize/session in this task.
- No product runtime/discovery implementation yet.

## Tests

- found: valid installed CodeBuddy CN layout resolves to real EXE + CLI JS and version evidence.
- missing: no matching shim/entry -> stable NOT_FOUND probe result.
- malformed version: raw version absent/invalid -> parser rejects; metadata fallback behavior explicitly tested if used.
- actual host probe: real files, real hashes, real raw outputs.

## Acceptance

- `verification.md` records discovery evidence, raw outputs, parsed/fallback version semantics and limitations.
- `binary.sha256` records exact binary identity, at minimum real EXE hash and ideally CLI JS hash.
- A supported-version candidate can cite exact EXE hash + product version source + commit/build metadata without conflating VS Code base version.
- Probe explicitly states that current technical-design placeholder `where.exe codebuddy` does not match installed CodeBuddy CN command name.
- No product source code changed.