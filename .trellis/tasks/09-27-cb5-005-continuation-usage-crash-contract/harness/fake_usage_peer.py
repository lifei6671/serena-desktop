"""Stage B deterministic fake：只回传合成数字，永不启动Provider。"""
import json
import sys
import time

MODE, LOG = sys.argv[1:]
META = "codebuddy.ai/conversationRequestId"
turn = 0
previous = None


def emit(value):
    """发送完整有界JSON-RPC frame。"""
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def usage(used, corr=None, size=1000, session="S1"):
    """含结构、秘密字段和数字扩展的typed UsageUpdate fixture。"""
    value = {"sessionUpdate": "usage_update", "used": used, "size": size,
             "cost": {"amount": 0.1, "currency": "PRIVATE_CURRENCY"},
             "_meta": {"usage": {"enabled": True, "pending": None, "accessToken": 987654321,
                                  "note": "PRIVATE_USAGE", "bins": ["PRIVATE_ARRAY", 999999]}}}
    if corr:
        value["_meta"][META] = corr
    emit({"method": "session/update", "params": {"sessionId": session, "update": value}})


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method is None:
        continue
    with open(LOG, "a", encoding="utf-8") as stream:
        stream.write(json.dumps({"method": method, "sessionId": request.get("params", {}).get("sessionId"),
                                 "cwd": request.get("params", {}).get("cwd")}) + "\n")
    if MODE == "timeout":
        time.sleep(30)
    if method == "initialize":
        emit({"id": request["id"], "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}})
    elif method == "session/new":
        usage(0)
        emit({"id": request["id"], "result": {"sessionId": "S1", "configOptions": []}})
    elif method == "session/resume":
        assert request["params"]["sessionId"] == "S1"
        assert "mcpServers" not in request["params"]
        turn = 2
        if MODE == "failed-resume":
            emit({"id": request["id"], "error": {"code": -32601, "message": "PRIVATE_ERROR"}})
            continue
        usage(0, session="wrong" if MODE == "wrong-session" else "S1")
        emit({"id": request["id"], "result": {"configOptions": [], **({"sessionId": "wrong"} if MODE == "wrong-response" else {})}})
    elif method == "session/load":
        raise AssertionError("NO_LOAD_FALLBACK")
    elif method == "session/prompt":
        turn += 1
        corr = request["params"]["_meta"][META]
        if previous:
            usage(120, previous)
        usage(100 if turn == 1 else 80, corr, size=2000 if MODE == "size-change" and turn >= 2 else 1000)
        emit({"method": "session/update", "params": {"sessionId": "S1", "update": {
            "sessionUpdate": "session_info_update", "_meta": {"context": {"used": 88}, "cost": 0.2, "secretUsage": 87654321}}}})
        emit({"method": "session/update", "params": {"sessionId": "S1", "update": {
            "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "PRIVATE_ANSWER"}, "_meta": {META: corr}}}})
        result = {"stopReason": "end_turn", "_meta": {META: corr, "context": {"size": 1000}}}
        if MODE == "breakdown":
            result["_meta"]["usage"] = {"inputTokens": 20 * turn, "outputTokens": 5, "cachedReadTokens": 0, "cachedWriteTokens": 0, "totalTokens": 20 * turn + 5}
        emit({"id": request["id"], "result": result})
        usage(0, corr)
        previous = corr
        print("STDERR_SECRET", file=sys.stderr, flush=True)
