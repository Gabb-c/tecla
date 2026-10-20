# tecla

A minimalist typing speed test for the terminal, inspired by [Monkeytype] and built with
[Ratatui].

Type random words against the clock or through a fixed word count. The timer starts on your
first keypress. Results are saved to a local history file.

## Modes

- **time**: type as many words as you can in 15, 30, 60 or 120 seconds.
- **words**: type 10, 25, 50 or 100 words as fast as you can. The test ends when the last word
  is typed correctly, or when you press space on it.

## Running

```sh
cargo run --release
```

## Keybindings

| Screen            | Key                                         | Action                    |
| ----------------- | ------------------------------------------- | ------------------------- |
| Typing            | any character                               | type                      |
| Typing            | `Space`                                     | move to the next word     |
| Typing            | `Backspace`                                 | delete the last character |
| Typing            | `Ctrl-Backspace`, `Alt-Backspace`, `Ctrl-W` | delete the current word   |
| Typing            | `Tab`                                       | restart with new words    |
| Typing            | `Esc`                                       | open settings             |
| Results           | `Tab`, `Enter`                              | start a new test          |
| Results           | `Esc`                                       | open settings             |
| Settings          | `↑`/`↓`, `k`/`j`                            | select a setting          |
| Settings          | `←`/`→`, `h`/`l`, `Enter`, `Space`          | change its value          |
| Settings          | `Esc`                                       | back to a new test        |
| Results, Settings | `q`                                         | quit                      |
| Any               | `Ctrl-C`                                    | quit                      |

Input is matched word by word, like Monkeytype. Letters typed past the end of a word stay in
that word as extra letters, and pressing space moves on to the next word, leaving any skipped
letters missed. Backspace can go back into the previous word only if it has a mistake.

## Settings

Press `Esc` to open the settings screen. Every change is saved right away to `config.toml` in
the XDG config directory:

| Platform     | Path                                                                |
| ------------ | ------------------------------------------------------------------- |
| Any          | `$XDG_CONFIG_HOME/tecla/config.toml`, when `XDG_CONFIG_HOME` is set |
| Linux, macOS | `~/.config/tecla/config.toml`                                       |
| Windows      | `%APPDATA%\tecla\config.toml`                                       |

The file can also be edited by hand; missing keys take their defaults, and unknown keys are an
error. Saving from the settings screen keeps your comments and layout. If the file fails to load,
tecla starts with the defaults, shows the error, and leaves the file alone until you fix it.

```toml
mode = "time"          # time | words
time = 30              # seconds, for time mode
words = 25             # word count, for words mode
caret = "block"        # block | bar | underline | off
caret_blink = false
live_progress = true   # countdown, or words typed in words mode
live_wpm = false
live_accuracy = false
live_errors = false    # mistakes so far, including corrected ones
confidence = "off"     # off | on (no going back to previous words) | max (no backspace)
stop_on_error = "off"  # off | letter (wrong letters rejected) | word (space needs a correct word)
blind = false          # hide mistakes while typing
key_tips = true
```

In the file, `time` accepts 1 to 3600 seconds and `words` accepts 1 to 10000; the settings screen
cycles through the presets above. A keystroke rejected by `stop_on_error` still counts against accuracy.

## Metrics

A character is one grapheme cluster (so `é` counts once), and a word is five characters.

- **Net WPM**: (characters of correct words, plus the space after each, plus the current word's
  characters if it is correct so far / 5) / minutes elapsed
- **Raw WPM**: (all typed characters, extra ones and spaces included / 5) / minutes elapsed
- **Accuracy**: correct keystrokes / total character keystrokes. A mistake you fix later still
  counts as a mistake. Backspace is not a character keystroke.

The results screen also counts letters as correct, incorrect, extra (typed past the end of a
word) and missed (skipped by moving on to the next word).

## History

Each finished test is appended as one JSON line (timestamp, mode, duration, word count in words
mode, wpm, raw, accuracy) to `history.jsonl` in the XDG data directory:

| Platform     | Path                                                                |
| ------------ | ------------------------------------------------------------------- |
| Any          | `$XDG_DATA_HOME/tecla/history.jsonl`, when `XDG_DATA_HOME` is set   |
| Linux, macOS | `~/.local/share/tecla/history.jsonl`                                |
| Windows      | `%APPDATA%\tecla\history.jsonl`                                     |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

[Monkeytype]: https://monkeytype.com
[Ratatui]: https://ratatui.rs
