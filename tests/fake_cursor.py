"""Offline Cursor agent fixture: emits Cursor stream-json for one headless run."""
import json
import sys


def emit(value):
    print(json.dumps(value), flush=True)


text = sys.stdin.read().strip()
prompt = (text.split("Current request:\n")[-1] if "Current request:\n" in text else text.split("\n\n")[-1]).strip()
session = "cur_fixture"

emit(
    {
        "type": "system",
        "subtype": "init",
        "apiKeySource": "env",
        "cwd": ".",
        "session_id": session,
        "model": "auto",
        "permissionMode": "default",
    }
)
emit(
    {
        "type": "assistant",
        "message": {"role": "assistant", "content": [{"type": "text", "text": "cursor reply: " + prompt}]},
        "session_id": session,
    }
)
emit(
    {
        "type": "result",
        "subtype": "success",
        "is_error": False,
        "duration_ms": 1,
        "result": "cursor reply: " + prompt,
        "session_id": session,
    }
)
