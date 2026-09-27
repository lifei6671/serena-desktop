# Stage B — Usage Contract

授权：只扩展 usage mode，不改/重跑 Stage A resume/load，不做 Crash，不修改生产源码或数据库，不提交。沿用现有 task，保持 in_progress。Stage A Host PASS 是前提，普通 Continue candidate=session/resume，无 load fallback。

## 执行合同

R1 direct installed launcher / initialize / new S1 / P1 read-only terminal / P2 read-only terminal / reap；R2 fresh process / initialize / typed resume exact S1/cwd / P3 read-only terminal / reap。全程 same ordinary fresh temp cwd、标准 Host env，不保存 env。各 terminal 后有界250ms接收窗口，所有 observed frames 按 wire sequence + runtime ordinal 记录。失败 resume 不发送 P3。

新增 usage.rs 安全投影/collector/analyzer、usage_runtime.rs 生命周期、usage_tests.rs/fake_usage_peer.py。Stage A runtime.rs/evidence.rs/transport.rs与真实continuation evidence保持原字节。main只增加usage分支，参数测试只替换已合法usage为非法crash测试值。

## 证据合同

固定 evidence/usage/usage.attempt-started.json、usage.result.json、usage-analysis.json。sentinel create_new+fsync，不支持force/retry/output。usage结果即使PARTIAL也生成分析；缺失结果或部分落盘不允许重跑。

session/update必须通过官方SessionNotification typed envelope和exact session identity；usage_update额外匹配typed UsageUpdate。原wire仅内存：usage_update的安全number/bool/null可投影，session_info和PromptResponse仅投影usage/token/context/cost相关字段。字符串无值；对象仅结构和递归安全叶子；数组仅结构/元素数，不保存内容。未知字段名用hash路径，已知语义键保留原名；secret/auth/credential/providerData/env/content等字段及其子树无数值。固定语义allowlist识别token breakdown，不因名字包含token就认为累计token。

phaseAtReceipt记录new/P1/P2/resume/P3/late。存在旧turn correlation时另外标记late及归属；无correlation只声明window_only，不将跨turn时间窗口当精确token归因。terminal用RPC id匹配。agent答案只保存hash/length，并记录是否完整归因。

analysis分observations与interpretation：raw safe snapshots、used/size顺序和下降/归零、size稳定/变化、显式breakdown/cost字段、terminal前末帧/late帧、R1末快照与R2首快照关系。只比较大小/相等，不计算token delta，不从prompt/answer长度或cost推导token。零值不覆盖lastPositiveSnapshot；最新零帧仍保留。官方used/size语义记录为context occupancy gauge；Provider显式breakdown即使出现，若跨turn/restart语义尚未Host冻结仍建议token_usage=false/public Usage unknown，不自动广告能力。

## 验证与交付

只运行usage_* deterministic tests（不重跑Stage A场景）、Windows build/fmt；保存本轮基线、scope校验、独立review target。Host真实usage始终NOT_RUN，完成后交唯一Host命令并停Host Gate。
