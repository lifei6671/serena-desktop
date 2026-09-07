# UI 生成记录

## 当前修订提示词

方式：内置 image_gen 编辑上一版图片。当前图片以本段提示词为准，下面初始提示词仅保留生成来源。

Edit this exact UI board, preserving all layout, icons, typography and both windows. Only change these texts: in upper Serena card replace 项目配置与索引已就绪 with 项目配置已加载; in lower dialog information strip replace 将创建 Serena 项目配置与索引，保留已有配置。 with 将创建 Serena 项目配置，保留已有配置。 Add one subtle small helper line below this strip: 预建索引可稍后执行，不影响激活。 Keep 添加并初始化激活 and 取消激活 buttons unchanged. No other changes.

方式：内置 image_gen；基于用户提供的 Serena Desktop UI 图片编辑。
用途：技术方案的视觉参考，不代表实际运行状态或已实现页面。

## 完整提示词

Edit target: supplied Serena Desktop screenshot. Preserve its polished Windows desktop visual style, purple Serena logo, white/light blue palette, blue buttons, left navigation 首页 设置 日志, Chinese typography. Produce one high resolution UI design board with TWO complete desktop windows stacked vertically, readable Chinese, each 1536x900 approximately. Top window: active project daily dashboard. Header Serena Desktop, no generic duplicate running indicators. Main title 当前项目. Project row dropdown veyra with path E:/projects/veyra and green 已激活 badge; outline button 添加项目; secondary button 取消激活. Helper 当前活动项目：veyra. Three cards MCP Broker / 监听中; Serena / 项目已就绪; Git / 可用. No CodeGraph and no invented backend versions. Connection section 连接配置, label 本机 MCP 地址, http://127.0.0.1:9120/mcp copy icon, helper 用于配置 Cloudflare 上游服务. Lower window: same app with 添加项目 dialog open. Background current project area says 尚未激活项目. Dialog title 添加项目. Fields 项目名称 with guard-wall, 项目目录 E:/projects/guard-wall and 选择文件夹 button. Status Git 仓库有效 and 尚未初始化. Hint 将创建 Serena 项目配置与索引，保留已有配置。 Buttons 仅添加 (secondary) and 添加并初始化激活 (primary). Dialog X close. Both windows app version v0.3.0 subtle bottom left. Do not add workflow diagrams or spurious widgets. This is a UI concept, no code.
