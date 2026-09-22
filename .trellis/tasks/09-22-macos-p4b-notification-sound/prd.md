# Phase 4B：macOS 通知与真实提示音

## Goal

为 macOS 保留独立系统通知与提示音开关，使用 AudioToolbox 播放用户首选警告音，并定义通知权限及点击激活的真机验收边界。

## Requirements

- macOS 必须继续保留“系统通知”和“提示音”两个独立开关；任一能力单独启用时均按各自语义工作。
- macOS 提示音必须调用系统原生能力并播放用户首选警告音，不得继续返回成功但实际静默。
- 系统通知本身保持无声；当两个开关同时启用时只播放一次提示音。
- Windows 继续使用现有 `MessageBeep`，其他非 macOS 平台行为保持不变。
- 提示音属于非关键桌面副作用，不得改变 Agent 终态、终态去重或错误传播契约。
- 通知权限的未决定、允许、拒绝，以及点击通知后的应用激活，必须由真实 `.app` 验证；没有真机证据时不得标记为通过。
- 本任务不替换现有通知插件，不增加通知点击监听层，不修改前端设置 UI，也不处理 LAN 权限、文件系统或其他 Phase 4 项目。

## Acceptance Criteria

- [ ] macOS 使用 AudioToolbox 播放 `kSystemSoundID_UserPreferredAlert`，不新增音频依赖或外部播放器进程。
- [ ] “仅系统通知”“仅提示音”“二者同时启用”“二者同时关闭”继续满足独立开关策略，二者同时启用时无重复声音来源。
- [ ] Windows 与其他平台的条件编译边界和既有行为不回归。
- [ ] macOS 构建、相关策略测试、格式检查、Clippy 与完整 Rust 测试通过。
- [ ] 真机人工验收记录明确区分通知权限、点击激活和声音可听性；未执行或失败的项目继续保持未完成。

## Notes

- 首版最低目标为 macOS 12.0；所选 AudioToolbox API 自 macOS 10.11 起可用。
- 当前 `tauri-plugin-notification` Desktop 权限 API 不反映 macOS 真实授权状态，本任务不据此宣称权限 Gate 已通过。
