# README demo: one task's life on the command line, from create to done.
#
# Render with `scripts/demo-gif` (VHS in docker, nothing to install) or, with
# vhs on PATH, `vhs docs/demo/cli.tape` from the repository root with `tasq`
# on PATH. docs/demo/setup.sh builds the store it records against.

Output docs/demo/cli.gif

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

Type "tasq create 'Fix the release workflow on tags' --prio A --due tomorrow --tag ci" Sleep 500ms Enter
Sleep 2s

Type "tasq log 6 'the rules rewrite dropped the tag condition'" Sleep 500ms Enter
Sleep 1.5s

Type "tasq tag 6 +release" Sleep 500ms Enter
Sleep 1.5s

Type "tasq ls ready" Sleep 500ms Enter
Sleep 3s

Type "clear" Enter
Type "tasq view 6" Sleep 500ms Enter
Sleep 3.5s

Type "tasq done 6 'fixed in !4830'" Sleep 500ms Enter
Sleep 2s

Type "tasq ls --done" Sleep 500ms Enter
Sleep 3s
