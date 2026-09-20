"""Offline Claude/Antigravity protocol fixture; no tools or network."""
import json
import sys
from control_fixtures import control_reply

kind = sys.argv[1]
if "--print-timeout" in sys.argv and "/usage" in sys.argv:
    assert kind == "antigravity"
    print("Gemini Models\tWeekly Limit Remaining\t81%\t2033-05-18T03:33:20Z", flush=True)
    sys.exit(0)
if "--no-session-persistence" in sys.argv:
    assert kind == "claude"
    for line in sys.stdin:
        request = json.loads(line)
        assert request["type"] == "control_request"  # No model turn allowed.
        result = {} if request["request"]["subtype"] == "initialize" else {
            "rate_limits_available": True,
            "rate_limits": {"five_hour": {"utilization":42,"resets_at":"2033-05-18T03:33:20Z"}, "seven_day":None},
        }
        print(json.dumps({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":result}}),flush=True)
    sys.exit(0)
assert "--effort" in sys.argv, sys.argv
assert "--model" in sys.argv, sys.argv
assert "--dangerously-skip-permissions" not in sys.argv
if kind == "antigravity":
    assert "--sandbox" in sys.argv


def send(value):
    print(json.dumps(value), flush=True)


for line in sys.stdin:
    value = json.loads(line)
    if kind == "claude":
        text = value["message"]["content"][0]["text"]
    else:
        text = value["message"]["content"]
    text = text.split("\n\n")[-1]
    text = text.removeprefix("Current request:\n")
    if text == "hold":
        send({"type": "tool_use", "name": "holding-stdio"} if kind == "claude" else
             {"event": "step_update", "step_update": {"step_type": "tool", "tool_name": "holding-stdio"}})
        continue
    if text == "quota fixture":
        if kind == "claude":
            send({"type": "result", "is_error": True, "result": "usage limit reached"})
        else:
            send({"event": "result", "result": {"error": "usage limit reached", "status": "ERROR"}})
        continue
    reply = 'stdio reply: ' + text
    if text.startswith("Conversation data to summarize:"):
        reply = "compact-model=" + sys.argv[sys.argv.index("--model") + 1] + "; compact-effort=" + sys.argv[sys.argv.index("--effort") + 1]
    if text == "note fixture":
        reply = '{"accessor":{"action":"note","text":"Only save once"}}'
    reply = control_reply(text) or reply
    if kind == "claude":
        # Claude can repeat its final text in assistant and result messages.
        send({"type": "assistant", "message": {"content": [{"text": reply}]}})
        send({"type": "result", "result": reply})
    else:
        send({"event": "result", "result": {"response": reply}})
