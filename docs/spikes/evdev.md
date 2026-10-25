# Spike: reading keyboards with evdev on GNOME 50 Wayland (#4)

Tested on 2026-10-25 on an Acer Aspire A715-75G, Ubuntu, GNOME Shell 50.1, Wayland, with the real `flowbuddyd` rather than a throwaway prototype.

## Result

Reading keys system-wide works on Wayland through `/dev/input/event*`. The compositor doesn't get in the way, because evdev sits below it. The daemon never grabs a device, so every key still reaches the focused app.

## Permissions: `input` group or a udev `uaccess` rule

| | `input` group | udev rule + `uaccess` (recommended) |
|---|---|---|
| Who can read | every process of the user, all the time, including over ssh | only the user at the seat, only while their session is active (logind ACL) |
| Devices | all input devices | only those the rule tags: keyboards and mice |
| Setup | `usermod -aG input`, re-login | `scripts/install.sh --with-udev` (sudo), no re-login needed |
| Undo | remove from group, re-login | `scripts/uninstall.sh` |

Before the rule was installed the daemon reported `denied=20` and `keyboards: 0`. After installing it and triggering udev, the logind ACL gave the user access to exactly these devices:

| Device | udev tags | ACL for the user |
|---|---|---|
| AT Translated Set 2 keyboard (`event4`) | `ID_INPUT_KEYBOARD` | yes |
| SIGMACHIP USB mouse (`event19`) | `ID_INPUT_MOUSE` | yes |
| ELAN touchpad, mouse interface (`event5`) | `ID_INPUT_MOUSE` | yes |
| ELAN touchpad (`event6`) | `ID_INPUT_TOUCHPAD` | no |
| Power and sleep buttons, Acer hotkeys, video bus | `ID_INPUT_KEY` only | no |
| Lid switch, HDA jacks | none | no |

The rule file must sort before `73-seat-late.rules`, which turns the `uaccess` tag into ACLs. That is why it's named `70-`.

## Telling real keyboards from other key devices

Power buttons, hotkey devices and the video bus all have `EV_KEY` and `ID_INPUT_KEY=1`. The daemon treats a device as a keyboard only if it has `KEY_A`, `KEY_Z`, `KEY_J`, `KEY_SPACE`, `KEY_ENTER` and `KEY_ESC`. Anything else with `BTN_LEFT` is a pointer, used only to notice clicks. With that check the only keyboard found was `event4`, which matches the table.

## Key repeat and multiple keyboards

- Holding a key sends `value=2` repeats. These become strokes with `repeat: true`, so holding `j` counts as many `j`s, which is what Vim does too.
- A keyboard plugged in while the daemon runs is picked up through inotify on `/dev/input`. This machine has only one keyboard; a second USB keyboard should behave the same but wasn't tested.

## Follow-ups

- **Touchpad taps aren't seen.** The touchpad device itself isn't tagged, and a tap is turned into a click by libinput inside the compositor, so it never appears as `BTN_LEFT` on any evdev device. Clicks matter because they can change Vim's mode in an editor. A better source is the GNOME extension reporting clicks from the stage, which covers every pointer and would let the udev rule drop mice altogether (#74).
