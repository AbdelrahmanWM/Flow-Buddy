# Using Flow Buddy

Every command, the settings, and the AI. New here? `flowbuddy tour` covers the essentials in a few minutes.

## Commands

```sh
flowbuddy tour                    # new here? how it all works, in seven short parts
flowbuddy setup                   # a few questions: how often, your level, AI, text features
flowbuddy doctor                  # checks every piece and says how to fix what's missing
flowbuddy mode balanced           # off | passive | balanced | active
flowbuddy pause 1h                # or 30m, until-tomorrow; `flowbuddy resume`
flowbuddy summary [--week]        # how you're doing vs. before
flowbuddy next                    # the one habit worth working on now
flowbuddy learn                   # today's "something new" and your progress through Vim; `learn --know ciw,marks` / `--forget` / `--reset`, `--list` for the ids
flowbuddy learn --map             # every skill as a coloured tile: you use it, slipping, learning, ready, later (also in Settings → Skills, and VS Code's "Flow Buddy: Skill map")
flowbuddy practice                # a two-minute drill on your weakest habit; `--history` for how drills have gone
flowbuddy watch                   # tips as they come, with what they mean, in a terminal you keep open
flowbuddy tips [words] [-e]       # your recent tips, or those that match; -e says what each command does
flowbuddy explain 'ci"'            # what keys do, piece by piece; `explain --last`: your last slow move, done better
flowbuddy coach [question]        # advice from your AI ([ai]; --prompt shows what it's sent)
flowbuddy review [--apply 1 2]    # your AI proposes tuning the tips; apply what you agree with
flowbuddy rules                   # list; `rules mute|disable|enable|reset <id>`
flowbuddy memory show             # the mistake memory; `memory export`, `memory forget`, `memory fix-adoptions`
flowbuddy report-fp               # the last nudge was wrong
flowbuddy record start|stop       # record what the rules see (nothing typed); `record check FILE`
flowbuddy record tune             # what each rule's threshold would give over your recordings, and what you marked unwanted
```

On GNOME, **Settings…** in Flow Buddy's top-bar menu (or the Extensions app) opens a window with the basics first, every other setting under Advanced, each rule on its own, and the terminal's tools as buttons: check setup, progress, today's and this week's summary, whether tips help, and exporting or deleting what Flow Buddy remembers.

## Settings

Settings live in `~/.config/flowbuddy/config.toml` and are reloaded automatically. You don't have to edit the file: `flowbuddy config` shows the basics, `flowbuddy config list` every setting with what it does, and `flowbuddy config set coaching.quiet_hours 22:00-08:00` changes one (it checks the value, and keeps your comments). `flowbuddy config reset <key>` puts one back. To start somewhere sensible, `flowbuddy config profile` lists a few profiles (Gentle, Learning fast, Just watching, Already good at Vim) and `flowbuddy config profile gentle` applies one, showing what it changes; they only set ordinary settings, so change any afterwards. `flowbuddy config explain <key>` says more about a setting. The file looks like this:

```toml
[coaching]
default_mode = "balanced"
balanced_interval_minutes = 30
quiet_hours = "22:00-08:00"
when = "now"                 # or "pause": tips wait until you stop typing for a moment
nudge_style = "card"              # or "bar"; default: card in active and balanced mode
show_in = "editor"           # tips in Vim or VS Code when their plugin is there; or "desktop" / "both"

[rules."vim.repeat-jk"]
threshold = 8
```


## Practice

`flowbuddy practice` is a two-minute drill in Vim on your weakest habit, the way keybr drills your weakest key: six small rounds (reach the marked line, delete the marked words, empty the brackets), counting your keys against the fewest that would do. It tells you where fewer would do and how you did against last time; `--history` and `flowbuddy learn --report` show how drills have gone. Run it again and you get a different one: after your weakest habit's drill comes the one you haven't done for the longest. Or pick one: `flowbuddy practice vertical` (or `horizontal`, `delete`, `inside`), `:FlowBuddyPractice delete`, or the list in VS Code. It's also `:FlowBuddyPractice` in Vim, **Practice** in the settings window's Tools, and **Flow Buddy: Practice** in VS Code. The text is made up; nothing of yours is opened.

## AI

`flowbuddy coach` asks an AI of your choice (your Claude subscription, an API key, or a local model) for advice based on the mistake memory. It's off until you set it up, and `--prompt` shows exactly what would be sent:

```toml
[learn]
daily = true                                  # one small Vim feature you don't use yet, once a day
level = "beginner"                            # or "intermediate" / "advanced": skip ahead instead of waiting to see you use the basics
challenges = true                             # after a lesson: use it the next time it fits
holdout = 0.1                                 # share of tips held back at random, to measure what tips do (`flowbuddy learn --report`)

[record]
keep_days = 0                                 # e.g. 7: keep a week of recordings for `flowbuddy record tune`

[typing]
accuracy = false                              # true: count corrections (counts only) for `flowbuddy summary`

[ai]
provider = "claude-code"     # your Claude subscription via `claude -p`; or "anthropic" / "openai" with a key
# command = ["ollama", "run", "llama3.2:3b"]  # or any local program: prompt on stdin, answer on stdout
weekly = false               # true: the model writes the weekly progress note and reviews the tips
tips = false                 # true: once a day the model writes a personal sentence for each recent habit
filter = true                # the learned tip filter (no LLM); skips tips you tend to dismiss

[text]                       # opt-in, off by default; secret files are never looked at (docs/privacy.md)
shapes = false               # true: plugins work out which motions an edit could have used (counts only)
ai_snippets = false          # true: `coach` shows your AI a few redacted lines around recent tips
projects = []                # folders snippets may come from, e.g. ["~/code/myapp"]
cloud = false                # true: snippets may go to a cloud provider, not only a local one
exclude = []                 # more files never to look at, e.g. ["*.sql"]
```

### Claude Code as the AI

With [Claude Code](https://code.claude.com) installed and logged in, Flow Buddy can use your Claude subscription, with no API key:

```sh
flowbuddy config set ai.provider claude-code
flowbuddy config set ai.model opus     # optional: an alias (opus, sonnet, haiku) or a full name (claude-opus-5-5)
flowbuddy doctor                       # should say "AI: Claude Code (claude -p)"
flowbuddy coach --prompt               # what Claude would be sent; nothing is sent
flowbuddy coach "how do I delete up to a comma?"
```

Flow Buddy runs `claude -p` in an empty folder, so Claude sees only the prompt: counts and the shapes of your habits, never your keys or text. Two more uses run on their own once turned on: `flowbuddy config set ai.weekly true` for a weekly progress note, and `flowbuddy config set ai.tips true` for a personal sentence on each day's tips.
