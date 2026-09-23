# Phase 4B：macOS 通知与真实提示音设计

## 范围

本任务只修复 macOS “提示音开关已开启但实际静默”的产品缺口，并冻结通知权限、通知点击激活和声音可听性的真机验收边界。现有 Agent 终态策略、通知正文、Host 生命周期去重、Windows `MessageBeep` 和 Tauri 通知插件路径保持不变。

LAN `Info.plist`、Workspace 文件权限、前端平台文案、LaunchAgent、菜单栏和其他 Phase 4 行为不在本任务内。

## 方案选择

采用 AudioToolbox 原生提示音：macOS 调用 `AudioServicesPlayAlertSoundWithCompletion`，传入 `kSystemSoundID_UserPreferredAlert` 和空 completion block。该常量播放用户在系统设置中选择的警告音，API 自 macOS 10.11 起可用，满足项目 macOS 12.0 最低版本目标。

不采用以下方案：

- 通知 builder 的 `sound("default")`：提示音会依赖系统通知开关，无法满足两个开关独立工作的产品契约。
- `/usr/bin/afplay`：会增加外部进程生命周期和固定声音文件路径，不如系统 API 直接。
- 直接接入 `notify-rust` 的通知 handle：虽可监听点击，但会绕过当前 Tauri 插件路径并引入阻塞等待及窗口激活协调；当前没有真机失败证据证明必须增加这层实现。
- macOS 隐藏提示音开关：与用户已确认保留两个独立开关的产品决定冲突。

## 行为与数据流

`plan_agent_notification` 继续只负责从终态和配置生成 `AgentNotificationPlan`：

1. `send_system_notification=true` 时沿用现有 Tauri 通知插件发送无声通知。
2. `play_sound=true` 时独立调用平台提示音实现。
3. 两者同时为 `true` 时仍只存在第二步这一处声音来源，不给通知 builder 附加声音，因此不会双响。
4. 两者同时为 `false`、提醒类别关闭或用户主动取消时，继续不产生桌面副作用。

通知发送失败或声音调用异常继续只记录固定安全码，不改变 Agent 终态。AudioToolbox 播放 API 返回 `void`，macOS 实现只能确认调用已提交，不能同步确认扬声器实际发声；静音、音量和系统提示音设置属于真机验收条件。

## 平台实现

`agent_notification.rs` 保持局部条件编译，不引入通用平台 trait：

- `#[cfg(windows)]`：保留现有 `MessageBeep(0)`。
- `#[cfg(target_os = "macos")]`：链接系统 `AudioToolbox` framework，以最小 FFI 声明调用 `AudioServicesPlayAlertSoundWithCompletion`；声音 ID 固定为 SDK 定义的 `0x0000_1000`。
- `#[cfg(all(not(windows), not(target_os = "macos")))]`：继续保持既有非 Windows、非 macOS 行为。

FFI 调用集中在 `play_system_sound` 的 macOS 分支，配套中文函数与安全性注释。因为系统 API 已由目标 SDK 提供，不修改 `Cargo.toml`，也不增加第三方依赖。

## 通知权限与点击激活边界

当前 `tauri-plugin-notification 2.3.3` 的 Desktop `permission_state` / `request_permission` 不提供 macOS 真实授权状态，发送路径也不向应用暴露通知点击 handle。因此自动化测试不能证明以下行为：

- 首次通知是否出现系统授权流程；
- 系统设置中允许、拒绝及恢复后的投递结果；
- 点击通知后应用是否被激活并恢复主窗口。

本任务先保留现有通知实现，通过真实 bundle 人工记录上述结果。若点击激活失败，再以独立失败证据设计通知 handle 或原生 delegate 接入；不在本任务中预先增加兼容层。

## 验证与清单更新

实施阶段遵循 RED→GREEN：先增加仅在 macOS 编译的契约测试，冻结用户首选警告音 ID 和独立开关行为，再增加 AudioToolbox 调用。随后运行相关测试、`cargo fmt --all -- --check`、`cargo check --locked`、`cargo clippy --locked --all-targets -- -D warnings` 和 `cargo test --locked`。

真实 `.app` 验收至少覆盖：仅声音、仅通知、两者同时开启、通知被系统拒绝，以及点击通知时主窗口的激活表现。只有实际获得对应证据时才勾选 `docs/macos-porting-checklist.md` 的声音、权限或点击条目；代码编译成功不能替代真机结果。

## 回滚边界

实现只涉及 `agent_notification.rs` 的 macOS 条件编译分支。若 AudioToolbox 链接或运行验证失败，回滚该分支即可恢复当前基线，不影响通知计划、Windows 行为、配置结构或持久化数据。
