# Slice 1 · manual procedure in the new profile

This procedure was not run on a rebooted Mac or an iPhone on the mobile network
during implementation. Acceptances 1 through 6 have their own automated execution.
This covers acceptance 7 and verification of both applications on real devices.

## Preparation

1. Follow [Dev installation](dev-local.md), [the Mac app](apps-mac.md) and
   [the iPhone app](apps-iphone.md). Use the same sources for engine and applications.
2. Check `soyeht --profile dev status`. Services must be named
   `com.soyeht.next.dev.engine` and `com.soyeht.next.dev.ptyd`. The port is 8291.
   Check each service's executable path with `launchctl print`.
3. In the new Mac app, create a shell pane in a test directory you own. Record
   the workspace, pane name, directory, `paneId`, `pid` and `generation` returned
   by `soyeht --profile dev pane list`. Do not use a pane containing real work.
4. Type `printf 'SLICE_1_ACCEPTANCE\n'; pwd` in that pane and wait for output.
   Record the time. Do not include credentials in commands or evidence.

## Pair and read over the mobile network

1. Connect Mac and iPhone to the same tailnet. In the new Mac app, choose
   “Add iPhone”. In the new iPhone app, choose “Scan Mac QR”.
2. Before allowing access, verify that the iPhone shows a waiting state and does
   not receive the pane list. Compare the name and full fingerprint on both devices.
3. Allow access on the Mac. Open the pane on the iPhone. The `SLICE_1_ACCEPTANCE`
   output and directory must appear. The iPhone is read-only in this slice.
4. Disable the iPhone's Wi-Fi, keeping mobile data and Tailscale enabled. Return
   to the app. Wait for synchronization. Type `printf 'MOBILE_NETWORK_OK\n'`
   on the Mac and verify output on the iPhone. Record time and result, without
   photographing QR codes or tokens.
5. Put the iPhone app in the background for one minute. Create another pane on
   the Mac. Return to the iPhone: the list must reflect current panes, without
   showing stale state as connected. Close the extra pane on the Mac and verify
   its removal on the iPhone.
6. Close and reopen the iPhone app. Pairing must remain saved in Keychain;
   no new QR is needed while the credential has not been revoked.

## Restart the Mac

1. Save personal work before restarting the computer through the system menu.
   Reboot is a manual action in this procedure; no test script triggers it.
2. After login, wait for the two new services. Open the new Mac and iPhone apps.
3. Verify the same workspace, pane and directory. The shell must have a new
   generation and a “new shell” indicator. The previous process does not survive
   reboot; a numeric PID that happens to be reused does not prove continuity.
4. Recorded `SLICE_1_ACCEPTANCE` output must appear in replay, followed by output
   from the new shell. The last seconds before reboot may be missing.
   A declared gap must appear as such, never as false continuity.
5. Type `printf 'AFTER_REBOOT\n'; pwd` on the Mac. Verify output on the Mac and
   iPhone and the expected directory. Record IDs and the new generation.
6. Close the test pane through the Mac. Reopen clients and confirm it did not
   return. Do not delete other profiles' data or services during cleanup.

## Revoke

On the Mac, open the pairing window, find the device and choose “Revoke”.
Confirm. The iPhone must lose access, clear the visible screen and request
new pairing. Reopening the app must not restore a revoked credential.

## Record the result

For each step, record date, OS version, commit, result and evidence without
secrets. Mark separately: mobile-network reading, return from background,
credential after reopening, restoration after reboot and revocation. If any fails,
record operation IDs from the new profile's logs and open a fix; do not mark
manual acceptance as passed based only on a build or Simulator.
