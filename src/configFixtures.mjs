/** 创建独立 Provider 配置 fixture，与 Rust 的迁移默认值一致。 */
export function providerSettingsFixture() {
  return {
    providers: { codex: { enabled: true }, codebuddy: { enabled: false } },
    roleRouting: { development: 'codex', testing: 'codex', review: 'codex', analysis: 'codex', general: 'codex' },
  };
}
