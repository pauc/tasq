//! Ctrl-C while a plugin runs.
//!
//! A plugin leads its own process group, so that its timeout can kill all
//! of it, and a Ctrl-C at the terminal does not reach that group: only
//! tasq's own gets SIGINT. While a plugin runs, a [`Watch`] turns that
//! SIGINT into a flag the runner checks, so it can pass it on to the
//! plugin; at any other time SIGINT kills tasq as it always did.
//!
//! The handler is installed on the first event and stays. `signal_hook`
//! cannot give SIGINT its default disposition back (unregistering the last
//! action leaves the signal ignored), so its first action does the default
//! itself whenever no plugin runs.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use signal_hook::consts::SIGINT;

/// What the SIGINT handler reads and writes.
struct Flags {
    /// No plugin runs: SIGINT does its default.
    idle: Arc<AtomicBool>,
    /// SIGINT arrived while a plugin ran.
    hit: Arc<AtomicBool>,
}

/// The flags, with the handler installed on first use; `None` when it
/// could not be, and SIGINT keeps its default.
fn flags() -> Option<&'static Flags> {
    static FLAGS: OnceLock<Option<Flags>> = OnceLock::new();
    FLAGS
        .get_or_init(|| {
            let flags = Flags {
                idle: Arc::new(AtomicBool::new(true)),
                hit: Arc::new(AtomicBool::new(false)),
            };
            // The default runs first, so an idle tasq never sets `hit`.
            signal_hook::flag::register_conditional_default(SIGINT, Arc::clone(&flags.idle))
                .ok()?;
            signal_hook::flag::register(SIGINT, Arc::clone(&flags.hit)).ok()?;
            Some(flags)
        })
        .as_ref()
}

/// SIGINT caught from [`Watch::start`] until [`Watch::end`], which every
/// watch must reach. One at a time: events run one plugin after the other.
pub struct Watch {
    flags: Option<&'static Flags>,
    /// A Ctrl-C was passed on to the plugin ([`Watch::pass_on`]).
    passed_on: Cell<bool>,
}

impl Watch {
    /// Catches SIGINT from now on. Started before the plugin is spawned, so
    /// a Ctrl-C never leaves it running without tasq.
    #[must_use = "SIGINT stays caught until `end`"]
    pub fn start() -> Self {
        let flags = flags();
        if let Some(flags) = flags {
            flags.hit.store(false, Ordering::SeqCst);
            flags.idle.store(false, Ordering::SeqCst);
        }
        Self {
            flags,
            passed_on: Cell::new(false),
        }
    }

    /// Whether SIGINT arrived since [`Self::start`], or since the last
    /// [`Self::pass_on`].
    pub fn hit(&self) -> bool {
        self.flags
            .is_some_and(|flags| flags.hit.load(Ordering::SeqCst))
    }

    /// Notes that the Ctrl-C [`Self::hit`] reported was passed on to the
    /// plugin, and watches for the next one: a second Ctrl-C is the user
    /// insisting.
    pub fn pass_on(&self) {
        if let Some(flags) = self.flags {
            self.passed_on.set(true);
            flags.hit.store(false, Ordering::SeqCst);
        }
    }

    /// Gives SIGINT its default back and says whether one arrived
    /// meanwhile: one that arrives after this kills tasq, so none is lost.
    pub fn end(self) -> bool {
        if let Some(flags) = self.flags {
            flags.idle.store(true, Ordering::SeqCst);
        }
        self.passed_on.get() || self.hit()
    }
}
