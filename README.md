# tecla

A minimalist typing speed test for the terminal, inspired by [Monkeytype] and built with
[Ratatui].

Type the random words before the 30 second timer runs out. The timer starts on your first
keypress. Results are saved to a local history file.

## Running

```sh
cargo run --release
```

## Keybindings

| Screen  | Key                                         | Action                    |
| ------- | ------------------------------------------- | ------------------------- |
| Typing  | any character                               | type                      |
| Typing  | `Space`                                     | move to the next word     |
| Typing  | `Backspace`                                 | delete the last character |
| Typing  | `Ctrl-Backspace`, `Alt-Backspace`, `Ctrl-W` | delete the current word   |
| Typing  | `Tab`                                       | restart with new words    |
| Results | `Tab`, `Enter`                              | start a new test          |
| Any     | `Esc`, `Ctrl-C`                             | quit                      |

Input is matched word by word, like Monkeytype. Letters typed past the end of a word stay in
that word as extra letters, and pressing space moves on to the next word, leaving any skipped
letters missed. Backspace can go back into the previous word only if it has a mistake.

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

Each finished test is appended as one JSON line (timestamp, mode, duration, wpm, raw,
accuracy) to `history.jsonl` in the platform data directory, e.g.
`~/.local/share/tecla/` on Linux.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

[Monkeytype]: https://monkeytype.com
[Ratatui]: https://ratatui.rs
