# ADR 0011 — Isolated acceptances under launchd

Status: accepted for slice 1.

The six acceptances use real processes and the same service templates as the
Dev installation. Each run receives unique names under
`com.soyeht.next.dev.e2e.*`, a short temporary directory and binary copies.
Running the test therefore does not replace the already installed Dev profile.

Services use the user's graphical domain. If the runner lacks that domain,
they use the user domain. The test never uses the system domain.
On completion, it closes panes through the supervisor and unloads only the
two services it created.

The test shell uses `ZDOTDIR` pointing to its own files, without personal
history. The test waits for the controlled prompt before sending commands;
an existing PID does not prove shell startup has finished. The supervisor passes
this variable when it exists in its environment. Normal installation preserves
the user's settings.

The restart acceptance sends SIGKILL to the engine and observes its automatic
restart by the service. The overflow acceptance unloads only the engine, waits
for the shell to finish producing more than 8 MiB and loads the engine again.
The same supervisor stays alive. Offset-based replay must declare the gap.

The report records each acceptance separately, including failures. CI publishes
the content in logs and the run summary. A real iPhone connection with Wi-Fi
disabled and rebooting the Mac remain in the manual procedure.

The runner completed all six acceptances in 6 seconds. The account reached
GitHub's artifact quota; the JSON report stays in the logs and run summary.
Approval still depends on the test results.
