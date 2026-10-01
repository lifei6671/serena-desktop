"""Windows PATH解析的fake回归，不启动npx/模型。"""
import ntpath
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import path_resolver_probe as probe


def filesystem(paths):
    """Windows大小写不敏感的fake文件集合。"""
    known = {ntpath.normcase(path) for path in paths}
    return lambda path: ntpath.normcase(path) in known


class ResolverTests(unittest.TestCase):
    """用户指定的launcher归因、NVM候选、源顺序与可执行扩展名。"""

    def test_wrong_roaming_without_node_is_launcher_failure(self):
        """Roaming wrapper无同目录node且PATH无Node，不能记成ACPfailure。"""
        wrong = r"C:\Users\fixture\AppData\Roaming\npm\npx.cmd"
        r = probe.launcher_resolution(wrong, [r"C:\Windows\System32"], filesystem([wrong]))
        self.assertEqual(r["status"], "LAUNCHER_DEPENDENCY_UNAVAILABLE")
        self.assertEqual(r["failureLayer"], "launcher")
        self.assertFalse(r["acpFailure"])
        self.assertIsNone(r["resolvedNodePath"])

    def test_correct_nvm_candidates(self):
        """指定NVM npx及同目录node，同时出现在child PATH首选。"""
        node = r"C:\nvm4w\nodejs\node.exe"
        exists = filesystem([probe.NPX_PATH, node])
        entries = probe.path_entries(probe.NODE_DIRECTORY, "", "", "%NVM_SYMLINK%", [], {"NVM_SYMLINK": probe.NODE_DIRECTORY})
        r = probe.launcher_resolution(probe.NPX_PATH, entries, exists)
        self.assertEqual(r["status"], "READY")
        self.assertEqual(r["resolvedNpxPath"], probe.NPX_PATH)
        self.assertEqual(r["resolvedBareNpxPath"], probe.NPX_PATH)
        self.assertEqual(r["resolvedNodePath"], node)
        self.assertTrue(r["siblingNodeExists"])
        self.assertTrue(r["childPathContainsNvmSymlink"])

    def test_source_priority_registry_expansion_and_dedup(self):
        """explicit→process→HKCU→HKLM→common；展开变量并去除大小写重复。"""
        entries = probe.path_entries(r"C:\Explicit", r"c:\explicit;C:\Process", r"%NVM_HOME%;C:\User", r"%nvm_symlink%;C:\Machine", [r"C:\Common", r"c:\USER"], {"NVM_HOME": r"%LOCALAPPDATA%\nvm", "LOCALAPPDATA": r"C:\Profile\Local", "NVM_SYMLINK": r"C:\nvm4w\nodejs"})
        self.assertEqual(entries, [r"C:\Explicit", r"C:\Process", r"C:\Profile\Local\nvm", r"C:\User", r"C:\nvm4w\nodejs", r"C:\Machine", r"C:\Common"])

    def test_bare_npx_uses_cmd_not_ps1_or_shim(self):
        """无扩展shim和ps1不在bare可执行候选中。"""
        directory = r"C:\Tools"
        exists = filesystem([directory + r"\npx", directory + r"\npx.ps1", directory + r"\npx.cmd"])
        self.assertEqual(probe.resolve_bare("npx", [directory], exists), directory + r"\npx.cmd")
        self.assertIsNone(probe.resolve_bare("npx.ps1", [directory], exists))

    def test_missing_dependency_does_not_start_jsonl(self):
        """launcher preflight失败时wire.run完全不调用，不创建ACP尝试。"""
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(probe.wire, "ROOT", Path(directory)), patch.object(probe, "registry_paths", return_value={"user": {}, "machine": {}}), patch.object(probe, "launcher_resolution", return_value={"status": "LAUNCHER_DEPENDENCY_UNAVAILABLE"}), patch.object(probe.wire, "run") as launch:
                self.assertEqual(probe.run(), 1)
                launch.assert_not_called()

    def test_unexpanded_or_relative_paths_not_used(self):
        """未知变量/相对目录不进入spawn搜索路径，也不读取任意环境键。"""
        self.assertEqual(probe.path_entries("%UNKNOWN%;relative;C:\\Good", "", "", "", [], {}), [r"C:\Good"])


if __name__ == "__main__":
    unittest.main()
