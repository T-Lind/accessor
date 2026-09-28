"""Deterministic local-control replies shared by offline harness fixtures."""
import datetime
import json


def _quiet_covering_now():
    """A local quiet window that always contains the current time."""
    now = datetime.datetime.now()
    return {
        "start": (now - datetime.timedelta(hours=1)).strftime("%H:%M"),
        "end": (now + datetime.timedelta(hours=1)).strftime("%H:%M"),
    }


def control_reply(text):
    if text == "stop the alarm":
        action = dict(action="stop_alarm")
    elif text == "schedule fixture":
        action = dict(action="schedule", prompt="Report", delay_seconds=3600,
                      harness="mock", model="mock-light", reasoning="medium")
    elif text == "quiet fixture":
        action = dict(action="schedule", prompt="Quiet report", delay_seconds=1,
                      harness="mock", model="mock-light", reasoning="low",
                      quiet=_quiet_covering_now())
    elif text == "notify fixture":
        action = dict(action="notify", title="Fixture notice",
                      text="Deterministic notice body.", speak=False)
    elif text == "notify distinct":
        action = dict(action="notify", title="Server alert",
                      text="Disk usage passed ninety percent on host alpha.", speak=False)
    elif text.startswith("edit fixture "):
        action = dict(action="update_schedule", id=text.split()[-1],
                      changes=dict(prompt="Changed by harness", reasoning="high", paused=True))
    elif text.startswith("delete fixture "):
        action = dict(action="delete_schedule", id=text.split()[-1])
    else:
        return None
    return json.dumps({"accessor": action})
