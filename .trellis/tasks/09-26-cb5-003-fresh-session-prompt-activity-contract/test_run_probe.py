"""仅用 fake subprocess 回归 runner，不启动真实 CLI 或读取登录态。"""
import contextlib
import copy
import io
import json
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent
runner = types.ModuleType("probe_under_test")
runner.__file__ = str(ROOT / "run_probe.py")
exec(compile((ROOT / "run_probe.py").read_text(encoding="utf-8"), runner.__file__, "exec"), runner.__dict__)


def result(success=False):
    """构造受控 HTTP500 或成功合同结果，身份保持可观测。"""
    return {
        "scenario": "read", "attempt": 1, "argv": ["fake"], "cwd": "fake-cwd", "pid": 100,
        "environmentChanges": {}, "cleanup": {"succeeded": True, "directChildReaped": True, "errors": []},
        "workspaceDeleted": True, "workspaceDeleteError": None, "elapsedMs": 1, "harnessExitCode": 0,
        "before": {}, "after": {}, "sessionNew": {"response": {"message": {"error": {"data": {"details": "Request failed with status code 500"}}}}},
        "sdkTyped": {"stage": "terminal" if success else "session/new"},
        "canExecuteEvidence": success, "protocolSucceeded": success, "error": None if success else "HTTP500",
        "delta": [], "prompt": {"request": None, "response": None}, "wire": [], "updates": [],
        "conversationRequestId": None, "sessionId": "fake-session" if success else None,
    }


class RunnerTests(unittest.TestCase):
    """所有输出只写本测试创建的临时目录。"""

    def setUp(self):
        """隔离 evidence 与进程调用计划。"""
        self.temp = tempfile.TemporaryDirectory(prefix="cb5-003-runner-test-")
        self.addCleanup(self.temp.cleanup)
        self.evidence = Path(self.temp.name)
        self.commands = []
        self.plan = []

    def spawn(self, command, **_kwargs):
        """每次 fake harness 只可向指定的新输出路径写入本次结果。"""
        self.commands.append(command)
        payload, exit_code = self.plan.pop(0)
        output = Path(command[2])
        self.assertFalse(output.exists())

        class Process:
            """最小 Popen 契约，无任何 OS 子进程。"""
            returncode = exit_code
            stdout = io.StringIO()
            stderr = io.StringIO()

            def communicate(self, timeout):
                """模拟当前进程可正常写结果，也可异常退出不产出。"""
                if payload is not None:
                    output.write_text(json.dumps(payload), encoding="utf-8")
                return "", ""

        return Process()

    def run_main(self, summarize=False):
        """替换唯一 subprocess 边界并捕获无关 stdout。"""
        args = ["probe", "--summarize"] if summarize else ["probe"]
        with patch.object(runner, "EVIDENCE", self.evidence), patch.object(runner.sys, "argv", args), patch.object(runner.subprocess, "Popen", side_effect=self.spawn), contextlib.redirect_stdout(io.StringIO()):
            return runner.main()

    def test_missing_second_result_cannot_reuse_first(self):
        """第一次500有结果，第二次退出1无结果，不能伪造 attempt2。"""
        self.plan = [(result(), 0), (None, 1)]
        with self.assertRaisesRegex(RuntimeError, "attempt=2.*exit=1.*evidence missing"):
            self.run_main()
        self.assertEqual(len(self.commands), 2)
        self.assertNotEqual(self.commands[0][2], self.commands[1][2])
        self.assertFalse((self.evidence / "read-attempt-2.json").exists())
        failure = json.loads((self.evidence / "read-attempt-2-failure.json").read_text())
        self.assertEqual(failure["diagnostic"], "CURRENT_ATTEMPT_RESULT_MISSING")
        self.assertEqual(failure["harnessExitCode"], 1)
        self.assertNotIn("wire", failure)

    def test_cleanup_or_exit_failure_stops_before_retry(self):
        """清理、回收、删除失败或非零退出均不可启动后续进程。"""
        variants = []
        for field in ["succeeded", "directChildReaped"]:
            report = result(); report["cleanup"][field] = False
            variants.append((report, 0))
        report = result(); report["workspaceDeleted"] = False
        variants.extend([(report, 0), (result(), 1)])
        for payload, code in variants:
            with self.subTest(payload=payload, code=code):
                self.commands = []; self.plan = [(payload, code), (result(True), 0)]
                self.assertEqual(self.run_main(), 1)
                self.assertEqual(len(self.commands), 1)
                status = json.loads((self.evidence / "runner-result.json").read_text())
                self.assertFalse(status["allAttemptsClean"])

    def test_summary_cannot_hide_earlier_cleanup_failure(self):
        """最终场景成功也不能覆盖前一尝试的清理失败。"""
        first = result(); first["cleanup"]["succeeded"] = False
        read = result(True); read["attempt"] = 2
        write = result(True); write["scenario"] = "write"
        for name, payload in [("read-attempt-1.json", first), ("read-attempt-2.json", read), ("write-attempt-1.json", write), ("read-only-result.json", read), ("isolated-write-result.json", write)]:
            (self.evidence / name).write_text(json.dumps(payload), encoding="utf-8")
        self.assertEqual(self.run_main(summarize=True), 1)
        self.assertEqual(self.commands, [])
        status = json.loads((self.evidence / "runner-result.json").read_text())
        self.assertEqual(status["attemptCount"], 3)
        self.assertFalse(status["allAttemptsClean"])

    def test_clean_retry_can_succeed(self):
        """只有先前进程清理成功，500才允许一次新尝试并汇总成功。"""
        second = result(True); second["pid"] = 101; second["cwd"] = "second-cwd"
        self.plan = [(result(), 0), (second, 0), (result(True), 0)]
        self.assertEqual(self.run_main(), 0)
        self.assertEqual(len(self.commands), 3)
        first = json.loads((self.evidence / "read-attempt-1.json").read_text())
        second = json.loads((self.evidence / "read-attempt-2.json").read_text())
        self.assertNotEqual(first["pid"], second["pid"])
        self.assertEqual(first["attempt"], 1)
        self.assertEqual(second["attempt"], 2)

    def test_http500_retry_limit_is_one_per_scenario(self):
        """重复500每场景最多两次，且每次输出路径全新。"""
        self.plan = [(copy.deepcopy(result()), 0) for _ in range(4)]
        self.assertEqual(self.run_main(), 1)
        self.assertEqual([cmd[1] for cmd in self.commands], ["read", "read", "write", "write"])
        self.assertEqual(len({cmd[2] for cmd in self.commands}), 4)


if __name__ == "__main__":
    unittest.main()
