# Incremental independent review

用户授权极窄Host-orchestration timeout repair。原usage CommandRun 30s timeout，旧sentinel不可改，Host已确认child全清理。该事件不是Provider结果。

新增唯一usage-repair模式，gate严格要求old sentinel exists、old result absent、HOST_COMMAND_TIMEOUT；repair sentinel create_new/fsync，无其他output或force参数。复用原scenario/analysis，fresh root/new Session；只修改report/analysis scenario label和固定输出文件名。再次失败不能新增第三mode。README命令层timeout>=300000ms，推荐420000ms。

只读审查usage-repair-review-target.json的改动及必要上下文；原usage_runtime完整前缀hash不变，旧evidence全部hash不变，见scope与baseline。主实现4文件变化：main、usage_runtime追加、usage_tests追加、usage-README追加。保留既有Stage A/B review记录。独立reviewer不运行binary/真实CLI、不写文件，不扩大协议/进程树scope。

验证18 usage tests/build/fmt/diff PASS，真实repair NOT_RUN。返回matching targetId、覆盖和P0/P1/P2/gate。aggregate仍是SHA256(canonical JSON files sort_keys=True,separators=(',',':'))。
