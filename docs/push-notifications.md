# Push notifications

When an agent pane (Claude Code, Codex or any ACP harness) stops to ask for
permission or asks a question, the engine on the Mac sends an alert straight
to APNs for every paired iPhone or iPad. A long press on a permission alert
shows **Allow** and **Deny**. Both require an unlocked device and answer in the
background through the same `pane.permission.answer` request as the Mac, with
the same rules. A tap opens the app on that pane. A question (a form) only
opens the pane. The protocol is in contract 08 §2.3.

The app asks for notification permission the first time it connects to the
Mac after pairing, never at launch. Each later connection registers the
device's current APNs token with `push.register`.

## Where the key and config go

Push stays off until a config file exists. The engine then logs one line,
`{"event":"engine.push_disabled","reason":"config_missing"}`, and runs as
before. Nothing in the repository holds a key, team id or token, and the
engine never logs them.

1. In the Apple Developer account, create an APNs authentication key
   (Certificates, Identifiers & Profiles → Keys → Apple Push Notifications
   service). Download the `AuthKey_<KEYID>.p8` file and note the key id and
   the team id. One key can serve both environments. Apple also offers
   sandbox-only and production-only keys; use one that covers the
   environment of the app build you install.
2. Make sure the App ID `com.soyeht.next.dev.ios` has the Push Notifications
   capability. The iOS target already declares `aps-environment` in
   `apps/ios/SoyehtPhone.entitlements`; automatic signing adds the capability
   to the provisioning profile.
3. Put the key and config in the engine's data directory, outside the
   repository:

   ```sh
   PUSH="$HOME/Library/Application Support/Soyeht.next/dev/push"
   mkdir -p "$PUSH" && chmod 700 "$PUSH"
   mv ~/Downloads/AuthKey_<KEYID>.p8 "$PUSH/" && chmod 600 "$PUSH/AuthKey_<KEYID>.p8"
   ```

   Then create `$PUSH/apns.json`:

   ```json
   {
     "keyId": "<10-character key id>",
     "teamId": "<10-character team id>",
     "keyPath": "AuthKey_<KEYID>.p8",
     "topic": "com.soyeht.next.dev.ios"
   }
   ```

   `keyPath` is relative to the `push` directory, or absolute. `topic` is the
   app's bundle id. The engine refuses a key that other users can read
   (`reason":"key_readable_by_others"`).
4. Restart only the engine (shells survive):

   ```sh
   launchctl kickstart -k "gui/$(id -u)/com.soyeht.next.dev.engine"
   grep engine.push "$HOME/Library/Application Support/Soyeht.next/dev/logs/engine.log" | tail -1
   ```

   The last line should be `{"event":"engine.push_enabled"}`. Other reasons
   are `config_invalid`, `config_unreadable`, `key_unreadable` and
   `key_invalid`.

## How delivery works

- Token-based authentication: an ES256 JWT (`kid` = key id, `iss` = team id,
  `iat`) signed with the `.p8` key, reused for 50 minutes as Apple requires
  (between 20 and 60).
- HTTP/2 over TLS to `api.sandbox.push.apple.com` or `api.push.apple.com`,
  chosen per device by the environment the phone reported. One connection per
  environment is reused.
- Headers: `apns-push-type: alert`, `apns-topic` = bundle id,
  `apns-priority: 10`, `apns-collapse-id` = request id, `apns-expiration` =
  the request's own deadline, and a fresh `apns-id`.
- A request is pushed once per device. A network error, 429 or 5xx is retried
  after 5 s, 30 s and 120 s. Retries stop as soon as the request is answered,
  cancelled or expires. A 410, `BadDeviceToken` or `DeviceTokenNotForTopic`
  drops that device's token until the phone registers again.
  `ExpiredProviderToken` renews the JWT once.
- When the app reconnects, it removes delivered alerts whose request is no
  longer open. If an action cannot reach the Mac, the app posts the alert
  again so the person can retry.

The payload (at most 4 KB) shows only a short title such as
“Claude needs approval” and the tool and its target, truncated to 160
characters. The shared example is `contracts/vectors/push-permission.json`.
Both the engine and the app tests read it.

## Test with a real device

1. Configure the key as above and confirm `engine.push_enabled`.
2. Build and install the app on the device with your development team:
   `SOYEHT_DEVELOPMENT_TEAM=<team> bash scripts/check_ios.sh --sign`, then
   install from Xcode. A development-signed build gets sandbox tokens; the
   app detects this from its provisioning profile and reports `sandbox`.
3. Pair the device with the Mac. On its first connection the app asks to send
   notifications; allow it. Check the registration (it shows no token):

   ```sh
   sqlite3 "$HOME/Library/Application Support/Soyeht.next/dev/soyeht.db" \
     "SELECT device_id, environment FROM push_registrations"
   ```

4. Open a Claude Code or Codex agent pane in “ask” mode and give it something
   that needs approval. Lock the phone. The alert arrives; long press, choose
   Allow, unlock, and the agent continues on the Mac.
5. If nothing arrives, look for `engine.push_failed` lines in the engine log.
   The `status` and `reason` fields are APNs's (for example
   `BadDeviceToken` when a sandbox token is sent to production). Apple's Push
   Notifications Console can also send a test alert to the token.

## Test in the Simulator

The Simulator cannot reach APNs with this engine's key, but it shows the
same alert and actions: `xcrun simctl push <device> com.soyeht.next.dev.ios
payload.json` with a payload shaped like the shared vector, using a real
`engineId`, `paneId`, `requestId` and option ids from the engine's snapshot.
