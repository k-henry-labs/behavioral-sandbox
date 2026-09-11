//! What a sandbox consumed, in units and never in money.
//!
//! - **Units, not prices.** `vcpu_seconds`, `memory_gib_seconds`, `disk_gib_seconds`. No rate, no
//!   currency, no plan and no tier live in this crate; what a unit costs is a question for
//!   somewhere that knows who is paying, and a rate written here is a rate that goes stale on the
//!   day pricing changes.
//! - **Accrued while it runs, never at the end.** Each [`Meter::tick`] appends the interval that
//!   just passed. A sandbox killed by a crash, an OOM or a power cut has already been charged for
//!   the time it held the machine: the ledger is a sum of intervals, so the only thing a sudden
//!   death loses is the part-tick since the last line.
//! - **Allocated, not used.** A sandbox given four vCPUs that sits idle still held four and
//!   stopped them being sold twice. The allocation comes from the record's `limits` line, which is
//!   what makes a count reproducible from the archive alone.
//! - **An append-only ledger.** One JSON object to a line. A reader sums the lines for a `run_id`;
//!   nothing here rewrites or totals, because a file that is only ever appended to survives a
//!   process that stops between two writes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How much of the machine one sandbox was given, as its record's `limits` line spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allocation {
    /// vCPUs the VM was started with.
    pub vcpus: u32,
    /// Guest RAM in MiB.
    pub mem_mib: u32,
    /// Writable disk held, in MiB. Zero for a sandbox with a read-only root and no mount of its
    /// own, which is the default posture.
    pub disk_mib: u32,
}

/// One interval's consumption: what the sandbox held, for how long.
///
/// Deliberately not `Serialize`d from a struct with a `Duration`: the ledger's numbers are what a
/// meter reads, so they are written as plain seconds and left there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Units {
    pub seconds: f64,
    pub vcpu_seconds: f64,
    pub memory_gib_seconds: f64,
    pub disk_gib_seconds: f64,
}

/// A mebibyte as a gibibyte, for the two units measured in GiB.
const MIB_PER_GIB: f64 = 1024.0;

impl Allocation {
    /// What this allocation accrues over `elapsed`.
    #[must_use]
    pub fn over(&self, elapsed: Duration) -> Units {
        let seconds = elapsed.as_secs_f64();
        Units {
            seconds,
            vcpu_seconds: f64::from(self.vcpus) * seconds,
            memory_gib_seconds: f64::from(self.mem_mib) / MIB_PER_GIB * seconds,
            disk_gib_seconds: f64::from(self.disk_mib) / MIB_PER_GIB * seconds,
        }
    }
}

impl Units {
    /// Two intervals of the same sandbox, added. What a reader of the ledger does.
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            seconds: self.seconds + other.seconds,
            vcpu_seconds: self.vcpu_seconds + other.vcpu_seconds,
            memory_gib_seconds: self.memory_gib_seconds + other.memory_gib_seconds,
            disk_gib_seconds: self.disk_gib_seconds + other.disk_gib_seconds,
        }
    }

    /// Nothing consumed.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            seconds: 0.0,
            vcpu_seconds: 0.0,
            memory_gib_seconds: 0.0,
            disk_gib_seconds: 0.0,
        }
    }
}

/// Where the counts go. A file a self-hoster owns, because they have nobody to send them to.
#[derive(Debug, Clone)]
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    /// The ledger at `path`, whose directory is created if it is absent.
    pub fn at(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self { path })
    }

    /// Where it writes, for a startup line that tells an operator.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one interval. Opened and closed per line: a held handle is a buffer that a killed
    /// process never flushes, which is the failure this whole design is built against.
    pub fn append(&self, run_id: &str, at_ms: u64, units: Units) -> std::io::Result<()> {
        let line = serde_json::json!({
            "run_id": run_id,
            "at_ms": at_ms,
            "seconds": units.seconds,
            "vcpu_seconds": units.vcpu_seconds,
            "memory_gib_seconds": units.memory_gib_seconds,
            "disk_gib_seconds": units.disk_gib_seconds,
        });
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")
    }

    /// Everything the ledger holds for one sandbox, summed. What a reader does, and what the
    /// tests assert against.
    pub fn total(&self, run_id: &str) -> std::io::Result<Units> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Units::none()),
            Err(e) => return Err(e),
        };
        let mut total = Units::none();
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if value.get("run_id").and_then(serde_json::Value::as_str) != Some(run_id) {
                continue;
            }
            let number = |key: &str| {
                value
                    .get(key)
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0)
            };
            total = total.plus(Units {
                seconds: number("seconds"),
                vcpu_seconds: number("vcpu_seconds"),
                memory_gib_seconds: number("memory_gib_seconds"),
                disk_gib_seconds: number("disk_gib_seconds"),
            });
        }
        Ok(total)
    }
}

/// How often a running sandbox's consumption is written down.
///
/// The interval is what a sudden death costs: a machine that loses power mid-tick has been
/// charged for everything before the last line and nothing after it. Short enough that the loss
/// is small, long enough that a long run is not thousands of lines.
pub const TICK: Duration = Duration::from_secs(10);

/// One sandbox's running meter: the allocation it holds, and when it was last written down.
pub struct Meter {
    run_id: String,
    allocation: Allocation,
    ledger: Ledger,
    since: Instant,
}

impl Meter {
    /// Starts metering `run_id` as of now.
    #[must_use]
    pub fn started(run_id: impl Into<String>, allocation: Allocation, ledger: Ledger) -> Self {
        Self {
            run_id: run_id.into(),
            allocation,
            ledger,
            since: Instant::now(),
        }
    }

    /// Writes down what has accrued since the last tick, and starts a new interval.
    ///
    /// Called on a timer while the sandbox runs, and once more when it ends, so the ledger covers
    /// the whole of a run whether it finished or was killed.
    pub fn tick(&mut self) -> std::io::Result<Units> {
        let now = Instant::now();
        let units = self.allocation.over(now.duration_since(self.since));
        self.since = now;
        self.ledger
            .append(&self.run_id, tormoni_record::now_ms(), units)?;
        Ok(units)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tormoni_test_support::ScratchDir;

    /// **Allocated, not used.** A sandbox that sat idle held what it was given, and the number
    /// comes from the allocation alone, so it is reproducible from the record's `limits` line
    /// without anybody having watched the run.
    #[test]
    fn units_are_what_was_held_and_not_what_was_touched() {
        let four = Allocation {
            vcpus: 4,
            mem_mib: 2048,
            disk_mib: 10240,
        };
        let units = four.over(Duration::from_secs(30));
        assert_eq!(units.seconds, 30.0);
        assert_eq!(units.vcpu_seconds, 120.0);
        assert_eq!(units.memory_gib_seconds, 60.0, "2 GiB for 30s");
        assert_eq!(units.disk_gib_seconds, 300.0, "10 GiB for 30s");

        // The default posture holds no writable disk, and accrues none rather than a floor.
        let default = Allocation {
            vcpus: 1,
            mem_mib: 512,
            disk_mib: 0,
        };
        assert_eq!(default.over(Duration::from_secs(10)).disk_gib_seconds, 0.0);
    }

    /// **A sandbox killed mid-flight has already been charged for the time it ran.** The ledger is
    /// a sum of intervals written as they pass, so a `Meter` dropped without ever being told the
    /// run ended still leaves a non-zero count.
    ///
    /// This is the whole reason ticks exist: a meter that only wrote on a clean exit would
    /// under-bill every crash, and could be made to under-bill on purpose.
    #[test]
    fn a_run_that_was_killed_still_accrued_what_it_held() {
        let scratch = ScratchDir::created("meter-killed");
        let ledger = Ledger::at(scratch.path().join("usage.jsonl")).expect("a ledger");
        let mut meter = Meter::started(
            "1756860007123-killed",
            Allocation {
                vcpus: 2,
                mem_mib: 1024,
                disk_mib: 0,
            },
            ledger.clone(),
        );

        std::thread::sleep(Duration::from_millis(60));
        meter.tick().expect("a tick lands");
        // Nothing tells it the run ended: the process is gone from here on.
        drop(meter);

        let total = ledger.total("1756860007123-killed").expect("a total");
        assert!(
            total.vcpu_seconds > 0.0,
            "a killed run accrued nothing: {total:?}"
        );
        // Two vCPUs, so the cpu count is twice the wall clock whatever the wall clock was.
        assert!(
            (total.vcpu_seconds - total.seconds * 2.0).abs() < 1e-9,
            "{total:?}"
        );
        assert_eq!(total.memory_gib_seconds, total.seconds, "1 GiB held");
    }

    /// The ledger is append-only and keyed by run: two sandboxes metered into one file do not
    /// mix, and a total is the sum of a run's lines rather than its last one.
    #[test]
    fn a_total_is_the_sum_of_a_runs_intervals_and_nobody_elses() {
        let scratch = ScratchDir::created("meter-ledger");
        let ledger = Ledger::at(scratch.path().join("usage.jsonl")).expect("a ledger");
        let one = Allocation {
            vcpus: 1,
            mem_mib: 1024,
            disk_mib: 0,
        };
        for _ in 0..3 {
            ledger
                .append("run-a", 1, one.over(Duration::from_secs(10)))
                .expect("appended");
        }
        ledger
            .append("run-b", 1, one.over(Duration::from_secs(7)))
            .expect("appended");

        let a = ledger.total("run-a").expect("a total");
        assert_eq!(a.seconds, 30.0, "three intervals, summed");
        assert_eq!(a.vcpu_seconds, 30.0);
        assert_eq!(ledger.total("run-b").expect("a total").seconds, 7.0);
        assert_eq!(
            ledger.total("never-ran").expect("a total"),
            Units::none(),
            "a run with no lines consumed nothing"
        );
    }

    /// **No prices anywhere.** What a unit costs belongs where somebody knows who is paying, and
    /// a rate written into a ledger line is a rate that is wrong the day pricing changes.
    #[test]
    fn a_ledger_line_carries_units_and_never_money() {
        let scratch = ScratchDir::created("meter-no-money");
        let ledger = Ledger::at(scratch.path().join("usage.jsonl")).expect("a ledger");
        ledger
            .append(
                "run-a",
                1_700_000_000_000,
                Allocation {
                    vcpus: 1,
                    mem_mib: 512,
                    disk_mib: 0,
                }
                .over(Duration::from_secs(1)),
            )
            .expect("appended");

        let line = std::fs::read_to_string(ledger.path()).expect("the ledger");
        let value: serde_json::Value = serde_json::from_str(line.trim()).expect("one JSON object");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "at_ms",
                "disk_gib_seconds",
                "memory_gib_seconds",
                "run_id",
                "seconds",
                "vcpu_seconds"
            ],
            "a key that is not a unit or an identifier got in"
        );
        for word in ["price", "cost", "usd", "rate", "tier", "plan", "amount"] {
            assert!(!line.contains(word), "{word:?} is in a ledger line: {line}");
        }
    }
}
