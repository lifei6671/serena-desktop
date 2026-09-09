use rmcp::{
    model::{Tool, ToolAnnotations},
    schemars::{self, JsonSchema},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceArgs {
    #[serde(default)]
    pub relative_path: Option<String>,
    #[serde(default)]
    pub start_line: Option<u32>,
    #[serde(default)]
    pub end_line: Option<u32>,
    #[serde(default)]
    pub recursive: Option<bool>,
    #[serde(default)]
    pub file_mask: Option<String>,
    #[serde(default)]
    pub substring_pattern: Option<String>,
    #[serde(default)]
    pub name_path_pattern: Option<String>,
    #[serde(default)]
    pub name_path: Option<String>,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default)]
    pub include_body: Option<bool>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActivateArgs {
    pub id: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphArgs {
    pub query: String,
    #[serde(rename = "maxFiles")]
    pub max_files: Option<u32>,
}
pub const SOURCES: &[(&str, &str, &[&str], &[&str])] = &[
    (
        "source_read_file",
        "read_file",
        &["relative_path", "start_line", "end_line", "max_bytes"],
        &["relative_path"],
    ),
    (
        "source_list_dir",
        "list_dir",
        &["relative_path", "recursive", "max_bytes"],
        &["relative_path"],
    ),
    (
        "source_find_file",
        "find_file",
        &["relative_path", "file_mask", "max_bytes"],
        &["file_mask"],
    ),
    (
        "source_search_pattern",
        "search_for_pattern",
        &["relative_path", "substring_pattern", "max_bytes"],
        &["substring_pattern"],
    ),
    (
        "source_symbols_overview",
        "get_symbols_overview",
        &["relative_path", "depth", "max_bytes"],
        &["relative_path"],
    ),
    (
        "source_find_symbol",
        "find_symbol",
        &[
            "relative_path",
            "name_path_pattern",
            "depth",
            "include_body",
            "max_bytes",
        ],
        &["name_path_pattern"],
    ),
    (
        "source_find_references",
        "find_referencing_symbols",
        &["relative_path", "name_path", "max_bytes"],
        &["relative_path", "name_path"],
    ),
];
pub const GITS: &[&str] = &[
    "git_status",
    "git_diff",
    "git_log",
    "git_show",
    "git_branch",
    "git_worktree_list",
];
fn git_fields(name: &str) -> &[&str] {
    match name {
        "git_diff" => &["scope", "path", "max_bytes"],
        "git_log" => &["reference", "path", "count", "max_bytes"],
        "git_show" => &["reference", "path", "max_bytes"],
        _ => &["max_bytes"],
    }
}
fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap()
}
fn tool(name: &'static str, desc: &'static str, value: Value) -> Tool {
    let mut t = Tool::new(name, desc, value.as_object().unwrap().clone());
    t.annotations = Some(ToolAnnotations::default().read_only(!matches!(
        name,
        "workspace_activate" | "workspace_deactivate"
    )));
    let workspace = json!({"type":"object", "properties":{"id":{"type":"string"},"name":{"type":"string"},"root":{"type":"string"}}, "required":["id","name","root"]});
    let mut output = json!({"type":"object","properties":{"truncated":{"type":"boolean"}},"required":["truncated"]});
    if name == "workspace_list" {
        output["properties"]["workspaces"] = json!({"type":"array","items":workspace});
        output["required"] = json!(["workspaces", "truncated"]);
    } else if name.starts_with("workspace_") {
        output["properties"]["activeWorkspace"] = json!({"anyOf":[workspace,{"type":"null"}]});
        output["properties"]["status"] = json!({"type":"string"});
        output["required"] = json!(["activeWorkspace", "truncated"]);
    } else {
        output["properties"]["workspace"] = workspace;
        output["properties"]["text"] = json!({"type":"string"});
        output["properties"]["hint"] = json!({"type":"string"});
        output["required"] = json!(["workspace", "text", "truncated"]);
    }
    if name == "codegraph_explore" {
        output = json!({"type":"object", "oneOf":[output, {
            "type":"object", "required":["error"], "properties":{"error":{
                "type":"object", "required":["code","message","workspace","recoverable"],
                "properties":{"code":{"type":"string"},"message":{"type":"string"},
                    "workspace":{"anyOf":[{"type":"object","required":["id","name"],"properties":{"id":{"type":"string"},"name":{"type":"string"}}},{"type":"null"}]},
                    "recoverable":{"type":"boolean"}}
            }}
        }]});
    }
    t.output_schema = Some(output.as_object().unwrap().clone().into());
    t
}
pub fn list(upstream: &[Tool]) -> Result<Vec<Tool>, String> {
    let mut list = vec![
        tool(
            "workspace_list",
            "【做什么】\n列出 Desktop 已从 Serena 同步的项目，返回项目 ID、名称和根目录。\n\n【什么时候使用】\n查找可激活的项目，或在调用 workspace_activate 前获取项目 ID。\n\n【关键约束】\n只读取已同步列表，不扫描目录、不初始化项目，也不切换当前活动项目。新项目需先在 Serena 初始化，再由 Desktop 同步。",
            schema::<Empty>(),
        ),
        tool(
            "workspace_current",
            "【做什么】\n查询所有客户端共享的当前活动项目，返回其 ID、名称和根目录；没有有效活动绑定时返回 null。\n\n【什么时候使用】\n执行 source_* 或 git_* 操作前确认目标项目，或检查切换后的共享工作区。\n\n【关键约束】\n只读查询，不激活项目。活动状态由所有客户端共享，其他客户端可能切换或取消该绑定。",
            schema::<Empty>(),
        ),
        tool(
            "workspace_activate",
            "【做什么】\n激活一个已登记项目，并将其设置为所有客户端共享的当前活动项目。\n\n【什么时候使用】\n当后续 source_* 或 git_* 操作需要切换到指定项目时使用。\n\n【关键约束】\n这是全局共享状态变更，会影响所有连接到本服务的客户端。id 必须来自 workspace_list；项目须已初始化且 Serena 正在运行。切换失败可能清空活动绑定，应查询 workspace_current 后重新激活。",
            schema::<ActivateArgs>(),
        ),
        tool(
            "workspace_deactivate",
            "【做什么】\n取消 Broker 中所有客户端共享的当前活动项目绑定。\n\n【什么时候使用】\n结束当前工作区的使用，或希望后续操作必须先明确激活项目时使用。\n\n【关键约束】\n影响所有客户端；取消后 source_* 和 git_* 不可用，直到重新激活。不会停止 Serena 或 Broker，也不会删除项目文件、配置或登记信息。",
            schema::<Empty>(),
        ),
    ];
    for &(name, remote_name, allowed, required) in SOURCES {
        let mut s = schema::<SourceArgs>();
        s["properties"]
            .as_object_mut()
            .unwrap()
            .retain(|key, _| allowed.contains(&key.as_str()));
        s["required"] = json!(required);
        let remote = upstream
            .iter()
            .find(|t| t.name == remote_name)
            .ok_or_else(|| format!("BACKEND_INCOMPATIBLE: missing {remote_name}"))?;
        let mut source = tool(name, "", s);
        // Preserve the upstream description exactly, including whitespace and absence.
        source.description = remote.description.clone();
        list.push(source);
    }
    for &name in GITS {
        let mut s = schema::<super::git::GitArgs>();
        s["properties"]
            .as_object_mut()
            .unwrap()
            .retain(|key, _| git_fields(name).contains(&key.as_str()));
        let description = match name {
            "git_status" => {
                "【做什么】\n查看当前活动仓库的工作区状态，包括分支、已暂存、未暂存和未跟踪文件。\n\n【什么时候使用】\n开始修改前确认工作区状态，或检查哪些文件需要审查、暂存或提交。\n\n【关键约束】\n必须先激活项目；只读，不暂存或修改文件。返回 Git porcelain v1 格式。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_diff" => {
                "【做什么】\n查看当前活动仓库已跟踪文件的差异，可按相对路径过滤。\n\n【什么时候使用】\n审查未暂存修改、待提交修改，或比较整个工作区与 HEAD 的差异。\n\n【关键约束】\n必须先激活项目；只读。scope 为 unstaged（默认，工作区对暂存区）、staged（暂存区对 HEAD）或 all（工作区对 HEAD），不包含未跟踪文件内容。path 相对仓库根且不得越界。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_log" => {
                "【做什么】\n查看当前活动仓库的提交历史，返回提交哈希、时间和标题，可按引用和相对路径筛选。\n\n【什么时候使用】\n追踪某个文件的变更历史，寻找相关提交，或了解最近的开发记录。\n\n【关键约束】\n必须先激活项目；只读。reference 默认 HEAD；count 默认 20，范围 1–100。path 相对仓库根且不得越界。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_show" => {
                "【做什么】\n查看当前活动仓库指定 Git 引用的内容，例如提交详情和补丁，或通过 HEAD:相对路径读取历史文件。\n\n【什么时候使用】\n深入检查某次提交，或读取指定版本中的文件内容。\n\n【关键约束】\n必须先激活项目；只读。reference 默认 HEAD；path 可过滤提交涉及的路径，相对仓库根且不得越界。不会切换分支或检出文件。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_branch" => {
                "【做什么】\n列出当前活动仓库的本地分支及 Git 的当前分支标记。\n\n【什么时候使用】\n确认当前分支，或查看仓库中已有的本地分支。\n\n【关键约束】\n必须先激活项目；只读，不创建、删除或切换分支，不列出远程跟踪分支。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_worktree_list" => {
                "【做什么】\n列出当前活动仓库关联的 Git 工作树及其路径、HEAD 和分支信息。\n\n【什么时候使用】\n确认同一仓库有哪些工作树，或检查分支与工作目录的对应关系。\n\n【关键约束】\n必须先激活项目；只读，返回 Git porcelain 格式。不创建、删除或切换工作树，也不将这些路径自动登记或激活。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            _ => unreachable!("GITS contains only locally implemented tools"),
        };
        list.push(tool(name, description, s));
    }
    list.push(tool(
        "codegraph_explore",
        "【做什么】\n探索当前活动 Workspace 的代码结构、跨函数/文件/模块调用路径、依赖关系和潜在影响范围。\n\n【什么时候使用】\n理解功能或模块如何工作、追踪完整调用链、分析修改影响或探索架构。已知文件路径、Symbol 名称，或只需读文件、直接 references 时优先使用 source_*。\n\n【关键约束】\n只查询当前活动项目；query 必填，maxFiles 默认 12。不接受 projectPath，不跨项目回退，不自动初始化索引。保留索引陈旧提示。启动中返回 CODEGRAPH_STARTING；运行故障每次调用最多恢复一次、查询最多重试一次，恢复有 30 秒冷却。错误以 error.code/message/workspace/recoverable 返回。",
        schema::<GraphArgs>(),
    ));
    let mut media = tool(
        "media_read_image",
        "【做什么】\nRead an image from the active workspace and return it as MCP image content for visual inspection.\n\n【什么时候使用】\n查看当前项目中的截图或图片。\n\n【关键约束】\n仅支持 Workspace 相对路径；只读 PNG/JPEG/WebP。输入最多 20 MiB、40 MP，最长边缩至 2560 px，重编码并剥离原始 metadata；输出最多 6 MiB。WebP 返回 PNG。",
        schema::<super::media::MediaReadImageArgs>(),
    );
    media.output_schema = None;
    list.push(media);
    Ok(list)
}
pub fn validate(name: &str, args: &Value) -> Result<(), String> {
    if let Some((_, _, allowed, required)) = SOURCES.iter().find(|t| t.0 == name) {
        let object = args.as_object().ok_or("INVALID_PARAMS: 参数必须是对象")?;
        if object.keys().any(|k| !allowed.contains(&k.as_str()))
            || required
                .iter()
                .any(|k| !object.contains_key(*k) || object[*k].is_null())
        {
            return Err("INVALID_PARAMS: 缺少必要参数或存在未公开参数".into());
        }
        serde_json::from_value::<SourceArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if GITS.contains(&name) {
        if args
            .as_object()
            .ok_or("INVALID_PARAMS")?
            .keys()
            .any(|k| !git_fields(name).contains(&k.as_str()))
        {
            return Err("INVALID_PARAMS: 此工具不支持该参数".into());
        }
        serde_json::from_value::<super::git::GitArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if name == "media_read_image" {
        serde_json::from_value::<super::media::MediaReadImageArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if name == "codegraph_explore" {
        let parsed = serde_json::from_value::<GraphArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
        if parsed.query.trim().is_empty() || parsed.max_files == Some(0) {
            return Err("INVALID_PARAMS: query 不能为空，maxFiles 必须为正整数".into());
        }
    } else if name == "workspace_activate" {
        let parsed = serde_json::from_value::<ActivateArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
        if parsed.id.is_empty() {
            return Err("INVALID_PARAMS: id 不能为空".into());
        }
    } else if matches!(
        name,
        "workspace_list" | "workspace_current" | "workspace_deactivate"
    ) {
        serde_json::from_value::<Empty>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else {
        return Err("UNKNOWN_TOOL".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn upstream() -> Vec<Tool> {
        SOURCES.iter().map(|(_, name, _, _)| {
            Tool::new(*name, format!("  Original {name}\n\nDetailed usage, constraints, and examples.\n中文说明。\n"), serde_json::Map::new())
        }).collect()
    }
    #[test]
    fn fixed_surface() {
        let tools = list(&upstream()).unwrap();
        assert_eq!(tools.len(), 19);
        let mut expected = vec![
            "workspace_list",
            "workspace_current",
            "workspace_activate",
            "workspace_deactivate",
            "codegraph_explore",
            "media_read_image",
        ];
        expected.extend(SOURCES.iter().map(|s| s.0));
        expected.extend(GITS.iter().copied());
        let names: std::collections::HashSet<_> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, expected.into_iter().collect());
        let media = tools.iter().find(|t| t.name == "media_read_image").unwrap();
        assert!(media.output_schema.is_none());
        assert_eq!(
            media.annotations.as_ref().unwrap().read_only_hint,
            Some(true)
        );
        assert_eq!(media.input_schema["additionalProperties"], false);
        assert_eq!(media.input_schema["required"], json!(["path"]));
        assert_eq!(
            media.input_schema["properties"],
            json!({"path":{"type":"string","description":"Image path relative to the active workspace root."}})
        );
        assert!(validate("media_read_image", &json!({"path":"a.png"})).is_ok());
        for args in [
            json!({}),
            json!({"path":null}),
            json!({"path":3}),
            json!({"path":"a.png","root":"x"}),
            json!({"path":"a.png","workspaceId":"x"}),
            json!({"path":"a.png","absolutePath":"x"}),
        ] {
            assert!(validate("media_read_image", &args).is_err());
        }
        assert_eq!(
            tools
                .iter()
                .map(|t| &t.name)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            19
        );
        assert!(
            validate(
                "source_read_file",
                &json!({"relative_path":"x","projectPath":"elsewhere"})
            )
            .is_err()
        );
        assert!(validate("workspace_deactivate", &json!({})).is_ok());
        let graph = tools
            .iter()
            .find(|t| t.name == "codegraph_explore")
            .unwrap();
        assert!(
            graph.input_schema["properties"]
                .get("projectPath")
                .is_none()
        );
        assert!(
            validate(
                "codegraph_explore",
                &json!({"query":"symbol", "maxFiles":2})
            )
            .is_ok()
        );
        for args in [
            json!({}),
            json!({"query":" "}),
            json!({"query":"x","maxFiles":0}),
            json!({"query":"x","projectPath":"elsewhere"}),
        ] {
            assert!(validate("codegraph_explore", &args).is_err());
        }
    }

    #[test]
    fn forwarded_descriptions_are_exact_and_local_descriptions_have_three_sections() {
        let upstream = upstream();
        let tools = list(&upstream).unwrap();
        for (name, remote, _, _) in SOURCES {
            let exposed = tools.iter().find(|t| t.name == *name).unwrap();
            let original = upstream.iter().find(|t| t.name == *remote).unwrap();
            assert_eq!(exposed.description, original.description, "{name}");
        }
        for tool in tools.iter().filter(|t| !t.name.starts_with("source_")) {
            let description = tool.description.as_deref().unwrap();
            let sections: Vec<_> = description
                .lines()
                .filter(|line| line.starts_with('【'))
                .collect();
            assert_eq!(
                sections,
                ["【做什么】", "【什么时候使用】", "【关键约束】"],
                "{}",
                tool.name
            );
        }
        let activate = tools
            .iter()
            .find(|t| t.name == "workspace_activate")
            .unwrap();
        assert!(
            activate
                .description
                .as_deref()
                .unwrap()
                .contains("这是全局共享状态变更，会影响所有连接到本服务的客户端。")
        );
    }

    #[test]
    fn missing_upstream_tool_fails_instead_of_using_a_placeholder() {
        assert!(list(&[]).unwrap_err().contains("missing read_file"));
        let mut upstream = upstream();
        upstream[0].description = None;
        let tools = list(&upstream).unwrap();
        assert!(
            tools
                .iter()
                .find(|t| t.name == "source_read_file")
                .unwrap()
                .description
                .is_none()
        );
    }
}
