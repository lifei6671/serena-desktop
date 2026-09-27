"""下载辅助脚本异常 cleanup 回归，不访问网络或用户进程。"""
import subprocess
import unittest
from unittest.mock import Mock, patch
import cargo_bootstrap


class CleanupTests(unittest.TestCase):
    """注入 OS 拒绝/超时，确保后续 kill/wait 不被跳过。"""

    def test_tree_timeout_still_kills_and_waits(self):
        """树命令 timeout 后直接回收仍执行。"""
        process = Mock(pid=123)
        process.poll.return_value = None
        with patch.object(cargo_bootstrap.subprocess, 'run', side_effect=subprocess.TimeoutExpired('taskkill', 10)):
            errors = cargo_bootstrap.cleanup_cargo(process)
        process.kill.assert_called_once()
        process.wait.assert_called_once_with(timeout=5)
        self.assertIn('tree_cleanup=TimeoutExpired', errors)

    def test_kill_failure_still_waits_and_reports(self):
        """kill 失败与 wait 失败分别记录，不伪造成功。"""
        process = Mock(pid=123)
        process.poll.return_value = None
        process.kill.side_effect = OSError('denied')
        process.wait.side_effect = subprocess.TimeoutExpired('cargo', 5)
        with patch.object(cargo_bootstrap.subprocess, 'run', side_effect=OSError('denied')):
            errors = cargo_bootstrap.cleanup_cargo(process)
        process.wait.assert_called_once_with(timeout=5)
        self.assertEqual(len(errors), 3)
        self.assertIn('direct_wait=TimeoutExpired', errors)


if __name__ == '__main__':
    unittest.main()
