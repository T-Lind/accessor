"""Protocol fixture: never connects to a model or executes a command."""
import json
import os
import time
import sys
from control_fixtures import control_reply

def send(msg):
    print(json.dumps(msg), flush=True)

def reply(text):
    send({"method": "item/completed", "params": {"item": {"type": "agentMessage", "text": text}}})
    send({"method": "turn/completed", "params": {"turn": {"id": "turn-1", "status": "completed"}}})

for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        if msg["params"]["clientInfo"].get("title") == "Accessor":
            assert msg["params"]["capabilities"]["experimentalApi"] is True
        time.sleep(float(os.environ.get("ACC_FIXTURE_START_DELAY", "0")))
        send({"id": msg["id"], "result": {}})
    elif method == "thread/start":
        assert msg["params"]["sandbox"] == "read-only"
        assert msg["params"]["approvalsReviewer"] in ("auto_review", "user")
        send({"id": msg["id"], "result": {"thread": {"id": "thread-1"}}})
    elif method == "model/list":
        send({"id": msg["id"], "result": {"data": [{"id":"fixture-astra","model":"fixture-astra","displayName":"Fixture Astra"},{"id":"fixture-sol","model":"fixture-sol","displayName":"Fixture Sol"}], "nextCursor":None}})
    elif method == "account/rateLimits/read":
        send({"id":msg["id"],"result":{"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":27,"windowDurationMins":300,"resetsAt":2000000000},"secondary":{"usedPercent":39,"windowDurationMins":10080,"resetsAt":2000100000}}}}})
    elif method == "mcpServerStatus/list":
        send({"id":msg["id"],"result":{"data":[{"name":"accessor","tools":{"memory_save":{}},"toolsError":None}]}})
    elif method == "app/installed":
        assert msg["params"]["threadId"] == "thread-1"
        assert msg["params"]["forceRefresh"] is True
        send({"id":msg["id"],"result":{"apps":[{"id":"fixture-calendar","runtimeName":"Calendar","enabled":True,"callable":True}]}})
    elif method == "turn/start":
        send({"id": msg["id"], "result": {"turn": {"id": "turn-1"}}})
        send({"method": "turn/started", "params": {"turn": {"id": "turn-1"}}})
        text = msg["params"]["input"][0]["text"]
        if text.startswith("Conversation data to summarize:"):
            reply("compact-model=" + str(msg["params"].get("model")) + "; compact-effort=" + str(msg["params"].get("effort")))
            continue
        control = control_reply(text.rsplit("Current request:\n", 1)[-1])
        if control:
            reply(control)
            continue
        if "Accessor worker" in text and "<worker_result>" in text:
            reply("main received worker result")
            continue
        if text in ("delegate fixture", "delegate hold fixture"):
            reply(json.dumps({"accessor": {"action":"delegate", "prompt":"hold" if "hold" in text else "worker settings", "harness":"codex", "model":"fixture-worker", "reasoning":"high"}}))
            continue
        if text == "worker settings":
            reply("model=" + str(msg["params"].get("model")) + "; effort=" + str(msg["params"].get("effort")))
            continue
        if text == "plugin delegate fixture":
            reply(json.dumps({"accessor":{"action":"delegate","role":"plugin","prompt":"worker settings","harness":"codex","model":"ignored-worker-model","reasoning":"medium"}}))
            continue
        if text == "approval test":
            send({"id": "approval-1", "method": "item/commandExecution/requestApproval", "params": {"command": "fixture-only; no execution", "threadId": "thread-1", "turnId": "turn-1"}})
        elif text == "grant test":
            send({"id": "grant-1", "method": "item/permissions/requestApproval", "params": {"permissions": {"network": {"enabled": True}}}})
        elif text == "hold":
            send({"method": "item/started", "params": {"item": {"type": "commandExecution", "command": "hold"}}})
            send({"method": "item/completed", "params": {"item": {"type": "agentMessage", "phase": "commentary", "text": "Holding task"}}})
        elif text == "tool test":
            send({"method": "item/started", "params": {"item": {"type": "mcpToolCall", "server": "gmail", "tool": "search"}}})
            send({"method": "item/completed", "params": {"item": {"type": "mcpToolCall", "server": "gmail", "tool": "search"}}})
            reply("tool done")
        elif "which" in text.lower() and "model" in text.lower():
            reply("model=" + str(msg["params"].get("model")))
        else:
            reply("fixture: " + text)
    elif method == "turn/interrupt":
        send({"id": msg["id"], "result": {}})
        reply("interrupted")
    elif msg.get("id") == "approval-1":
        reply("decision=" + msg["result"]["decision"])
    elif msg.get("id") == "grant-1":
        assert msg["result"]["permissions"] == {}
        reply("grant denied")
