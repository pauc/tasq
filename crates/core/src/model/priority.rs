//! Task priority.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::ModelError;

/// Priority of a task: `A` (high), `B` (normal, the default) or `C` (low).
///
/// Stored in the nb file as the tag `#A`, `#B` or `#C`; the format layer does
/// that mapping. Sorts high to low (`A < B < C`), so sorting ascending puts
/// the most urgent tasks first.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Priority {
    /// High.
    A,
    /// Normal; the default.
    #[default]
    B,
    /// Low.
    C,
}

impl Priority {
    /// Every priority, highest first.
    pub const ALL: [Self; 3] = [Self::A, Self::B, Self::C];

    /// The letter, without `#`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
        }
    }

    /// The tag form the file uses: `#A`, `#B`, `#C`.
    pub fn to_hash(self) -> &'static str {
        match self {
            Self::A => "#A",
            Self::B => "#B",
            Self::C => "#C",
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Priority {
    type Err = ModelError;

    /// Accepts `A`, `B`, `C` and the tag forms `#A`, `#B`, `#C`.
    ///
    /// Case matters: `a` is rejected, as it was by the original script
    /// (`^#?[ABC]$`), and because `#a` would be a topic tag in the file.
    /// Surrounding whitespace is not trimmed.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.strip_prefix('#').unwrap_or(s) {
            "A" => Ok(Self::A),
            "B" => Ok(Self::B),
            "C" => Ok(Self::C),
            _ => Err(ModelError::InvalidPriority(s.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_b() {
        assert_eq!(Priority::default(), Priority::B);
    }

    #[test]
    fn parses_bare_and_hashed_letters() {
        assert_eq!("A".parse::<Priority>(), Ok(Priority::A));
        assert_eq!("B".parse::<Priority>(), Ok(Priority::B));
        assert_eq!("C".parse::<Priority>(), Ok(Priority::C));
        assert_eq!("#A".parse::<Priority>(), Ok(Priority::A));
        assert_eq!("#B".parse::<Priority>(), Ok(Priority::B));
        assert_eq!("#C".parse::<Priority>(), Ok(Priority::C));
    }

    #[test]
    fn rejects_lowercase_blank_and_junk_reporting_the_original_text() {
        for bad in ["a", "#a", "", "#", "D", "AB", " A", "A ", "##A"] {
            assert_eq!(
                bad.parse::<Priority>(),
                Err(ModelError::InvalidPriority(bad.to_owned())),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn display_and_hash_forms() {
        assert_eq!(Priority::A.to_string(), "A");
        assert_eq!(Priority::B.to_string(), "B");
        assert_eq!(Priority::C.to_string(), "C");
        assert_eq!(Priority::A.as_str(), "A");
        assert_eq!(Priority::B.as_str(), "B");
        assert_eq!(Priority::C.as_str(), "C");
        assert_eq!(Priority::A.to_hash(), "#A");
        assert_eq!(Priority::B.to_hash(), "#B");
        assert_eq!(Priority::C.to_hash(), "#C");
    }

    #[test]
    fn every_priority_round_trips_and_sorts_high_first() {
        for p in Priority::ALL {
            assert_eq!(p.as_str().parse::<Priority>(), Ok(p));
            assert_eq!(p.to_hash().parse::<Priority>(), Ok(p));
        }
        assert_eq!(Priority::ALL, [Priority::A, Priority::B, Priority::C]);
        assert!(Priority::A < Priority::B && Priority::B < Priority::C);
    }

    #[test]
    fn serde_uses_the_letter() {
        assert_eq!(serde_json::to_string(&Priority::A).unwrap(), "\"A\"");
        assert_eq!(
            serde_json::from_str::<Priority>("\"C\"").unwrap(),
            Priority::C
        );
    }

    #[test]
    fn error_message() {
        assert_eq!(
            ModelError::InvalidPriority("x".into()).to_string(),
            "invalid priority \"x\": expected A, B or C"
        );
    }
}
