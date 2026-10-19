# README demo: editing in `tasq ui`: filter, create with the calendar, log, done.
#
# Render with `scripts/demo-gif` (VHS in docker, nothing to install) or, with
# vhs on PATH, `vhs docs/demo/ui-edit.tape` from the repository root with
# `tasq` on PATH. docs/demo/setup.sh builds the store it records against.

Output docs/demo/ui-edit.gif

Require tasq

Set Shell "bash"
Set FontSize 18
Set Width 1300
Set Height 680
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 40ms
Set Framerate 30

Hide
Type "source docs/demo/setup.sh && clear && tasq ui" Enter
Sleep 2s
Show

Sleep 1.5s

# Create a task in the edit view; Enter on the due date opens the calendar.
Type "c" Sleep 1s
Type "Fix the release workflow on tags" Sleep 500ms
Tab Sleep 600ms
Tab Sleep 300ms
Left Sleep 800ms
Tab Sleep 300ms
Enter Sleep 1.2s
Right Sleep 400ms
Right Sleep 400ms
Right Sleep 800ms
Enter Sleep 800ms
Tab Sleep 300ms
Tab Sleep 300ms
Type "ci" Sleep 400ms
Tab Sleep 300ms
Type "The tag pipeline skips the build job since the rules rewrite." Sleep 1s
Ctrl+S Sleep 2s

# Filter down to one task, log a note on it, close it.
Type "/" Sleep 400ms
Type "flaky" Sleep 600ms
Enter Sleep 1.2s
Type "l" Sleep 600ms
Type "the mailer read the clock twice" Sleep 500ms
Enter Sleep 1.2s
Tab Sleep 2.5s
Type "d" Sleep 600ms
Type "fixed, frozen clock in the spec" Sleep 500ms
Enter Sleep 2s

Type "/" Sleep 400ms
Backspace 5 Sleep 400ms
Enter Sleep 2.5s

Type "q"
Sleep 1s
