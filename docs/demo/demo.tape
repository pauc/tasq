# README demo: `tasq` on the command line, then `tasq ui`.
#
# Render with `scripts/demo-gif` (VHS in docker, nothing to install) or, with
# vhs on PATH, `vhs docs/demo/demo.tape` from the repository root with `tasq`
# on PATH. docs/demo/setup.sh builds the notebook it records against.

Output docs/demo/tasq.gif

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
Type "source docs/demo/setup.sh && clear" Enter
Sleep 1s
Show

Type "tasq" Sleep 500ms Enter
Sleep 3s

Type "tasq set 3 in-progress 'reproduced with a frozen clock'" Sleep 500ms Enter
Sleep 2.5s

Type "tasq summary" Sleep 500ms Enter
Sleep 3s

Type "clear" Enter
Type "tasq ui" Sleep 500ms Enter
Sleep 2.5s

Type "j" Sleep 800ms
Type "j" Sleep 1.5s

Type "s" Sleep 1.5s
Type "j" Sleep 800ms
Enter Sleep 2s

Type "?" Sleep 3s
Escape Sleep 1s

Type "q"
Sleep 1.5s
