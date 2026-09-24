#!/usr/bin/env python3
"""Accessor Linux desktop bridge.

Reads the AT-SPI2 accessibility tree and performs semantic actions, so an agent
can act on controls by name instead of pixel coordinates. AT-SPI is the Linux
accessibility standard; most GTK/Qt/Electron apps expose it once accessibility
is enabled (GNOME: `gsettings set org.gnome.desktop.interface
toolkit-accessibility true`).

Usage: accessor-atspi.py <command> [args...]
  list
  focus <title>
  snapshot <max>
  invoke <name> <role>
  select <name> <role>
  expand <name>
  set <name> <value>

Exits 2 with a clear message when the AT-SPI bindings are not installed.
"""
import sys


def load_atspi():
    try:
        import gi

        gi.require_version("Atspi", "2.0")
        from gi.repository import Atspi

        return Atspi
    except Exception:
        try:
            import pyatspi as Atspi  # legacy bindings

            return Atspi
        except Exception:
            return None


def call(obj, *names):
    for name in names:
        fn = getattr(obj, name, None)
        if callable(fn):
            try:
                return fn()
            except Exception:
                return None
    return None


def child_count(acc):
    value = call(acc, "get_child_count", "getChildCount")
    return value if isinstance(value, int) else 0


def child_at(acc, index):
    for name in ("get_child_at_index", "getChildAtIndex"):
        fn = getattr(acc, name, None)
        if callable(fn):
            try:
                return fn(index)
            except Exception:
                return None
    return None


def name_of(acc):
    value = call(acc, "get_name", "getName")
    return value if isinstance(value, str) else ""


def role_of(acc):
    value = call(acc, "get_role_name", "getRoleName")
    return value if isinstance(value, str) else ""


def is_active(acc, Atspi):
    states = call(acc, "get_state_set", "getState")
    if states is None:
        return False
    for name in ("contains", "contains_state"):
        fn = getattr(states, name, None)
        if callable(fn):
            for state in ("ACTIVE", "STATE_ACTIVE"):
                enum = getattr(Atspi, "StateType", Atspi)
                value = getattr(enum, state, None)
                if value is None:
                    continue
                try:
                    if fn(value):
                        return True
                except Exception:
                    pass
            if name == "contains":
                try:
                    return bool(fn(1 << 8))  # STATE_ACTIVE bit
                except Exception:
                    pass
    return False


def do_action(acc, index=0):
    action = call(acc, "get_action_iface", "queryAction")
    if action is None:
        fn = getattr(acc, "get_action", None)
        action = fn() if callable(fn) else None
    if action is None:
        return False
    fn = getattr(action, "do_action", None) or getattr(action, "doAction", None)
    if not callable(fn):
        return False
    try:
        return bool(fn(index))
    except Exception:
        return False


def set_text(acc, text):
    for getter in ("get_editable_text_iface", "queryEditableText", "get_text_iface", "queryText"):
        fn = getattr(acc, getter, None)
        if not callable(fn):
            continue
        try:
            iface = fn()
        except Exception:
            continue
        for setter in ("set_text_contents", "setTextContents"):
            method = getattr(iface, setter, None)
            if callable(method):
                try:
                    method(text)
                    return True
                except Exception:
                    pass
    return False


def applications(Atspi):
    desktop = Atspi.get_desktop(0)
    apps = []
    for i in range(child_count(desktop)):
        child = child_at(desktop, i)
        if child is not None:
            apps.append(child)
    return apps


def frames(app):
    out = []
    for i in range(child_count(app)):
        child = child_at(app, i)
        if child is not None:
            out.append(child)
    return out


def active_frame(Atspi):
    for app in applications(Atspi):
        for frame in frames(app):
            if is_active(frame, Atspi):
                return frame
    # Fall back to the first frame of the first windowed app.
    for app in applications(Atspi):
        windowed = frames(app)
        if windowed:
            return windowed[0]
    return None


def cmd_list(Atspi):
    lines = []
    for app in applications(Atspi):
        for frame in frames(app):
            lines.append(f"{name_of(app)}  {name_of(frame)}")
    return "\n".join(lines) if lines else "No visible windows found."


def cmd_focus(Atspi, needle):
    needle = needle.lower()
    for app in applications(Atspi):
        for frame in frames(app):
            if needle in name_of(frame).lower() or needle in name_of(app).lower():
                if not do_action(frame, 0):
                    call(frame, "grab_focus", "grabFocus")
                return f"Focused {name_of(app)} — {name_of(frame)}"
    return f"No window matches {needle}"


def walk(acc, depth, out, max_count):
    if depth > 15 or len(out) >= max_count:
        return
    for i in range(child_count(acc)):
        if len(out) >= max_count:
            return
        child = child_at(acc, i)
        if child is None:
            continue
        label = name_of(child)
        if label:
            out.append((role_of(child), label, child))
        walk(child, depth + 1, out, max_count)


def cmd_snapshot(Atspi, max_text):
    max_count = max(1, min(int(max_text), 400))
    root = active_frame(Atspi)
    if root is None:
        return "No active window. Enable accessibility and focus an app first."
    collected = []
    walk(root, 0, collected, max_count)
    if not collected:
        return "Window exposes no named controls. Use screenshot-based actions instead."
    lines = [f'Window "{name_of(root)}" — {len(collected)} controls:']
    for index, (role, label, _) in enumerate(collected, start=1):
        lines.append(f'[{index}] {role} "{label}"')
    return "\n".join(lines)


def find(acc, needle, role_filter, depth=0):
    if depth > 15:
        return None
    for i in range(child_count(acc)):
        child = child_at(acc, i)
        if child is None:
            continue
        role = role_of(child)
        if name_of(child) == needle and (not role_filter or role == role_filter):
            return child
        found = find(child, needle, role_filter, depth + 1)
        if found is not None:
            return found
    return None


def cmd_act(Atspi, needle, role_filter, mode):
    root = active_frame(Atspi)
    if root is None:
        return "No active window."
    target = find(root, needle, role_filter)
    if target is None:
        return f'Control "{needle}" is not on screen'
    if mode == "set":
        return "internal error"
    if do_action(target, 0):
        return f'{mode} "{needle}"'
    if mode == "expand":
        if do_action(target, 1):
            return f'expand "{needle}"'
    return f'Could not {mode} "{needle}"'


def main():
    Atspi = load_atspi()
    if Atspi is None:
        print(
            "AT-SPI Python bindings are not installed. Install python3-pyatspi "
            "(Debian/Ubuntu) or python3-atspi (Fedora), enable accessibility, or "
            "use screenshot-based actions."
        )
        return 2
    args = sys.argv[1:]
    if not args:
        print("usage: accessor-atspi.py <command> [args...]")
        return 2
    command, rest = args[0], args[1:]
    try:
        if command == "list":
            print(cmd_list(Atspi))
        elif command == "focus":
            print(cmd_focus(Atspi, rest[0]))
        elif command == "snapshot":
            print(cmd_snapshot(Atspi, rest[0] if rest else "80"))
        elif command in ("invoke", "select", "expand"):
            print(cmd_act(Atspi, rest[0], rest[1] if len(rest) > 1 else "", command))
        elif command == "set":
            root = active_frame(Atspi)
            target = find(root, rest[0], "") if root is not None else None
            if target is None:
                print(f'Control "{rest[0]}" is not on screen')
            elif set_text(target, rest[1]):
                print(f'set "{rest[0]}"')
            elif do_action(target, 0):
                print(f'set "{rest[0]}" via action')
            else:
                print(f'Could not set "{rest[0]}"')
        else:
            print(f"Unknown command: {command}")
            return 2
    except Exception as error:  # never crash the caller
        print(f"AT-SPI bridge error: {error}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
