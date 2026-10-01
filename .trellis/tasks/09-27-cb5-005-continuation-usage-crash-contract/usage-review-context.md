# Stage B independent review context

用户最新授权只做Usage；Stage A Host PASS已实测核对并冻结。不运行真实CodeBuddy，不做Crash，不改生产、不提交。task-local既有harness新增usage模式。code-delivery-review，Tier3协议/数据脱敏/进程持久边界，独立只读review。

交付文件见usage-review-target.json，canonical JSON(files,sort_keys=True,separators=(',',':'))的SHA256为targetId。Stage A runtime/evidence/transport未改，可作为复用上下文；scope报告已做baseline hash核对及main反向还原hash验证。旧review-target保留为Stage A历史，不替换成当前binary。

要求映射：numeric/secret白名单->usage::project；typed envelope/session/phase/RPC->usage::collect；observation vs interpretation/reset/size/breakdown/cost->usage::analyze；same S1/cwd、no fallback、failed resume no P3、三turn只读、bounded cleanup->usage_runtime；fixed sentinel->usage_runtime::host_run；14 usage_*测试与build/fmt evidence见usage-verification。README承诺保守token_usage=false，因为跨turn/restart语义仍待Host冻结；不伪造真实合同PASS。

只读全scope，不运行主binary/真实CLI，不执行生成文件检查，不修改任何文件。返回matching targetId、完整覆盖、P0/P1/P2及PASSED/BLOCKED/UNAVAILABLE。缺陷必须有具体触发路径；不因已明确排除Job/tree安全而扩大实现范围。
