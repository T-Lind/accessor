-- Accessor macOS desktop bridge. Drives other applications through the
-- Accessibility API exposed by System Events (the same AXUIElement tree used
-- by native tools). Invoked as:
--   osascript accessor.applescript <command> [args...]
-- Commands: list | focus <title> | snapshot <windowNeedle> <max>
--           invoke <name> <role> | select <name> <role>
--           set <name> <value> | expand <name>
-- Requires Accessibility permission for the parent process (Terminal/acc).

on run argv
	set command to item 1 of argv
	if command is "list" then
		return my listWindows()
	else if command is "focus" then
		return my focusWindow(item 2 of argv)
	else if command is "snapshot" then
		if (count of argv) > 2 then
			return my snapshot(item 2 of argv, item 3 of argv)
		end if
		return my snapshot("", "80")
	else if command is "invoke" then
		return my act(item 2 of argv, item 3 of argv, "press")
	else if command is "select" then
		return my act(item 2 of argv, item 3 of argv, "select")
	else if command is "expand" then
		return my act(item 2 of argv, item 3 of argv, "expand")
	else if command is "set" then
		return my setValue(item 2 of argv, item 3 of argv)
	end if
	return "Unknown command: " & command
end run

on frontProcess()
	tell application "System Events"
		repeat with p in (every process whose frontmost is true)
			return p
		end repeat
	end tell
	error "No frontmost process"
end frontProcess

on listWindows()
	set out to ""
	tell application "System Events"
		repeat with p in (every process whose background only is false)
			try
				repeat with w in (every window of p)
					set out to out & (name of p) & "  " & (name of w) & linefeed
				end repeat
			end try
		end repeat
	end tell
	if out is "" then return "No visible windows found."
	return out
end listWindows

on focusWindow(needle)
	tell application "System Events"
		repeat with p in (every process whose background only is false)
			try
				repeat with w in (every window of p)
					if (name of w) contains needle or (name of p) contains needle then
						set frontmost of p to true
						try
							perform action "AXRaise" of w
						end try
						return "Focused " & (name of p) & " — " & (name of w)
					end if
				end repeat
			end try
		end repeat
	end tell
	return "No window matches " & needle
end focusWindow

on targetWindow(needle)
	if needle is not "" then
		tell application "System Events"
			repeat with p in (every process whose background only is false)
				try
					repeat with w in (every window of p)
						if (name of w) contains needle then return w
					end repeat
				end try
			end repeat
		end tell
	end if
	set p to my frontProcess()
	tell application "System Events" to return front window of p
end targetWindow

on snapshot(needle, maxText)
	set maxCount to maxText as integer
	set collected to {}
	set w to my targetWindow(needle)
	my walk(w, 0, maxCount, collected)
	if (count of collected) is 0 then
		return "Window exposes no named controls. Use screenshot-based actions instead."
	end if
	set AppleScript's text item delimiters to linefeed
	set title to ""
	try
		set title to name of w
	end try
	return "Window \"" & title & "\" — " & (count of collected) & " controls:" & linefeed & (collected as text)
end snapshot

on walk(el, depth, maxCount, collected)
	if depth > 12 then return
	tell application "System Events"
		try
			repeat with c in (UI elements of el)
				if (count of collected) > (maxCount - 1) then return
				try
					set roleName to role of c
					set label to ""
					try
						set label to title of c
					end try
					if label is missing value then set label to ""
					if label is "" then
						try
							set label to description of c
						end try
					end if
					if label is missing value then set label to ""
					if label is not "" then
						set end of collected to "[" & ((count of collected) + 1) & "] " & roleName & " \"" & label & "\""
					end if
				end try
				my walk(c, depth + 1, maxCount, collected)
			end repeat
		end try
	end tell
end walk

on findElement(el, needle, roleFilter, depth)
	if depth > 12 then return missing value
	tell application "System Events"
		try
			repeat with c in (UI elements of el)
				try
					set roleName to role of c
					set label to ""
					try
						set label to title of c
					end try
					if label is missing value then set label to ""
					if label is "" then
						try
							set label to description of c
						end try
					end if
					if label is missing value then set label to ""
					if label is needle and (roleFilter is "" or roleName is roleFilter) then return c
				end try
				set found to my findElement(c, needle, roleFilter, depth + 1)
				if found is not missing value then return found
			end repeat
		end try
	end tell
	return missing value
end findElement

on act(needle, roleFilter, mode)
	set w to my targetWindow("")
	set found to my findElement(w, needle, roleFilter, 0)
	if found is missing value then return "Control \"" & needle & "\" is not on screen"
	try
		if mode is "press" then
			click found
		else if mode is "select" then
			try
				set selected of found to true
			on error
				click found
			end try
		else if mode is "expand" then
			try
				perform action "AXShowMenu" of found
			on error
				click found
			end try
		end if
	on error errMsg
		try
			perform action "AXPress" of found
		on error
			return "Could not act on \"" & needle & "\": " & errMsg
		end try
	end try
	return mode & " \"" & needle & "\""
end act

on setValue(needle, value)
	set w to my targetWindow("")
	set found to my findElement(w, needle, "", 0)
	if found is missing value then return "Control \"" & needle & "\" is not on screen"
	try
		set value of found to value
	on error errMsg
		return "Could not set \"" & needle & "\": " & errMsg
	end try
	return "Set \"" & needle & "\""
end setValue
