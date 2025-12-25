# Architecture

`sivit` is a terminal image viewer built around Kitty Graphics Protocol (KGP).
The core goal is: keep navigation/status updates responsive even when image rendering or terminal I/O is slow.

## High-level pipeline

There are three concurrent “lanes”:

1. **Main thread** (`src/main.rs`)
   - Reads key events.
   - Updates application state.
   - Decides when to request rendering.
   - Sends status updates.

2. **Worker thread** (`src/worker.rs`)
   - Decodes the image file.
   - Resizes to a target size based on the current terminal size and `Fit`/`Normal`.
   - Encodes the resized image to KGP chunks (`_G ...`) suitable for sending to the terminal.

3. **Terminal writer thread** (`src/sender.rs`)
   - The only component allowed to write to stdout.
   - Prioritizes status updates over image output.
   - Writes image output in “safe boundaries” (KGP chunk boundaries and per-row placement).

## Why a single stdout writer exists

Terminal output is a single ordered stream. If multiple threads write to stdout:

- escape sequences can interleave and corrupt the screen
- cursor/save/restore can be violated
- large image writes can block unrelated status updates

`TerminalWriter` centralizes output, so status writes can preempt image writes safely.

## Output boundaries and preemption

Image output is chunked so the writer can yield between boundaries:

- **Transmit**: KGP encode is split into multiple independent escape sequences (`encode_chunks`).
- **Place / erase**: generated per terminal row (`place_rows` / `erase_rows`).

This allows the writer to:

- flush the status row immediately
- continue image output incrementally

## Cancellation

When the user navigates while an image transmission is in-flight:

- the main thread sends `CancelImage` to the writer
- the writer drops the current image task

This avoids the “wait for large image I/O” feeling during rapid navigation.
