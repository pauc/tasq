//! The name of a task file: a ULID (<https://github.com/ulid/spec>), 26
//! Crockford base32 characters whose first 48 bits are the creation time in
//! milliseconds since the Unix epoch and whose last 80 bits are random.
//!
//! The store reads the creation time of a task from its file name and
//! nothing else (ADR-0026). A name that is not a ULID is still a task file
//! when its content is; its creation time is then unknown.

use std::fmt;

use chrono::{DateTime, Utc};

/// Crockford base32: digits, then the letters without `I`, `L`, `O` and `U`.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The length of a ULID in characters.
pub const LEN: usize = 26;

/// A parsed ULID: a millisecond timestamp and 80 random bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ulid {
    /// Milliseconds since the Unix epoch, 48 bits.
    millis: u64,
    /// The random part, 80 bits.
    random: u128,
}

impl Ulid {
    /// A ULID from its parts; `millis` is truncated to 48 bits and
    /// `random` to 80.
    pub fn from_parts(millis: u64, random: u128) -> Self {
        Self {
            millis: millis & ((1 << 48) - 1),
            random: random & ((1 << 80) - 1),
        }
    }

    /// Reads a ULID written the way the store writes them: exactly 26
    /// uppercase Crockford base32 characters, the first at most `7` so the
    /// timestamp fits in 48 bits. Anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != LEN || bytes[0] > b'7' {
            return None;
        }
        let mut value: u128 = 0;
        for &b in bytes {
            let digit = ALPHABET.iter().position(|&c| c == b)?;
            value = (value << 5) + digit as u128;
        }
        Some(Self {
            millis: u64::try_from(value >> 80).expect("48 bits fit in a u64"),
            random: value & ((1 << 80) - 1),
        })
    }

    /// Milliseconds since the Unix epoch.
    pub fn timestamp_millis(&self) -> u64 {
        self.millis
    }

    /// The timestamp as a UTC date and time, to the millisecond.
    pub fn timestamp(&self) -> DateTime<Utc> {
        let millis = i64::try_from(self.millis).expect("48 bits fit in an i64");
        DateTime::from_timestamp_millis(millis).expect("48 bits of milliseconds are in range")
    }
}

impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = (u128::from(self.millis) << 80) + self.random;
        let mut out = [0u8; LEN];
        for (i, slot) in out.iter_mut().enumerate() {
            let shift = 5 * (LEN - 1 - i);
            *slot = ALPHABET[((value >> shift) & 0x1f) as usize];
        }
        f.write_str(std::str::from_utf8(&out).expect("the alphabet is ASCII"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example of the ULID spec, decoded by hand: the first ten
    /// characters are 1469922850259 ms, 2016-07-30 23:54:10.259 UTC.
    const SPEC_EXAMPLE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const SPEC_MILLIS: u64 = 1_469_922_850_259;

    #[test]
    fn parses_the_spec_example() {
        let ulid = Ulid::parse(SPEC_EXAMPLE).unwrap();
        assert_eq!(ulid.timestamp_millis(), SPEC_MILLIS);
        assert_eq!(ulid.timestamp().to_string(), "2016-07-30 23:54:10.259 UTC");
        assert_eq!(ulid.to_string(), SPEC_EXAMPLE);
    }

    #[test]
    fn refuses_what_the_store_never_writes() {
        assert_eq!(Ulid::parse(""), None);
        assert_eq!(Ulid::parse("01ARZ3NDEKTSV4RRFFQ69G5FA"), None, "25 chars");
        assert_eq!(Ulid::parse("01ARZ3NDEKTSV4RRFFQ69G5FAVX"), None, "27 chars");
        assert_eq!(Ulid::parse("01arz3ndektsv4rrffq69g5fav"), None, "lowercase");
        assert_eq!(
            Ulid::parse("01ARZ3NDEKTSV4RRFFQ69G5FAI"),
            None,
            "I is not in the alphabet"
        );
        assert_eq!(
            Ulid::parse("01ARZ3NDEKTSV4RRFFQ69G5FAU"),
            None,
            "U is not in the alphabet"
        );
        assert_eq!(Ulid::parse("01ARZ3NDEKTSV4RRFFQ69G5FA-"), None);
        assert_eq!(
            Ulid::parse("80000000000000000000000000"),
            None,
            "timestamp overflows 48 bits"
        );
        assert_eq!(
            Ulid::parse("7ZZZZZZZZZZZZZZZZZZZZZZZZZ").map(|u| u.timestamp_millis()),
            Some((1 << 48) - 1)
        );
        assert_eq!(
            Ulid::parse("01ARZ3NDÉKTSV4RRFFQ69G5FA"),
            None,
            "26 bytes but not ASCII"
        );
    }

    #[test]
    fn parts_round_trip_and_are_masked() {
        let ulid = Ulid::from_parts(SPEC_MILLIS, 0xD676_4C61_EFB9_9302_BD5B);
        assert_eq!(ulid.to_string(), SPEC_EXAMPLE);
        assert_eq!(Ulid::parse(&ulid.to_string()), Some(ulid));
        assert_eq!(
            Ulid::from_parts(0, 0).to_string(),
            "00000000000000000000000000"
        );
        assert_eq!(
            Ulid::from_parts(u64::MAX, u128::MAX),
            Ulid::from_parts((1 << 48) - 1, (1 << 80) - 1),
            "high bits are dropped"
        );
        assert_eq!(
            Ulid::from_parts(u64::MAX, u128::MAX).to_string(),
            "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"
        );
        assert!(
            Ulid::from_parts(1, 0) < Ulid::from_parts(2, 0),
            "older sorts first"
        );
    }
}
