"""只测纯函数与本地 sentinel，绝不启动真实 CLI。"""
import importlib.util
from pathlib import Path
import tempfile
import unittest
spec = importlib.util.spec_from_file_location("runner", Path(__file__).with_name("run_host_cancel_permission.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

class HostRunnerTests(unittest.TestCase):
    """重复执行与伪成功均被阻止。"""
    def test_sentinel_blocks_replay(self):
        """第一次占用 durable，第二次不能覆盖。"""
        with tempfile.TemporaryDirectory() as root:
            path = runner.reserve(Path(root), "before")
            previous = path.read_bytes()
            with self.assertRaises(FileExistsError):
                runner.reserve(Path(root), "before")
            self.assertEqual(path.read_bytes(), previous)

    def test_exit_zero_or_terminal_without_action_is_not_pass(self):
        """退出或 terminal 本身不证明 cancel/deny。"""
        for scenario in ["before", "after", "permission"]:
            self.assertEqual(runner.acceptance({"providerTerminalReceived": True}, scenario), "PARTIAL")

    def test_runtime_timeout_is_not_provider_terminal(self):
        """明确 timeout+回收可以收敛，但不会生成 terminal。"""
        report = {"protocol": {"stage": "no_terminal_timeout"}, "cleanup": {"succeeded": True, "directChildReaped": True}, "workspaceDeleted": True, "cancel": {"sequence": 5}, "delta": []}
        self.assertEqual(runner.acceptance(report, "before"), "PASS")
        self.assertNotIn("providerTerminalReceived", report)

if __name__ == "__main__":
    unittest.main()
