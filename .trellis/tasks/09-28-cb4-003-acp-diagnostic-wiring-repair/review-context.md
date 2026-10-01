# Independent FULL_SCOPE Review Context

## Authority and target

- Baseline HEAD: `5a3f3d331408472bfb66f7deadef83aff29b1715`
- Branch: `feat/codebuddy`
- Executable review target: complete tracked working-tree diff for the 12 paths below
- Frozen diff identity: `git diff --binary | git hash-object --stdin` = `de1100762f29bff8e5e7d65a77591dd76d5477e1`
- Review depth: Tier 3 protocol/concurrency/admission boundary
- Review mode required: independent `CHILD_AGENT`, read-only, `FULL_SCOPE`
- Task artifacts in this directory are review context; the pre-existing CB10-002 directory is explicitly excluded user-owned evidence.

## Executable path hashes

```text
58cdf33cae34271bcab1040100b3e86dbf266c633db70a7d8822328eaa25b30a  src-tauri/src/agent/codebuddy/client_tests.rs
23d79b46f2f26c816169ab3559dffae120e8206da40f80af84f917bc52308e24  src-tauri/src/agent/codebuddy/execute.rs
def1bebac7a2e3cc5f1c218469ecfaae744a27e589a7ef84dbd65c760faeb854  src-tauri/src/agent/codebuddy/execute/tests.rs
52606430629fb77eb48f849195be6fe503a612ee569a9cd947b9895f55ac3c76  src-tauri/src/agent/codebuddy/permission.rs
bd1db04a8135e87dda262047d9b89a5a021fe93ebb1d9b58caddcd2f9daf4da9  src-tauri/src/agent/codebuddy/protocol.rs
8c62713320350be6aee25ee5c26981f683209a736507211340754ca0fae8738d  src-tauri/src/agent/codebuddy/provider.rs
65faef5b1987bec25d63994770614df9946d7a2378866255eb11da60f482d62e  src-tauri/src/agent/product.rs
30a7875a9b2a2d20b757d2cf46f47ea7176e2739d535840f430ef2cb42dd6c97  src-tauri/src/agent/product/provider_catalog_tests.rs
d5f256288cb35e826dbf5394e058018aa88bb8d9c2cd19a36ca0c2a18cabef8e  src-tauri/src/agent/provider/port.rs
c3ab17f16b33e3584bc30f0572fd0eb549f72b100ed4a35459d3e4a97379807e  src-tauri/src/agent/provider/registry.rs
e137b3e7dcc01ec4342e4b3fdb428f630b9769f52b86e7d68a28a4300497dd9c  src-tauri/src/agent/provider/registry/tests.rs
a63df3b869e7fe0d19217cf7412683c568e5244b9ef81fa4c118acda92be2bbd  src-tauri/tests/fixtures/codebuddy_execute_child.rs
```

## Required reviewer focus

- effective health 只能被 deterministic provider-owned contract diagnostic 覆盖；EOF/timeout/malformed/permission/remote 等不得污染。
- `get_registered()` 必须继续支持 Cancel/Recovery；只阻断未来 `get()` admission。
- mismatch 当前 Execution 的 terminal/Runtime/Claim/recovery 收敛不能被改写。
- refresh 必须只做 discovery、替换 adapter 并清除 runtime diagnostic，不能启动 ACP。
- Catalog 必须 provider-neutral；不得按 provider id、版本、hash、error message 或 capability 推断。
- 无 schema/migration、持久 cache、配置 whitelist、Force Unlock/override、TaskManager CodeBuddy 字符串比较。
- tests 必须真正证明 Registry、Product serialization、managed mismatch、transient matrix、refresh、Codex/control 与前端 exact/negative 回归。
- P0/P1/P2 全部必须报告；terminal verdict 必须包含 reviewed target identity、coverage 与 freshness。
