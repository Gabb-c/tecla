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

| Screen  | Key            | Action                        |
| ------- | -------------- | ----------------------------- |
| Typing  | any character  | type                          |
| Typing  | `Backspace`    | delete the last character     |
| Typing  | `Tab`          | restart with new words        |
| Results | `Tab`, `Enter` | start a new test              |
| Any     | `Esc`, `Ctrl-C`| quit                          |

## Metrics

A character is one grapheme cluster (so `é` counts once), and a word is five characters.

- **Net WPM**: (correctly typed characters in the final input / 5) / minutes elapsed
- **Raw WPM**: (all typed characters in the final input / 5) / minutes elapsed
- **Accuracy**: correct keystrokes / total character keystrokes. A mistake you fix later still
  counts as a mistake. Backspace is not a character keystroke.

## History

Each finished test is appended as one JSON line (timestamp, mode, duration, wpm, raw,
accuracy) to `history.jsonl` in the platform data directory, e.g.
`~/.local/share/tecla/` on Linux.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

[Monkeytype]: https://monkeytype.com
[Ratatui]: https://ratatui.rs
