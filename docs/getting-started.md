# Getting started

This guide takes you from the download to a terminal, an agent, and a paired iPhone or iPad. The
button and menu names below are the ones the apps show. For what works on each system in 0.1.0,
read "Status" in the [README](../README.md).

## What you need

- A computer to run Soyeht: a Mac with Apple silicon (macOS 15 or later), a Windows 11 PC, or a
  Linux PC. The terminals and agents run on this computer, and your phone is a window onto it.
- **Git** and **Node.js**. Soyeht lists them as required, and starts the agents' adapters through
  Node.js. The Setup list (below) shows the command that installs each one that is missing.
- At least one agent: Claude Code, Codex, Cursor or OpenCode. The list says "one is enough".
- To use a phone: [Tailscale](https://tailscale.com) on the computer and on the phone, signed in
  to the same tailnet, and a Mac (see "Pair an iPhone or iPad").

## Install

Download the file for your system from the
[releases page](https://github.com/soyeht/soyeht/releases). Each release lists a `SHA256SUMS`
file you can check the download against.

| System | File | Install |
| --- | --- | --- |
| macOS | `soyeht-<version>-macos-aarch64.dmg` | Open it and drag the app to Applications. |
| Windows | `soyeht-<version>-windows-x86_64.exe`, or `-windows-aarch64.exe` on an Arm PC | Run it. It installs for your user and needs no administrator. A box on the installer, off by default, says "Start Soyeht when I log in". |
| Linux | `soyeht-<version>-linux-<arch>.deb` and `.AppImage` | `sudo apt install ./soyeht-<version>-linux-<arch>.deb`, or make the AppImage executable and run it. |

On Linux the machine must provide a few libraries, and web panes need GTK 3 and WebKitGTK 4.1;
[docs/platforms.md](platforms.md) lists them. The iPhone and iPad app comes from the App Store.

## First launch

**On the Mac**, the app may ask two questions:

- "Move Soyeht to Applications?" when it runs from somewhere else, such as the disk image. Choose
  "Move to Applications". The engine needs the app there so macOS remembers the permissions you give it.
- "Keep Soyeht running in the background?" Choose "Turn on" to keep your terminals and agents
  running when you close the window, and to start Soyeht when you log in. "Not now" asks nothing
  more; you can turn it on later from Preferences. If the engine is not running, the setup list
  shows a "Start Soyeht" button.

**On Windows and Linux**, the window starts the engine and the supervisor itself when it finds
none. It asks the same question, "Keep Soyeht running in the background?", with the answers
"Turn on" and "Not now". The setting stays in Settings, under Setup, as "Start Soyeht at login".
On Linux you can also run `soyeht service install` once (`./Soyeht.AppImage soyeht service install`
for the AppImage).

## Check the setup

The setup list says what this computer has of what Soyeht needs. Its headline is one of "Checking
this machine", "Install an agent to start" or "Everything is ready". Each missing item shows the
command that installs it on your system, with a button to copy the command. After you run it,
choose "Check again".

- **Mac:** the list is on the empty workspace, and in Preferences.
- **Windows and Linux:** it is on the welcome screen, and in Settings under Setup.

## Install and sign in to an agent

Under "Agents · one is enough" the list names the four supported agents (Claude Agent, Codex,
Cursor and OpenCode) with their install commands. Run the command for the one you use, in a
terminal, and choose "Check again". You can also use any other program that runs in a terminal;
open it in a terminal pane.

An agent is listed with one of these states: "Ready", "Installed", "Not checked", "Sign in",
"Check setup" or "Not installed". Preferences (Harnesses on the Mac) says why for each one.

The first time you start an agent that is not signed in, its pane shows "Sign in to continue"
with the methods the agent offers. The sign-in runs in a terminal pane next to the agent and you
finish it in your browser ("Finish signing in"). You sign in to the agent's provider, with your
own account. The first start of some agents downloads the agent's adapter package; the picker says
so before you start it, and [PRIVACY.md](../PRIVACY.md) says what is downloaded.

## Open a workspace, a terminal and an agent

A workspace is a folder. Choosing a folder opens the workspace for it, or the one that already
exists.

**On the Mac**

1. In the empty workspace choose "New pane", or choose the "+" in the top bar ("Open a terminal
   or agent pane").
2. Choose the folder with the "Project" button or the "+" in the form ("Choose a directory"),
   then "Choose".
3. For an agent, which is the default, type the first message under "Ask anything", pick the
   agent with the chip that shows its name, and pick a permission mode if the agent
   offers one. Send with the arrow button or Shift-Command-Return.
4. For a terminal, choose "Open a terminal" ("Prefer the command line?"), then the "Terminal"
   button.

The rail on the left switches between the places of a workspace: Agents, Editor, Web, Tasks,
Terminal, iPhone, Texts, Presentations and Screenplays.

**On Windows and Linux**

1. On the welcome screen, "Open a folder" opens a workspace and a terminal in it. "Start with an
   agent" opens the form: choose the folder ("Choose folder"), choose the agent ("Choose agent"),
   describe what you want under "Describe what you want to make", and press Enter.
2. Later, the "+" in the top bar ("Open a terminal, editor or agent pane") lists "Start with an
   agent…", "Folder…" and, under "Open a pane", Terminal, Editor, Text, Presentation, Screenplay,
   Web (where the system has a web view) and each installed agent.

## Pair an iPhone or iPad

A phone connects to a Mac that runs Soyeht, and shows the same panes. Pairing needs the Mac app:
**on Windows and Linux a phone cannot be paired yet**, because the app cannot yet prove itself to
the engine ([ADR 0162](adr/0162-pairing-is-decided-only-by-the-signed-app-on-every-system.md),
[ADR 0163](adr/0163-opening-a-pairing-window-is-the-signed-apps-alone.md)).

1. Install Soyeht on the iPhone or iPad, and keep Tailscale connected on both devices, on the
   same tailnet.
2. On the Mac, choose "iPhone" in the rail (Command-6), then "Pair iPhone or iPad". The sheet
   "Continue on iPhone or iPad" shows a QR code that is valid for 10 minutes. If it says "Not
   reachable from your devices", the line after it says why; with no Tailscale address the code
   cannot be used.
3. On the phone, choose "Scan Mac QR" and allow the camera. If the camera is not available, open
   "Use a pairing link", paste the link and choose "Connect".
4. The phone shows "Confirm on the Mac" and a code of four groups of four characters. The Mac
   shows "Allow access for" and the phone's name, with its own code.
5. **Compare the two codes.** Choose "Allow" only if they are the same. If the Mac shows another
   code, or none, choose "Deny" on the Mac and "Cancel" on the phone.

A paired phone can type in your terminals and answer your agents, so it has the power of the
person at the Mac. In the same sheet, "Revoke" ends a phone's access; on the phone, "Forget this
Mac" removes the pairing.

## Logs and diagnostics

When something goes wrong, use "Copy diagnostics". It puts a report of this Soyeht and its recent
logs on the clipboard, with paths, names, addresses and tokens removed, to paste into an issue.
Read it before you paste it.

- **Mac:** Preferences, General, Diagnostics, "Copy diagnostics". If the engine is not running,
  the button is also in the setup list. Open Preferences from the gear in the rail, or with
  Command-comma.
- **Windows and Linux:** Settings, Setup, Diagnostics, "Copy diagnostics".
- **Any system:** `soyeht diagnostics` prints the same report from the command line, and works
  when the app does not open.

The report starts with the version, the system and whether the engine and the supervisor run.
The version is also at the bottom of the Preferences sidebar. If Soyeht closes unexpectedly, the
next launch says "Soyeht closed unexpectedly" and saves a crash report on your computer. Nothing is
sent; "Copy report" puts it on the clipboard.

The logs and crash reports are files in Soyeht's data folder, in `logs/` and `crashes/`:

| System | Data folder |
| --- | --- |
| macOS | `~/Library/Application Support/Soyeht.next` |
| Windows | `%LOCALAPPDATA%\Soyeht.next` |
| Linux | `$XDG_DATA_HOME/soyeht-next`, else `~/.local/share/soyeht-next` |

They record event names and kinds of failure, never the text of your messages or terminals.

To report a problem, [open an issue](https://github.com/soyeht/soyeht/issues/new/choose) with
the version, your system and the diagnostics. Do not paste tokens or pairing codes. To report a
security problem, follow [SECURITY.md](../SECURITY.md).
