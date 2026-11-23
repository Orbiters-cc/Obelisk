//! What a game's guest on the Linux engine writes to its standard error, on
//! its way to the game's log. A renderer that retried after its graphics went
//! for good wrote 420 MB in five minutes; whatever it writes, and whatever
//! becomes of this side, the game never blocks on its log and never ends by
//! it:
//!
//! - the guest writes into a pipe as before 0.1.9, one whose writes block;
//! - here one thread always reads it into a bounded queue, and another filters
//!   the queue into the log: a disk that stands still, or is full, costs
//!   lines, which are counted, and the count is written once it can be. The
//!   reading thread waits for nothing but the pipe;
//! - beside the supervisor, a process of its own holds the pipe's reading end
//!   too, unread, and drains it to nothing once that thread, or the whole
//!   supervisor, is gone: the game meets neither a full pipe nor a broken one.
//!   It holds the engine's pin with it, as long as the game writes;
//! - a pipe, a thread or a log that cannot be had is no reason to refuse a
//!   game: its output goes nowhere then, and the session's record says why.
//!
//! The filter keeps a repeated line 200 times, then counts it. The lines the
//! frame counters and the incident capture read pass as they are. Every class
//! of lines has its budget, and all of them together a cap ([`Limits`]): the cap of
//! what the guest wrote. What the engine's own programs on the Mac write
//! (`metal-HUD:`, the window server's) goes to the log beside this, never
//! through it, and is not in that cap.
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    os::{fd::{AsFd, OwnedFd}, unix::process::CommandExt},
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex, mpsc},
    time::{Duration, Instant},
};

/// The launcher's variable that names its guest's end of the pipe, or
/// [`NOWHERE`].
pub(crate) const END: &str = "OBELISK_LOG_PIPE";
/// The guest's output goes nowhere.
pub(crate) const NOWHERE: &str = "none";
/// That end has a number no smaller: the launcher moves it where it wants.
pub(crate) const LOWEST: i32 = 20;

/// Waits until its standard error ends (the reader is gone), then reads its
/// standard input to nothing until its last writer is. Its standard output is
/// what it holds meanwhile.
const DRAIN_ALONE: &str =
    "trap '' TERM INT HUP; exec 3<&2 2>/dev/null 4>&1 >/dev/null; read -r _ <&3; exec 3<&-; exec /bin/cat";

/// The lines that always pass, within their own budgets, by what they hold.
/// Frame counters read the first two.
const MARKS: [&[u8]; 8] = [
    b"neptune unixlib wsi",
    b"metal-HUD",
    b"guest swapchain",
    b"wsd:",
    b"obelisk-wait:",
    b"destroying Neptune context",
    b"CONTEXT_LOST",
    b"lkd:",
];
const COUNTERS: usize = 2;

/// How much of a game's output its log takes.
#[derive(Clone, Copy, Debug)]
struct Limits {
    /// How often a repeated line is written as it is.
    repeated: u64,
    /// The kinds of line counted at once.
    kinds: usize,
    /// The bytes of a line, of which the rest is cut.
    line: usize,
    /// The bytes of the lines with no mark.
    ordinary: u64,
    /// The bytes of the lines of one frame counter, and of another mark.
    counter: u64,
    marked: u64,
    /// Past its budget, a mark still passes one line this often, for this
    /// many bytes: a count that goes on is never taken for one that stopped.
    trickle_every: Duration,
    trickle: u64,
    /// The bytes of what this writes of its own.
    notices: u64,
    /// The bytes waiting for the log.
    queue: usize,
}

const LIMITS: Limits = Limits {
    repeated: 200,
    kinds: 4096,
    line: 4096,
    ordinary: 64 << 20,
    counter: 32 << 20,
    marked: 4 << 20,
    trickle_every: Duration::from_secs(10),
    trickle: 2 << 20,
    notices: 1 << 20,
    queue: 8 << 20,
};

impl Limits {
    /// The most a log ever takes.
    #[cfg(test)]
    fn total(&self) -> u64 {
        let counters = COUNTERS as u64;
        let marks = MARKS.len() as u64;
        self.ordinary
            + counters * self.counter
            + (marks - counters) * self.marked
            + marks * self.trickle
            + self.notices
    }
}

/// Which lines of a guest's output its log takes, and the totals of the rest.
struct Filter {
    limits: Limits,
    partial: Vec<u8>,
    /// The line under way was cut: its rest is passed over.
    cut: bool,
    seen: HashMap<Vec<u8>, u64>,
    ordinary: u64,
    /// The lines with no mark that its budget left out.
    over: u64,
    marks: [Mark; MARKS.len()],
    notices: u64,
    /// The game is gone: what is kept for the last notices may be used.
    ending: bool,
}

#[derive(Clone, Copy, Default)]
struct Mark {
    bytes: u64,
    trickled: u64,
    last: Option<Instant>,
    left_out: u64,
}

/// A line's kind: its text, whatever its numbers.
fn kind(line: &[u8]) -> Vec<u8> {
    let mut kind = Vec::with_capacity(line.len().min(256));
    let mut rest = line;
    while let Some((&byte, after)) = rest.split_first() {
        if kind.len() >= 256 {
            break;
        }
        let hexadecimal = rest.starts_with(b"0x") && rest.get(2).is_some_and(u8::is_ascii_hexdigit);
        if hexadecimal {
            let digits = rest[2..].iter().take_while(|b| b.is_ascii_hexdigit()).count();
            kind.extend_from_slice(b"0x#");
            rest = &rest[2 + digits..];
        } else if byte.is_ascii_digit() {
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            kind.push(b'#');
            rest = &rest[digits..];
        } else {
            kind.push(byte);
            rest = after;
        }
    }
    kind
}

fn mark(line: &[u8]) -> Option<usize> {
    MARKS
        .iter()
        .position(|mark| line.windows(mark.len()).any(|window| window == *mark))
}

impl Filter {
    fn new(limits: Limits) -> Self {
        Self {
            limits,
            partial: Vec::new(),
            cut: false,
            seen: HashMap::new(),
            ordinary: 0,
            over: 0,
            marks: Default::default(),
            notices: 0,
            ending: false,
        }
    }

    fn feed(&mut self, bytes: &[u8], now: Instant, out: &mut Vec<u8>) {
        for piece in bytes.split_inclusive(|byte| *byte == b'\n') {
            let (text, ended) = match piece.split_last() {
                Some((b'\n', text)) => (text, true),
                _ => (piece, false),
            };
            if !self.cut {
                let room = self.limits.line - self.partial.len();
                self.partial.extend_from_slice(&text[..text.len().min(room)]);
                if text.len() > room {
                    self.cut = true;
                    self.partial.extend_from_slice(b" [obelisk-log: cut]");
                }
            }
            if ended {
                let line = std::mem::take(&mut self.partial);
                self.cut = false;
                self.line(&line, now, out);
            }
        }
    }

    /// What was cut off by lost output is not a line of the game's.
    fn gap(&mut self) {
        self.partial.clear();
        self.cut = false;
    }

    fn line(&mut self, line: &[u8], now: Instant, out: &mut Vec<u8>) {
        let length = line.len() as u64 + 1;
        if let Some(index) = mark(line) {
            let budget = if index < COUNTERS { self.limits.counter } else { self.limits.marked };
            let limits = self.limits;
            let mark = &mut self.marks[index];
            if mark.bytes + length <= budget {
                mark.bytes += length;
            } else if mark.trickled + length <= limits.trickle
                && mark.last.is_none_or(|last| now.saturating_duration_since(last) >= limits.trickle_every)
            {
                mark.trickled += length;
                mark.last = Some(now);
            } else {
                mark.left_out += 1;
                return;
            }
            out.extend_from_slice(line);
            out.push(b'\n');
            return;
        }
        let kind = kind(line);
        if !self.seen.contains_key(&kind) && self.seen.len() >= self.limits.kinds {
            self.totals("until here", out);
            self.seen.clear();
        }
        let count = self.seen.entry(kind).or_default();
        *count += 1;
        let count = *count;
        if count > self.limits.repeated {
            if matches!(count, 1_000 | 10_000 | 100_000) || count.is_multiple_of(1_000_000) {
                let repeated = self.limits.repeated;
                self.notice(
                    format!(
                        "{count} lines like this so far, the ones after the first {repeated} left out: {}",
                        String::from_utf8_lossy(line)
                    ),
                    out,
                );
            }
        } else if self.ordinary + length <= self.limits.ordinary {
            self.ordinary += length;
            out.extend_from_slice(line);
            out.push(b'\n');
        } else {
            self.over += 1;
            if self.over == 1 || self.over.is_multiple_of(1_000_000) {
                let over = self.over;
                self.notice(format!("the log is full: {over} lines left out so far"), out);
            }
        }
    }

    fn notice(&mut self, text: String, out: &mut Vec<u8>) {
        let line = format!("[obelisk-log: {text}]\n");
        let budget = if self.ending { self.limits.notices } else { self.limits.notices / 2 };
        if self.notices + line.len() as u64 <= budget {
            self.notices += line.len() as u64;
            out.extend_from_slice(line.as_bytes());
        }
    }

    fn totals(&mut self, when: &str, out: &mut Vec<u8>) {
        let mut repeated: Vec<_> = self
            .seen
            .iter()
            .filter(|(_, count)| **count > self.limits.repeated)
            .map(|(kind, count)| (*count, String::from_utf8_lossy(kind).into_owned()))
            .collect();
        repeated.sort_unstable_by(|a, b| b.cmp(a));
        // The most repeated ones tell what flooded the log.
        for (count, kind) in repeated.into_iter().take(64) {
            self.notice(format!("{count} lines like this {when}: {kind}"), out);
        }
    }

    /// The last line, and what was left out in all.
    fn end(&mut self, now: Instant, out: &mut Vec<u8>) {
        if !self.partial.is_empty() {
            let line = std::mem::take(&mut self.partial);
            self.line(&line, now, out);
        }
        self.ending = true;
        if self.over > 0 {
            let over = self.over;
            self.notice(format!("the log was full: {over} lines left out in all"), out);
        }
        for (index, name) in MARKS.iter().enumerate() {
            let left_out = self.marks[index].left_out;
            if left_out > 0 {
                let name = String::from_utf8_lossy(name);
                self.notice(format!("{left_out} lines with \"{name}\" left out past their budget"), out);
            }
        }
        self.totals("in all", out);
    }
}

/// What never reached the filter or the log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Lost {
    /// Bytes read that found the queue full, the log standing still.
    pub dropped: u64,
    /// Bytes the log could not be written.
    pub unwritten: u64,
}

#[derive(Default)]
struct Queue {
    chunks: VecDeque<Chunk>,
    bytes: usize,
    closed: bool,
    lost: Lost,
}

enum Chunk {
    Bytes(Vec<u8>),
    /// Output lost here.
    Gap(u64),
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
}

/// A pipe for a guest's output, before its game is spawned: the guest's end
/// and what takes the other to its log. `held` is kept open for as long as
/// anything holds the guest's end, whatever becomes of this process.
pub(crate) fn open(held: Stdio) -> io::Result<(OwnedFd, Writer)> {
    open_with(held, LIMITS)
}

fn open_with(held: Stdio, limits: Limits) -> io::Result<(OwnedFd, Writer)> {
    let (read, write) = io::pipe()?;
    let (alone, reading) = io::pipe()?;
    let mut drain = Command::new("/bin/sh");
    drain
        .args(["-c", DRAIN_ALONE])
        .env_clear()
        .current_dir("/")
        .process_group(0)
        .stdin(read.try_clone()?)
        .stdout(held)
        .stderr(alone);
    let mut drain = drain.spawn()?;
    // Reaped whenever it ends; a thread that cannot be had costs nothing.
    let _ = std::thread::Builder::new().name("game-log-drain".into()).spawn(move || {
        let _ = drain.wait();
    });
    let writer = Writer::start(read.into(), reading.into(), limits)?;
    Ok((write.into(), writer))
}

/// A game's output on its way to its log.
pub(crate) struct Writer {
    shared: Arc<Shared>,
    destination: Option<mpsc::Sender<Box<dyn Write + Send>>>,
    done: mpsc::Receiver<()>,
}

/// The pipe is looked at this often even with nothing in it.
const LOOK: i32 = 100;
/// How long what is left in the pipe gets once the game is gone, and how long
/// the log then gets to take it.
const DRAIN: Duration = Duration::from_secs(2);

impl Writer {
    /// Reads `read`; `reading` is closed as that ends, however it does.
    fn start(read: OwnedFd, reading: OwnedFd, limits: Limits) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let (destination, opened) = mpsc::channel();
        let (ended, done) = mpsc::channel();
        std::thread::Builder::new().name("game-log-read".into()).spawn({
            let shared = shared.clone();
            move || {
                let _reading = reading;
                take(read, &shared, limits.queue);
            }
        })?;
        let writing = std::thread::Builder::new().name("game-log-write".into()).spawn({
            let shared = shared.clone();
            move || {
                if let Ok(destination) = opened.recv() {
                    write(destination, &shared, limits);
                }
                let _ = ended.send(());
            }
        });
        if let Err(error) = writing {
            shared.queue.lock().unwrap().closed = true;
            return Err(error);
        }
        Ok(Self { shared, destination: Some(destination), done })
    }

    /// Where the game's output goes from now on, what came before included.
    pub(crate) fn write_to(&mut self, destination: impl Write + Send + 'static) {
        if let Some(sender) = self.destination.take() {
            let _ = sender.send(Box::new(destination));
        }
    }

    /// Ends once the game is gone: writes what its pipe still holds, for at
    /// most a few seconds, whatever still holds the pipe or the log.
    pub(crate) fn finish(mut self) -> Lost {
        self.destination.take();
        let deadline = Instant::now() + DRAIN;
        let mut queue = self.shared.queue.lock().unwrap();
        while !queue.closed && Instant::now() < deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            queue = self.shared.changed.wait_timeout(queue, left).unwrap().0;
        }
        queue.closed = true;
        drop(queue);
        self.shared.changed.notify_all();
        let _ = self.done.recv_timeout(DRAIN);
        self.shared.queue.lock().unwrap().lost
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.shared.queue.lock().unwrap().closed = true;
        self.shared.changed.notify_all();
    }
}

/// Reads the pipe into the queue, until its last writer is gone or the queue
/// is closed. It never waits on the log.
fn take(read: OwnedFd, shared: &Shared, capacity: usize) {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let mut pipe = std::fs::File::from(read);
    let mut buffer = vec![0; 64 << 10];
    loop {
        if shared.queue.lock().unwrap().closed {
            return;
        }
        let wait = Timespec { tv_sec: 0, tv_nsec: i64::from(LOOK) * 1_000_000 };
        match poll(&mut [PollFd::new(&pipe.as_fd(), PollFlags::IN)], Some(&wait)) {
            Ok(0) | Err(rustix::io::Errno::INTR) => continue,
            Ok(_) => {}
            Err(_) => break,
        }
        let read = match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let mut queue = shared.queue.lock().unwrap();
        if queue.bytes + read <= capacity {
            queue.bytes += read;
            match queue.chunks.back_mut() {
                Some(Chunk::Bytes(last)) if last.len() + read <= buffer.len() => last.extend_from_slice(&buffer[..read]),
                _ => queue.chunks.push_back(Chunk::Bytes(buffer[..read].to_vec())),
            }
        } else {
            queue.lost.dropped += read as u64;
            match queue.chunks.back_mut() {
                Some(Chunk::Gap(bytes)) => *bytes += read as u64,
                _ => queue.chunks.push_back(Chunk::Gap(read as u64)),
            }
        }
        drop(queue);
        shared.changed.notify_all();
    }
    shared.queue.lock().unwrap().closed = true;
    shared.changed.notify_all();
}

/// Filters the queue into `log` until the queue is closed and empty. A write
/// that fails costs its bytes, counted, and is said once one succeeds.
fn write(mut log: Box<dyn Write + Send>, shared: &Shared, limits: Limits) {
    let mut filter = Filter::new(limits);
    // Bytes that could not be written and were not told yet.
    let mut untold = 0_u64;
    let mut put = |filter: &mut Filter, out: Vec<u8>, shared: &Shared| {
        if out.is_empty() {
            return;
        }
        let mut whole = Vec::new();
        if untold > 0 {
            // After a write cut short, what comes next begins its own line.
            whole.push(b'\n');
            filter.notice(format!("{untold} bytes could not be written here"), &mut whole);
        }
        whole.extend_from_slice(&out);
        match log.write_all(&whole).and_then(|()| log.flush()) {
            Ok(()) => untold = 0,
            Err(_) => {
                untold += out.len() as u64;
                shared.queue.lock().unwrap().lost.unwritten += out.len() as u64;
            }
        }
    };
    loop {
        let mut queue = shared.queue.lock().unwrap();
        while queue.chunks.is_empty() && !queue.closed {
            queue = shared.changed.wait(queue).unwrap();
        }
        let chunk = queue.chunks.pop_front();
        if let Some(Chunk::Bytes(bytes)) = &chunk {
            queue.bytes -= bytes.len();
        }
        drop(queue);
        let mut out = Vec::new();
        match chunk {
            Some(Chunk::Bytes(bytes)) => filter.feed(&bytes, Instant::now(), &mut out),
            Some(Chunk::Gap(bytes)) => {
                filter.gap();
                filter.notice(format!("{bytes} bytes of the game's output dropped here, the log stood still"), &mut out);
            }
            None => break,
        }
        put(&mut filter, out, shared);
    }
    let mut out = Vec::new();
    filter.end(Instant::now(), &mut out);
    let lost = shared.queue.lock().unwrap().lost;
    if lost != Lost::default() {
        let Lost { dropped, unwritten } = lost;
        filter.notice(format!("output lost in all: {dropped} bytes dropped, {unwritten} bytes not written"), &mut out);
    }
    put(&mut filter, out, shared);
}

#[cfg(test)]
mod tests;
