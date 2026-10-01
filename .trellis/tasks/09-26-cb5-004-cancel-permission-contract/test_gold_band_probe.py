"""最小JSONL诊断的确定性回归；不启动CLI、不请求模型。"""
import json
import unittest
import gold_band_probe as probe


def response(rpc_id, result):
    """构造模拟peer响应帧。"""
    return {"jsonrpc": "2.0", "id": rpc_id, "result": result}


def notification():
    """包含不可落盘正文的模拟通知。"""
    return {"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s", "update": {"sessionUpdate": "agent_message_chunk", "content": {"text": "PRIVATE_TEXT_MUST_NOT_PERSIST"}}}}


def execute(frames):
    """fake使用与真实执行完全相同的JSON decoder和handshake。"""
    frames = iter(frames)
    sent = []

    def send(data):
        """收集实际序列化请求。"""
        sent.append(json.loads(data))

    def receive(_timeout):
        """耗尽即真实EOF语义；不伪造响应。"""
        item = next(frames, b"")
        return item if isinstance(item, bytes) else (json.dumps(item) + "\n").encode()

    client = probe.JsonlClient(send, receive)
    return probe.handshake(client, r"C:\temp\fixture"), sent, client


class GoldBandTests(unittest.TestCase):
    """覆盖interleave/identity/EOF/schema/session gate与初始化exact shape。"""

    def test_exact_initialize_and_launcher(self):
        """发送用户指定完整初始化，不增加额外capability或SDK默认值。"""
        result, sent, _ = execute([response(1, {"protocolVersion": 1}), response(2, {"sessionId": "s"})])
        self.assertEqual(sent[0]["params"], {"protocolVersion": 1, "clientCapabilities": {"elicitation": {"form": {}}, "_meta": {"subagent-transcript": True, "parameterizedModelPicker": True}}, "clientInfo": {"name": "serena-desktop-gold-band-repro", "title": "SerenaDesktop Gold Band Repro", "version": "0.1"}})
        self.assertEqual(sent[1]["params"], {"cwd": r"C:\temp\fixture", "mcpServers": []})
        self.assertEqual(probe.LAUNCHER[1:], ["-y", "@tencent-ai/codebuddy-code@2.158.0", "--acp"])
        self.assertEqual(result["status"], "PASS")

    def test_interleaved_notifications_and_close(self):
        """通知不是response；advertised close仅在非空sessionId后发送。"""
        result, sent, client = execute([notification(), response(1, {"protocolVersion": 1, "agentCapabilities": {"sessionCapabilities": {"close": {}}}}), notification(), response(2, {"sessionId": "exact-session"}), response(3, {})])
        self.assertEqual(result["close"], "ACKNOWLEDGED")
        self.assertEqual(sent[2]["params"], {"sessionId": "exact-session"})
        self.assertEqual(client.preroute_total, 2)
        self.assertNotIn("PRIVATE_TEXT_MUST_NOT_PERSIST", json.dumps(client.rows))

    def test_wrong_id_is_ignored(self):
        """错误id不能冒充initialize或new响应。"""
        result, _, client = execute([response(99, {"protocolVersion": 999}), response(1, {"protocolVersion": 1}), response("2", {"sessionId": "wrong"}), response(2, {"sessionId": "correct"})])
        self.assertEqual(result["sessionId"], "correct")
        self.assertEqual(client.wrong_ids, 2)

    def test_eof_stops_without_close_or_prompt(self):
        """new EOF立即停止，不重放、不发close/prompt。"""
        result, sent, _ = execute([response(1, {"protocolVersion": 1})])
        self.assertEqual(result["error"], "EOF")
        self.assertEqual([r["method"] for r in sent], ["initialize", "session/new"])

    def test_malformed_json_and_response(self):
        """坏JSON/冲突result+error不能成为成功事实。"""
        for frame in [b"not-json\n", {"jsonrpc": "2.0", "id": 1, "result": {}, "error": {}}, {"jsonrpc": "2.0", "id": 1, "result": []}]:
            with self.subTest(frame=frame):
                result, sent, _ = execute([frame])
                self.assertEqual(result["status"], "FAILED")
                self.assertTrue(result["error"].startswith("MALFORMED"))
                self.assertEqual(len(sent), 1)

    def test_new_error_blocks_close_and_prompt(self):
        """已广告close也不得对失败的new发送close；错误正文脱敏。"""
        result, sent, client = execute([response(1, {"protocolVersion": 1, "agentCapabilities": {"sessionCapabilities": {"close": {}}}}), {"jsonrpc": "2.0", "id": 2, "error": {"code": -32603, "data": {"details": "Request failed with status code 500", "secret": "PRIVATE"}}}])
        self.assertEqual(result["error"], "HTTP_500")
        self.assertEqual(result["close"], "NOT_RUN")
        self.assertEqual(len(sent), 2)
        self.assertNotIn("PRIVATE", json.dumps(client.rows))

    def test_empty_or_invalid_session_id_gate(self):
        """空白、null、数字都不是合法new成功，不能建立route或close。"""
        for session_id in ["", "  ", None, 17]:
            result, sent, client = execute([response(1, {"protocolVersion": 1, "agentCapabilities": {"sessionCapabilities": {"close": {}}}}), response(2, {"sessionId": session_id})])
            self.assertEqual(result["error"], "EMPTY_OR_INVALID_SESSION_ID")
            self.assertIsNone(client.route)
            self.assertEqual(len(sent), 2)

    def test_preroute_cache_bounded(self):
        """通知总数独立计数，但route前安全缓存最多64条。"""
        result, _, client = execute([response(1, {"protocolVersion": 1})] + [notification()] * 80 + [response(2, {"sessionId": "s"})])
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(client.preroute_total, 80)
        self.assertEqual(len(client.preroute), 64)
        self.assertEqual(len([r for r in client.rows if r["direction"] == "notification"]), 64)

    def test_prompt_method_forbidden(self):
        """诊断client无法发模型/prompt调用。"""
        client = probe.JsonlClient(lambda _: self.fail("must not send"), lambda _: b"")
        with self.assertRaises(probe.ProbeError):
            client.request("session/prompt", {}, 1)

    def test_mixed_notification_and_wrong_id_flood_is_bounded(self):
        """未知通知和wrong-id洪流都不能绕过缓存/证据上限或阻止真实响应匹配。"""
        frames = []
        for _ in range(100):
            frames.extend([notification(), {"jsonrpc": "2.0", "method": "custom/notice", "params": {}}, response(999, {"sessionId": "wrong"})])
        result, _, client = execute(frames + [response(1, {"protocolVersion": 1}), response(2, {"sessionId": "s"})])
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(client.preroute_total, 200)
        self.assertEqual(len(client.preroute), 64)
        self.assertEqual(client.wrong_ids, 100)
        self.assertEqual(sum(client.notifications.values()), 200)
        self.assertEqual(len([r for r in client.rows if r["direction"] == "notification"]), 64)
        self.assertEqual(len([r for r in client.rows if r["direction"] == "ignored_response"]), 16)
        self.assertLessEqual(len(client.rows), 84)


if __name__ == "__main__":
    unittest.main()
