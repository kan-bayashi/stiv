// Copyright 2025 Tomoki Hayashi
// MIT License (https://opensource.org/licenses/MIT)

//! Terminal output writer.
//!
//! This module is the only place allowed to write to stdout. It serializes output and prevents
//! escape-sequence interleaving across threads.
//!
//! Key properties:
//! - Status updates are prioritized and flushed immediately.
//! - Image output is chunked at safe boundaries (KGP chunks and per-row placement/erase).
//! - Image output can be cancelled on navigation.

use std::collections::VecDeque;
use std::io::{IsTerminal, Write, stdout};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use ratatui::layout::Rect;

use crate::kgp::{delete_all, erase_rows, place_rows};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusIndicator {
    Busy,
    Ready,
}

pub enum WriterRequest {
    /// Update the status row (single-line HUD at the bottom).
    Status {
        text: String,
        size: (u16, u16),
        indicator: StatusIndicator,
    },
    /// Transmit image bytes (KGP) and place the image in the terminal area.
    ImageTransmit {
        encoded_chunks: Vec<Vec<u8>>,
        area: Rect,
        kgp_id: u32,
        old_area: Option<Rect>,
        epoch: u64,
    },
    /// Place a previously transmitted image in the terminal area.
    ImagePlace {
        area: Rect,
        kgp_id: u32,
        old_area: Option<Rect>,
        epoch: u64,
    },
    /// Clear any KGP overlays (used on shutdown).
    ClearAll {
        area: Option<Rect>,
        is_tmux: bool,
    },
    /// Cancel an in-flight image task (best-effort).
    CancelImage {
        kgp_id: Option<u32>,
        is_tmux: bool,
        area: Option<Rect>,
        epoch: u64,
    },
    Shutdown,
}

pub struct WriterResult {
    pub kind: WriterResultKind,
    pub epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriterResultKind {
    TransmitDone { kgp_id: u32 },
    PlaceDone { kgp_id: u32 },
}

struct Task {
    chunks: VecDeque<Vec<u8>>,
    complete: Option<WriterResultKind>,
    epoch: u64,
}

pub struct TerminalWriter {
    request_tx: Sender<WriterRequest>,
    result_rx: Receiver<WriterResult>,
    handle: Option<JoinHandle<()>>,
}

impl TerminalWriter {
    /// Spawn the writer thread.
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<WriterRequest>();
        let (result_tx, result_rx) = mpsc::channel::<WriterResult>();

        let handle = thread::spawn(move || {
            Self::writer_loop(request_rx, result_tx);
        });

        Self {
            request_tx,
            result_rx,
            handle: Some(handle),
        }
    }

    /// Send a request to the writer thread.
    pub fn send(&self, req: WriterRequest) {
        let _ = self.request_tx.send(req);
    }

    /// Poll for completion notifications (e.g. transmit finished for a `kgp_id`).
    pub fn try_recv(&self) -> Option<WriterResult> {
        self.result_rx.try_recv().ok()
    }

    fn writer_loop(request_rx: Receiver<WriterRequest>, result_tx: Sender<WriterResult>) {
        let mut out = stdout();
        let is_tty = out.is_terminal();

        let mut last_status: Option<(String, (u16, u16), StatusIndicator)> = None;
        let mut status_dirty = false;
        let mut current_task: Option<Task> = None;
        let mut current_epoch: u64 = 0;
        let mut should_quit = false;
        let mut bytes_since_flush: usize = 0;
        const FLUSH_THRESHOLD: usize = 64 * 1024;

        loop {
            if should_quit {
                break;
            }

            if current_task.is_none() && !status_dirty {
                match request_rx.recv() {
                    Ok(msg) => Self::apply_msg(
                        msg,
                        &mut should_quit,
                        &mut last_status,
                        &mut status_dirty,
                        &mut current_task,
                        is_tty,
                        &mut out,
                        &mut current_epoch,
                    ),
                    Err(_) => break,
                }
            }

            while let Ok(msg) = request_rx.try_recv() {
                Self::apply_msg(
                    msg,
                    &mut should_quit,
                    &mut last_status,
                    &mut status_dirty,
                    &mut current_task,
                    is_tty,
                    &mut out,
                    &mut current_epoch,
                );
                if should_quit {
                    break;
                }
            }

            if status_dirty {
                if let Some((text, size, indicator)) = last_status.clone() {
                    if is_tty {
                        let _ = Self::render_status(&mut out, &text, size, indicator);
                        let _ = out.flush();
                    }
                    bytes_since_flush = 0;
                }
                status_dirty = false;
            }

            if let Some(task) = &mut current_task {
                if task.epoch != current_epoch {
                    current_task = None;
                    continue;
                }
                if !is_tty {
                    if let Some(kind) = task.complete {
                        let _ = result_tx.send(WriterResult {
                            kind,
                            epoch: task.epoch,
                        });
                    }
                    current_task = None;
                    continue;
                }
                if let Some(chunk) = task.chunks.pop_front() {
                    if !chunk.is_empty() {
                        let _ = out.write_all(&chunk);
                        bytes_since_flush = bytes_since_flush.saturating_add(chunk.len());
                        if bytes_since_flush >= FLUSH_THRESHOLD {
                            let _ = out.flush();
                            bytes_since_flush = 0;
                        }
                    }
                } else {
                    let _ = out.flush();
                    bytes_since_flush = 0;
                    if let Some(kind) = task.complete {
                        let _ = result_tx.send(WriterResult {
                            kind,
                            epoch: task.epoch,
                        });
                    }
                    current_task = None;
                }
            }
        }
    }

    fn apply_msg(
        msg: WriterRequest,
        should_quit: &mut bool,
        last_status: &mut Option<(String, (u16, u16), StatusIndicator)>,
        status_dirty: &mut bool,
        current_task: &mut Option<Task>,
        is_tty: bool,
        out: &mut impl Write,
        current_epoch: &mut u64,
    ) {
        match msg {
            WriterRequest::Shutdown => {
                *should_quit = true;
            }
            WriterRequest::Status {
                text,
                size,
                indicator,
            } => {
                *last_status = Some((text, size, indicator));
                *status_dirty = true;
            }
            WriterRequest::ClearAll { area, is_tmux } => {
                // Preempt current image work.
                *current_task = None;
                if is_tty {
                    let _ = Self::clear_all(out, area, is_tmux);
                    let _ = out.flush();
                }
            }
            WriterRequest::CancelImage {
                kgp_id,
                is_tmux,
                area,
                epoch,
            } => {
                if epoch >= *current_epoch {
                    *current_epoch = epoch;
                    *current_task = None;
                }
                if is_tty {
                    // Do not delete ids here: it's racy (the transmit may already have completed)
                    // and can lead to "Ready but not displayed" until a resize forces a re-send.
                    //
                    // Cancellation is best-effort: stop writing further chunks so status updates
                    // remain responsive.
                    //
                    // Note: We do NOT erase here. Erasing on cancel causes blank screen when
                    // navigation resumes before a new ImagePlace is sent (navigation latch).
                    // Instead, task_place/task_transmit always erase old_area to clean up.
                    let _ = kgp_id;
                    let _ = is_tmux;
                    let _ = area;
                    let _ = out.write_all(b"\x1b[0m");
                    let _ = out.flush();
                }
            }
            WriterRequest::ImageTransmit {
                encoded_chunks,
                area,
                kgp_id,
                old_area,
                epoch,
            } => {
                if epoch < *current_epoch {
                    return;
                }
                *current_epoch = epoch;
                *current_task =
                    Some(Self::task_transmit(encoded_chunks, area, kgp_id, old_area, epoch));
            }
            WriterRequest::ImagePlace {
                area,
                kgp_id,
                old_area,
                epoch,
            } => {
                if epoch < *current_epoch {
                    return;
                }
                *current_epoch = epoch;
                *current_task = Some(Self::task_place(area, kgp_id, old_area, epoch));
            }
        }
    }

    fn task_place(area: Rect, kgp_id: u32, old_area: Option<Rect>, epoch: u64) -> Task {
        let mut chunks = VecDeque::new();

        // Always erase old_area if provided (even if old == new).
        // This ensures partial placement data from cancelled tasks is cleaned up.
        if let Some(old) = old_area {
            for row in erase_rows(old) {
                chunks.push_back(row);
            }
        }

        for row in place_rows(area, kgp_id) {
            chunks.push_back(row);
        }

        Task {
            chunks,
            complete: Some(WriterResultKind::PlaceDone { kgp_id }),
            epoch,
        }
    }

    fn task_transmit(
        encoded_chunks: Vec<Vec<u8>>,
        area: Rect,
        kgp_id: u32,
        old_area: Option<Rect>,
        epoch: u64,
    ) -> Task {
        let mut chunks = VecDeque::new();

        // Always erase old_area if provided (even if old == new).
        // This ensures partial placement data from cancelled tasks is cleaned up.
        if let Some(old) = old_area {
            for row in erase_rows(old) {
                chunks.push_back(row);
            }
        }

        for enc in encoded_chunks {
            chunks.push_back(enc);
        }

        for row in place_rows(area, kgp_id) {
            chunks.push_back(row);
        }

        Task {
            chunks,
            complete: Some(WriterResultKind::TransmitDone { kgp_id }),
            epoch,
        }
    }

    fn clear_all(out: &mut impl Write, area: Option<Rect>, is_tmux: bool) -> std::io::Result<()> {
        if let Some(area) = area {
            for row in erase_rows(area) {
                out.write_all(&row)?;
            }
        }
        out.write_all(&delete_all(is_tmux))?;
        out.write_all(b"\x1b[0m")?;
        Ok(())
    }

    fn render_status(
        out: &mut impl Write,
        status_text: &str,
        size: (u16, u16),
        indicator: StatusIndicator,
    ) -> std::io::Result<()> {
        let (w, h) = size;
        if w == 0 || h == 0 {
            return Ok(());
        }

        let row_1based = h;
        // Reserve 2 columns for "● " prefix.
        let available = w.saturating_sub(2);
        let clipped = clip_utf8(status_text, available as usize);

        // Background first, then ECH so the cleared cells inherit the background.
        write!(out, "\x1b[{row_1based};1H\x1b[37;100m\x1b[{w}X")?;
        write!(out, "\x1b[{row_1based};1H")?;
        match indicator {
            StatusIndicator::Ready => write!(out, "\x1b[32m●")?, // green
            StatusIndicator::Busy => write!(out, "\x1b[31m●")?,  // red
        }
        write!(out, "\x1b[37;100m {clipped}\x1b[0m")?;
        Ok(())
    }
}

impl Drop for TerminalWriter {
    fn drop(&mut self) {
        let _ = self.request_tx.send(WriterRequest::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn clip_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = 0;
    for (i, _) in s.char_indices() {
        if i > max_bytes {
            break;
        }
        end = i;
    }
    &s[..end]
}
