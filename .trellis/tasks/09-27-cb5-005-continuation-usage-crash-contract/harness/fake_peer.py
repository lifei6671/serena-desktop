"""仅测试使用的 deterministic ACP peer；不导入/启动 CodeBuddy。"""
import json
import sys
import time

MODE, EXPECTED, LOG = sys.argv[1:]
TOKEN = "01900000-0000-7000-8000-000000000001"
META = "codebuddy.ai/conversationRequestId"


def emit(value):
    """发送一个完整 fake frame。"""
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def update(kind, text="", corr=None, session="S1"):
    """模拟 replay/live；正文仅在内存和 pipe 中。"""
    value = {"sessionUpdate": kind}
    if kind.endswith("message_chunk"):
        value["content"] = {"type": "text", "text": text}
    if corr:
        value["_meta"] = {META: corr}
    emit({"method": "session/update", "params": {"sessionId": session, "update": value}})


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method is None:
        continue
    # 只记录测试调用顺序/安全参数，不保存 prompt 或 token。
    with open(LOG, "a", encoding="utf-8") as stream:
        stream.write(json.dumps({"method": method, "sessionId": request.get("params", {}).get("sessionId"), "cwd": request.get("params", {}).get("cwd")}) + "\n")
    if MODE == "timeout":
        time.sleep(30)
    if MODE == "large-frame":
        print("x" * 1_048_578, flush=True)
        time.sleep(30)
    if method == "initialize":
        emit({"id": request["id"], "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}})
    elif method == "session/new":
        update("config_option_update")
        emit({"id": request["id"], "result": {"sessionId": "PRIVATE_INVALID_ID\nsecret text" if MODE == "invalid-new-id" else "S1", "configOptions": []}})
    elif method in ("session/resume", "session/load"):
        assert method == "session/" + EXPECTED
        assert request["params"]["sessionId"] == "S1"
        assert "cwd" in request["params"]
        # 官方 schema 可省略空 mcpServers；不能强写猜测 wire。
        assert request["params"].get("mcpServers", []) == []
        if MODE == "unsupported":
            emit({"id": request["id"], "error": {"code": -32601, "message": "PRIVATE_ERROR"}})
            continue
        update("user_message_chunk", "PRIVATE_REPLAY")
        update("agent_message_chunk", TOKEN, session="wrong" if MODE == "wrong-update" else "S1")
        result = {"configOptions": []}
        if MODE == "wrong-response":
            result["sessionId"] = "wrong"
        emit({"id": request["id"], "result": result})
    elif method == "session/prompt":
        corr = request["params"]["_meta"][META]
        if MODE == "ambiguous":
            update("agent_message_chunk", "PRIVATE_LATE_REPLAY", corr="old-conversation")
        update("agent_message_chunk", TOKEN if MODE != "forgot" else "UNKNOWN", corr=corr)
        emit({"id": request["id"], "result": {"stopReason": "end_turn"}})
        update("agent_message_chunk", "PRIVATE_LATE", corr=corr)
        print("STDERR_SECRET", file=sys.stderr, flush=True)
