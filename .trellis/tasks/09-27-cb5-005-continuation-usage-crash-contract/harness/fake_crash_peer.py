"""Stage C确定性fake，仅合成内存答案与测试身份，不启动任何Provider。"""
import json
import sys
import time
from pathlib import Path

MODE, LOG = sys.argv[1:]
REQUEST = "codebuddy.ai/requestId"
MESSAGE = "codebuddy.ai/messageId"
CONVERSATION = "codebuddy.ai/conversationRequestId"


def emit(value):
    """写入一条完整JSON-RPC frame。"""
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def update(kind, request, text, session="S1", message="M1"):
    """模拟typed chunk；正文只能进入pipe。"""
    emit({"method": "session/update", "params": {"sessionId": session, "update": {
        "sessionUpdate": kind, "content": {"type": "text", "text": text},
        "_meta": {REQUEST: request, MESSAGE: message}}}})


for line in sys.stdin:
    q = json.loads(line)
    method = q.get("method")
    if method is None:
        continue
    if MODE == "timeout":
        time.sleep(60)
    params = q.get("params", {})
    with open(LOG, "a", encoding="utf-8") as stream:
        stream.write(json.dumps({"method": method, "sessionId": params.get("sessionId"),
                                 "cwd": params.get("cwd"),
                                 "requestId": params.get("_meta", {}).get(REQUEST)}) + "\n")
    if method == "initialize":
        emit({"id": q["id"], "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}})
    elif method == "session/new":
        emit({"id": q["id"], "result": {"sessionId": "S1", "configOptions": []}})
    elif method == "session/prompt":
        request = params["_meta"][REQUEST]
        assert request == params["_meta"][CONVERSATION]
        # 收到prompt时write-ahead文件必须已经存在，不能在terminal后补写。
        identities = list(Path(LOG).parent.glob("*.prompt-identity.json"))
        assert len(identities) == 1
        assert json.loads(identities[0].read_text(encoding="utf-8"))["requestId"] == request
        if MODE == "terminal-first":
            emit({"id": q["id"], "result": {"stopReason": "end_turn", "_meta": {CONVERSATION: request}}})
            update("agent_message_chunk", request, "PRIVATE_ANSWER")
        else:
            update("agent_message_chunk", "wrong" if MODE == "uncorrelated" else request, "PRIVATE_")
            if MODE in ("before", "uncorrelated"):
                time.sleep(60)
            update("agent_message_chunk", request, "ANSWER")
            emit({"id": q["id"], "result": {"stopReason": "end_turn", "_meta": {
                CONVERSATION: "wrong" if MODE == "wrong-terminal" else request}}})
    elif method == "session/load":
        assert params["sessionId"] == "S1" and params["mcpServers"] == []
        if MODE == "failed-load":
            emit({"id": q["id"], "error": {"code": -32601, "message": "PRIVATE_ERROR"}})
            continue
        with open(LOG, encoding="utf-8") as stream:
            sent = [json.loads(row) for row in stream]
        request = next(row["requestId"] for row in sent if row["method"] == "session/prompt")
        assert params["cwd"] == next(row["cwd"] for row in sent if row["method"] == "session/new")
        if MODE == "wrong-request":
            request = "wrong"
        session = "wrong" if MODE == "wrong-session" else "S1"
        # replay早于load response；重复完整message不得重复拼入答案。
        update("user_message_chunk", request, "PRIVATE_PROMPT", session, "U1")
        for _ in range(2):
            update("agent_message_chunk", request, "PRIVATE_ANSWER", session, "WRONG_MESSAGE" if MODE == "wrong-message" else "M1")
        result = {"configOptions": []}
        if MODE == "terminal-extension":
            result["stopReason"] = "end_turn"
        emit({"id": q["id"], "result": result})
    else:
        raise AssertionError("UNEXPECTED_METHOD_NO_FALLBACK")
