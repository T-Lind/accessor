"""Protocol fixture: never connects to a model or executes a command."""
import json
import sys

def send(msg):
    print(json.dumps(msg), flush=True)

def reply(text):
    send({"method": "item/completed", "params": {"item": {"type": "agentMessage", "text": text}}})
    send({"method": "turn/completed", "params": {"turn": {"id": "turn-1", "status": "completed"}}})

for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        send({"id": msg["id"], "result": {}})
    elif method == "thread/start":
        assert msg["params"]["sandbox"] == "read-only"
        assert msg["params"]["approvalsReviewer"] in ("auto_review", "user")
        send({"id": msg["id"], "result": {"thread": {"id": "thread-1"}}})
    elif method == "model/list":
        send({"id": msg["id"], "result": {"data": [{"id":"fixture-astra","model":"fixture-astra","displayName":"Fixture Astra"},{"id":"fixture-sol","model":"fixture-sol","displayName":"Fixture Sol"}], "nextCursor":None}})
    elif method == "turn/start":
        send({"id": msg["id"], "result": {"turn": {"id": "turn-1"}}})
        send({"method": "turn/started", "params": {"turn": {"id": "turn-1"}}})
        text = msg["params"]["input"][0]["text"]
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
        elif text == "which model":
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
