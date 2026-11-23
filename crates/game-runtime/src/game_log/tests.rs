use super::*;
use std::{
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

fn filtered(limits: Limits, lines: impl IntoIterator<Item = String>) -> (String, Filter) {
    let mut filter = Filter::new(limits);
    let mut out = Vec::new();
    for line in lines {
        filter.feed(format!("{line}\n").as_bytes(), Instant::now(), &mut out);
    }
    filter.end(Instant::now(), &mut out);
    (String::from_utf8(out).unwrap(), filter)
}

#[test]
fn a_repeated_line_is_kept_then_counted_whatever_its_numbers() {
    let (log, _) = filtered(
        LIMITS,
        (0..12_000).map(|n| format!("npt: failed at 0x{n:x} (attempt {n})")),
    );
    assert_eq!(log.lines().filter(|l| l.starts_with("npt: ")).count(), 200);
    assert!(log.contains("[obelisk-log: 1000 lines like this so far"));
    assert!(log.contains("[obelisk-log: 10000 lines like this so far"));
    assert!(log.ends_with(
        "[obelisk-log: 12000 lines like this in all: npt: failed at 0x# (attempt #)]\n"
    ));
}

/// Finding 6: no class of lines, however distinct and whatever they hold,
/// takes the log past its cap.
#[test]
fn every_class_of_lines_is_bounded_and_so_is_the_whole() {
    let limits = Limits {
        repeated: 3,
        kinds: 64,
        line: 100,
        ordinary: 4_000,
        counter: 2_000,
        marked: 1_000,
        trickle_every: Duration::ZERO,
        trickle: 500,
        notices: 4_000,
        queue: 1 << 20,
    };
    let word = |n: u32| -> String {
        (0..6)
            .map(|place| char::from(b'a' + (n / 26_u32.pow(place) % 26) as u8))
            .collect()
    };
    let floods: [Box<dyn Fn(u32) -> String>; 5] = [
        // Distinct shapes.
        Box::new(move |n| format!("{} {}", word(n), word(n.wrapping_mul(7919)))),
        // Every mark.
        Box::new(|n| format!("{} {n}", String::from_utf8_lossy(MARKS[n as usize % MARKS.len()]))),
        // One mark.
        Box::new(|n| format!("wsd: {n}")),
        // Long lines.
        Box::new(move |n| word(n).repeat(1_000)),
        Box::new(|n| format!("same {n}")),
    ];
    for flood in &floods {
        let (log, _) = filtered(limits, (0..20_000).map(flood));
        assert!(log.len() as u64 <= limits.total(), "{}", log.len());
        assert!(log.lines().all(|line| line.len() <= 400));
    }
    let (log, filter) = filtered(limits, (0..100_000).map(|n| floods[n as usize % 5](n)));
    assert!(log.len() as u64 <= limits.total());
    // What was left out is said.
    assert!(filter.over > 0 && log.contains("lines left out in all"), "{log:.300}");
    assert!(log.contains("\"wsd:\" left out past their budget"));
    // What a player's log takes, all in all.
    assert!(LIMITS.total() < 192 << 20);
}

/// A frame counter that goes on is never taken for one that stopped: past its
/// budget a line still passes now and then, and a flood of another mark never
/// takes its place.
#[test]
fn a_frame_counter_passes_whatever_else_floods() {
    let limits = Limits {
        counter: 1_000,
        marked: 200,
        ..LIMITS
    };
    let mut filter = Filter::new(limits);
    let mut out = Vec::new();
    let start = Instant::now();
    for second in 0..600_u64 {
        let now = start + Duration::from_secs(second);
        for n in 0..100 {
            filter.feed(format!("VTEST_CLIENT_ERROR_CONTEXT_LOST {n}\n").as_bytes(), now, &mut out);
        }
        let line = format!("neptune unixlib wsi (wsd): {} presents\n", second * 60);
        filter.feed(line.as_bytes(), now, &mut out);
    }
    let log = String::from_utf8(out).unwrap();
    let counts: Vec<u64> = log
        .lines()
        .filter_map(|l| l.strip_prefix("neptune unixlib wsi (wsd): ")?.split(' ').next()?.parse().ok())
        .collect();
    assert!(counts.windows(2).all(|pair| pair[0] < pair[1] && pair[1] - pair[0] <= 11 * 60), "{counts:?}");
    assert!(*counts.last().unwrap() >= 589 * 60, "{counts:?}");
}

#[test]
fn lines_are_put_back_together_across_reads_and_a_gap_glues_none() {
    let mut filter = Filter::new(LIMITS);
    let mut out = Vec::new();
    let now = Instant::now();
    for piece in ["neptune uni", "xlib wsi 1\nhalf", " a line\nlost he"] {
        filter.feed(piece.as_bytes(), now, &mut out);
    }
    filter.gap();
    filter.feed(b"re\nnext\n", now, &mut out);
    filter.end(now, &mut out);
    assert_eq!(String::from_utf8(out).unwrap(), "neptune unixlib wsi 1\nhalf a line\nre\nnext\n");
}

/// The program that writes as a game does, given its body, into a pipe.
fn producer(body: &str) -> (Child, Writer) {
    let pipe = Pipe::new().unwrap();
    let ends = pipe.ends();
    let mut command = Command::new("/bin/bash");
    // As the launcher does: the reading end kept, the writing one the guest's.
    let script = format!(
        r#"read -r r w <<<"{ends}"
exec 7<&"$r" 2>&"$w"
eval "exec $r<&- $w>&-"
{body}"#
    );
    command
        .arg("-c")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    let (child, writer) = pipe.inherited(|| command.spawn()).unwrap();
    (child.unwrap(), writer)
}

const MANY: &str = "i=0; while [ $i -lt 20000 ]; do echo \"line $i of a game's\" >&2; i=$((i+1)); done; exit 7";

/// The producer ends by itself, with its own exit status, soon.
fn ends(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(7), "{status:?}");
            return;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("the game is held up by its log");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_game_s_output_reaches_its_log() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wine-1.log");
    let log = crate::diagnostics::open_log(&path).unwrap();
    let (mut child, mut writer) = producer("echo first >&2; echo second >&2; exit 7");
    writer.write_to(log);
    ends(&mut child);
    assert_eq!(writer.finish(), Lost::default());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "first\nsecond\n");
}

/// Finding 6: a supervisor that is gone (its reading end with it) ends no
/// game: no EPIPE, no SIGPIPE.
#[test]
fn a_dead_reader_neither_stops_nor_ends_its_game() {
    // As `Pipe::inherited` leaves it, the reader then gone.
    let pipe = Pipe::new().unwrap();
    let (read, write) = pipe.ends().split_once(' ').map(|(r, w)| (r.to_owned(), w.to_owned())).unwrap();
    let mut command = Command::new("/bin/bash");
    command
        .arg("-c")
        .arg(format!("exec 7<&{read} 2>&{write}; eval 'exec {read}<&- {write}>&-'; sleep 0.3; {MANY}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    let (child, writer) = pipe.inherited(|| command.spawn()).unwrap();
    let mut child = child.unwrap();
    // The reading thread ends and closes its end.
    drop(writer);
    ends(&mut child);
}

/// Finding 6: a supervisor that stands still holds no game up. Its thread is
/// stopped here by what it waits on: nothing reads the pipe at all.
#[test]
fn a_stopped_reader_neither_stops_nor_ends_its_game() {
    let pipe = Pipe::new().unwrap();
    let (read, write) = pipe.ends().split_once(' ').map(|(r, w)| (r.to_owned(), w.to_owned())).unwrap();
    let mut command = Command::new("/bin/bash");
    command
        .arg("-c")
        .arg(format!("exec 7<&{read} 2>&{write}; eval 'exec {read}<&- {write}>&-'; {MANY}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    rustix::io::fcntl_setfd(&pipe.read, rustix::io::FdFlags::empty()).unwrap();
    rustix::io::fcntl_setfd(&pipe.write, rustix::io::FdFlags::empty()).unwrap();
    let mut child = command.spawn().unwrap();
    // Both ends stay open here and nobody reads.
    ends(&mut child);
    drop(pipe);
}

/// A log that stands still until it is released.
struct Stuck(Arc<AtomicBool>, std::sync::mpsc::Sender<Vec<u8>>);
impl Write for Stuck {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        while !self.0.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.1.send(bytes.to_vec());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Finding 6: a disk that stands still costs lines past a bounded queue, which
/// are counted and said; it holds no game up, nor the end of its session.
#[test]
fn a_log_that_stands_still_costs_counted_lines_only() {
    let released = Arc::new(AtomicBool::new(false));
    let (sender, written) = std::sync::mpsc::channel();
    let pipe = Pipe::new().unwrap();
    let write = pipe.write.try_clone().unwrap();
    let mut writer = Writer::start(pipe.read, Limits { queue: 32 << 10, ..LIMITS }).unwrap();
    drop(pipe.write);
    writer.write_to(Stuck(released.clone(), sender));
    // Blocking here: the reader itself always keeps up.
    let flags = rustix::fs::fcntl_getfl(&write).unwrap();
    rustix::fs::fcntl_setfl(&write, flags - rustix::fs::OFlags::NONBLOCK).unwrap();
    let mut game = std::fs::File::from(write);
    let began = Instant::now();
    for n in 0..40_000 {
        writeln!(game, "line {n} of a game's, whose log stands still").unwrap();
    }
    assert!(began.elapsed() < Duration::from_secs(20), "held up for {:?}", began.elapsed());
    // Wait until the reader took all of it.
    std::thread::sleep(Duration::from_millis(500));
    assert!(writer.shared.queue.lock().unwrap().bytes <= 32 << 10, "bounded");
    released.store(true, Ordering::Release);
    drop(game);
    let lost = writer.finish();
    assert!(lost.dropped > 1 << 20, "{lost:?}");
    let log = String::from_utf8(written.iter().flatten().collect()).unwrap();
    assert!(log.contains("dropped here, the log stood still"), "{log:.500}");
    assert!(log.contains(&format!("{} bytes dropped", lost.dropped)), "{log}");
}

#[test]
fn a_log_that_never_moves_again_does_not_hold_up_the_end_of_the_session() {
    let (sender, _written) = std::sync::mpsc::channel();
    let (mut child, mut writer) = producer("echo something >&2; exit 7");
    writer.write_to(Stuck(Arc::new(AtomicBool::new(false)), sender));
    ends(&mut child);
    let began = Instant::now();
    writer.finish();
    assert!(began.elapsed() < DRAIN * 3);
}

/// A process the game left keeps the pipe open: the session ends all the same.
#[test]
fn a_process_that_keeps_the_pipe_does_not_hold_up_the_end_of_the_session() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("log");
    let (mut child, mut writer) = producer("echo something >&2; /bin/sleep 30 & echo $! > /dev/null; exit 7");
    writer.write_to(crate::diagnostics::open_log(&path).unwrap());
    ends(&mut child);
    let began = Instant::now();
    writer.finish();
    assert!(began.elapsed() < DRAIN * 3);
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("something\n"));
}

/// Fails its first writes, as a full disk does.
struct Full(Arc<AtomicU64>, Arc<Mutex<Vec<u8>>>);
impl Write for Full {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.try_update(Ordering::AcqRel, Ordering::Acquire, |left| left.checked_sub(1)).is_ok() {
            return Err(io::Error::from(io::ErrorKind::StorageFull));
        }
        self.1.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Finding 6: a log that cannot be written costs its lines, counted, and says
/// so once it can be; the game goes on.
#[test]
fn a_log_that_cannot_be_written_costs_counted_lines_only() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let (mut child, mut writer) = producer(
        "echo lost >&2; /bin/sleep 0.5; echo kept >&2; exit 7",
    );
    writer.write_to(Full(Arc::new(AtomicU64::new(1)), written.clone()));
    ends(&mut child);
    let lost = writer.finish();
    assert_eq!(lost.unwritten, 5);
    let log = String::from_utf8(written.lock().unwrap().clone()).unwrap();
    assert!(log.contains("[obelisk-log: 5 bytes could not be written here]\nkept\n"), "{log}");
    assert!(log.contains("5 bytes not written"), "{log}");
    // For good.
    let (mut child, mut writer) = producer(MANY);
    writer.write_to(Full(Arc::new(AtomicU64::new(u64::MAX)), written));
    ends(&mut child);
    assert!(writer.finish().unwritten > 0);
    // No log at all.
    let (mut child, writer) = producer(MANY);
    ends(&mut child);
    writer.finish();
}
