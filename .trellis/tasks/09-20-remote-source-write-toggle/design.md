# Remote Source Write 开关设计

## 边界

配置为 `ManagerConfig.remote_source_write_enabled`，通过现有 serde camelCase 映射为
`remoteSourceWriteEnabled`。`Default` 和 serde 的缺省补全共同保证旧配置为关闭。

## 生命周期

`Broker::config()` 每次从 Supervisor 快照读取配置。创建 Server Handler 时，Registry
以该快照决定 Source Write 的 descriptor 是否可见；每次 `tools/call` 在 Dispatcher
再次读取配置做授权。这样保存后的新连接得到新 catalog，旧 catalog 调用在关闭后仍被
拒绝；不需要 session、事件总线或 list_changed。

## UI

在现有 Settings 页面复用本地 `saveToggle` / `saveConfig` 路径。开关说明依据当前值切换：
关闭时说明只读，开启时说明重新连接后生效。

## 验证

将 Registry、Dispatcher、配置 round-trip 和 UI interaction 覆盖加入现有测试位置；
保持 Source Write Handler 模块完全不改。
