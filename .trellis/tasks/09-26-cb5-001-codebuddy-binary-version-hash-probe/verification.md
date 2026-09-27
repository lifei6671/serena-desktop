# CB5-001 Windows Binary / Version / Hash Evidence

状态：**PARTIAL，等待 Host Gate**。本机 binary identity 采集 PASS；PATH discovery 与 Host 预探测不一致。仅生成 supported-version candidate evidence，不声明 supported，不进入 CB5-002。

## 执行范围及命令

日期：2026-09-26。平台：Windows，Python 3.12.10。所有以下命令 cwd 为 `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`（Python `Path.cwd()` 实测为普通盘符路径）。仅 task-local 文件新增，安装文件只读。主 Agent 已启动现有 task；没有新 task、ACP initialize/session、安装、登录、升级、产品代码变更或 Git commit。

```powershell
python -X utf8 -B .trellis/tasks/09-26-cb5-001-codebuddy-binary-version-hash-probe/test_probe.py 2>&1 | Tee-Object -FilePath .trellis/tasks/09-26-cb5-001-codebuddy-binary-version-hash-probe/test-output.txt
python -B .trellis/tasks/09-26-cb5-001-codebuddy-binary-version-hash-probe/probe.py --shim 'C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\bin\buddycn.cmd'
```

第一条 Python exit=0，**11 tests PASS**，完整输出在 `test-output.txt`。found / missing discovery / missing shim / missing EXE or CLI / malformed shim / empty raw / malformed raw / failed process / explicit fallback source / no base fallback / valid three-line CLI 及 invalid commit、arch、trailing garbage 均被覆盖。fixtures 在测试脚本中生成临时布局，测试结束删除；不执行 fixture EXE。

第二条 exit=1，stdout 原样为 `{"actual_probe_status": "PARTIAL", "entry_status": "FOUND"}` 加 CRLF，stderr 为空。exit=1 显式表示未通过完整发现 Gate。首次无 `--shim` 执行 exit=1，返回 `BLOCKED / NOT_FOUND`；随后使用已独立读取的真实 shim 路径继续采集，未改变 PATH。

`probe-result.json` 保存每条真实子命令的 argv、Windows command_line、cwd、必要环境 override、15 秒 timeout、stdout/stderr/exit，以及输出原始 bytes 的 base64。读取 JSON 元数据及计算 hash 由 Python 标准库在同一次 probe 中执行。未导出完整环境、账号或凭据。测试是 Windows 证据，不包含 Linux 验证；无产品源码修改，产品 lint/build/Rust tests NOT_RUN。

## 发现差异

`where_executable = C:\WINDOWS\system32\where.exe`。

| 实际命令 | exit | stdout（JSON 字符串） | stderr（JSON 字符串） |
| --- | --- | --- | --- |
| `where.exe codebuddy` | 1 | `""` | `"INFO: Could not find files for the given pattern(s).\r\n"` |
| `where.exe buddycn` | 1 | `""` | `"INFO: Could not find files for the given pattern(s).\r\n"` |

当前 process PATH 确含 `C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\bin`，该目录列表确为 `buddycn`、`buddycn.cmd`。Host 预探测报告 `where.exe buddycn` 找到这两个路径；本次没有复现，原因未确定，不能把 Host 的成功发现当成本次 PASS。显式路径来源标记为 `entry_source=explicit_shim_argument`，同时保留 `discovery_status=NOT_FOUND`。

设计 §14.2 的 `codebuddy` 是当前安装不匹配的 placeholder；此 CN 安装使用 `buddycn`（product.json.applicationName 同值）。本卡没有修改产品 discovery。

读取的 `.cmd` 内容解析到：

```text
shim: C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\bin\buddycn.cmd
EXE: C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\CodeBuddy CN.exe
CLI: C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\resources\app\out\cli.js
ELECTRON_RUN_AS_NODE=1
VSCODE_DEV removed
```

三个文件存在。canonical identity 为 EXE + CLI JS；shim 只是入口解析证据，不是 canonical runtime binary。Host 曾报告 `.cmd` 在 MCP 的 `\\?\` cwd 下受到 cmd.exe 行为影响；本次没有执行 shim，不将该历史报告当成本次复现。

## 真实 raw CLI 及版本来源

两个直接入口命令均只在子进程设置 `ELECTRON_RUN_AS_NODE=1` 并清除 `VSCODE_DEV`（与已读取 shim 一致）：

```text
"C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\CodeBuddy CN.exe" "C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\resources\app\out\cli.js" --version
"C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\CodeBuddy CN.exe" "C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\resources\app\out\cli.js" --help
```

`--version` 本次 exit=0、stderr=`""`、未超时，stdout 原样 JSON 字符串为：

```json
"1.106.1\nb4c35ed08ffb428910211608831a314565c1256e\nx64\n"
```

`--help` 本次 exit=0、stderr=`""`、未超时；stdout 非空，以 `CodeBuddy CN 1.106.1\n\nUsage: buddycn.exe [options] [paths...]` 开头，完整原始输出见 `probe-result.json.raw_cli["--help"]`（文本及 base64）。

**Host 预探测报告**：`--version` stdout=`""`、exit=0；`--help` stdout=`""`、exit=0。这一空输出保留为 Host 报告事实，**不是本次独立实测**。本次获得非空输出，不能伪造空输出以匹配预期。Host 调用/cwd/环境与本次的差异是否为原因尚未证实。

parser 只接受完整单行 `major.minor.patch`，或完整三行 `version / 40 lowercase hex commit / x64|arm64|ia32`；允许首尾空白。空、畸形、非零退出、超时均 REJECTED，value=null。实际三行解析为 CLI version `1.106.1`，source=`cli_stdout`。CLI 解析结果始终独立保存，不能自动升级成 CodeBuddy 产品版本。

| 来源 | 实测值 | 使用语义 |
| --- | --- | --- |
| EXE PE ProductName | CodeBuddy CN | 产品归属证据 |
| EXE PE ProductVersion | 4.12.0 | 产品版本候选，source=`pe_product_version` |
| EXE PE FileVersion | 1.106.1.0 | PE 文件/base 版本 |
| CLI --version | 1.106.1 | CLI 报告版本，匹配 embedded/base |
| product.json | applicationName=buddycn; version=1.106.1; quality=stable | embedded/base 元数据 |
| package.json | name=CodeBuddy CN; version=1.106.1 | embedded/base 元数据 |
| product.json commit / CLI 第二行 | b4c35ed08ffb428910211608831a314565c1256e | build provenance |

PE 字段由 `Get-Item -LiteralPath ... .VersionInfo` 实际读取，完整 PowerShell argv、stdout、exit=0 在 JSON 的 `pe_metadata_command`。ProductVersion 的 Windows 字段语义及 ProductName 支持将 `4.12.0` 作为本地产品候选；它不等于兼容性结论。`1.106.1` / `1.106.1.0` 单列为 embedded/VS Code-base 家族证据，不冒充 CodeBuddy product version。

无论 raw CLI 成功还是失败，产品候选仅在 PE ProductName=`CodeBuddy CN` 且 ProductVersion 为合法三段版本时产生，明确 source=`pe_product_version`、supported=false。empty/malformed fixture 验证此显式 metadata fallback 不会覆盖 CLI 的 REJECTED；缺失或畸形 PE 不能用 base version 补齐。

## 候选身份锚点

```json
{
  "productVersion": "4.12.0",
  "source": "pe_product_version",
  "exeSha256": "d289e2a508dece84064ffb3243b817faf9c429877b5f022a726002463b428ed2",
  "cliSha256": "5dd40efc70561675207cde74b2970ac532721944e996a683ea62888bd4a98904",
  "commit": "b4c35ed08ffb428910211608831a314565c1256e",
  "applicationName": "buddycn",
  "supported": false
}
```

独立 SHA256 与 Host 两个 hash 完全一致。`binary.sha256` 另记录真实 `.cmd` shim hash。`probe-result.json` 还记录 product/package JSON hash 以绑定元数据。脚本及证据冻结摘要见 `evidence.sha256`，测试内联 fixtures 随 `test_probe.py` 冻结。

剩余不确定性：PATH discovery 差异、Host 空 stdout 未复现、PE 产品版本与未来 ACP 服务版本关系尚未验证。未测试 ACP、协议版本或能力；没有改 supported-version table。产品文件不变由主 Agent 的 task-local `product-baseline.json` 和最终比对确认。停止于 CB5-001，等待 Host Gate。
