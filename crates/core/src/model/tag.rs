//! Topic tags.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::ModelError;

/// A topic tag such as `gitlab` or `support`, stored without the `#`.
///
/// Status and priority are not tags in the model, even though the file spells
/// all three as `#word`; see the [module docs](super).
///
/// A tag is any non-empty text without whitespace that does not start with
/// `#`. Case is preserved (nb tags are case-sensitive).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Tag(String);

impl Tag {
    /// Validates `name`: rejects the empty string, a leading `#` and any
    /// whitespace. Use [`Tag::from_str`] to accept the `#tag` spelling.
    pub fn new(name: impl Into<String>) -> Result<Self, ModelError> {
        let name = name.into();
        let reason = if name.is_empty() {
            "must not be empty"
        } else if name.starts_with('#') {
            "must not start with '#'"
        } else if name.chars().any(char::is_whitespace) {
            "must not contain whitespace"
        } else {
            return Ok(Self(name));
        };
        Err(ModelError::InvalidTag { tag: name, reason })
    }

    /// The tag name, without `#`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The tag form the file uses: `#gitlab`.
    pub fn to_hash(&self) -> String {
        format!("#{}", self.0)
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Tag {
    type Err = ModelError;

    /// Lenient user-facing parse: strips one leading `#` (so `#gitlab` and
    /// `gitlab` both give the tag `gitlab`), then validates like [`Tag::new`].
    /// `##x` is therefore still rejected.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.strip_prefix('#').unwrap_or(s))
    }
}

impl TryFrom<String> for Tag {
    type Error = ModelError;

    /// Strict (like [`Tag::new`]); used by `Deserialize`.
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<Tag> for String {
    fn from(t: Tag) -> Self {
        t.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejected(tag: &str, reason: &'static str) -> Result<Tag, ModelError> {
        Err(ModelError::InvalidTag {
            tag: tag.to_owned(),
            reason,
        })
    }

    #[test]
    fn new_accepts_plain_words_preserving_case() {
        for ok in ["gitlab", "Support", "a", "3l-core", "x_y", "ü"] {
            let t = Tag::new(ok).unwrap();
            assert_eq!(t.as_str(), ok);
            assert_eq!(t.to_string(), ok);
            assert_eq!(t.to_hash(), format!("#{ok}"));
            assert_eq!(String::from(t), ok);
        }
    }

    #[test]
    fn new_rejects_empty_hash_prefix_and_whitespace() {
        assert_eq!(Tag::new(""), rejected("", "must not be empty"));
        assert_eq!(
            Tag::new("#gitlab"),
            rejected("#gitlab", "must not start with '#'")
        );
        assert_eq!(Tag::new("#"), rejected("#", "must not start with '#'"));
        assert_eq!(
            Tag::new("git lab"),
            rejected("git lab", "must not contain whitespace")
        );
        assert_eq!(
            Tag::new(" gitlab"),
            rejected(" gitlab", "must not contain whitespace")
        );
        assert_eq!(
            Tag::new("gitlab\t"),
            rejected("gitlab\t", "must not contain whitespace")
        );
        assert_eq!(
            Tag::new("a\nb"),
            rejected("a\nb", "must not contain whitespace")
        );
    }

    #[test]
    fn from_str_strips_one_hash_then_validates() {
        assert_eq!("#gitlab".parse::<Tag>(), Tag::new("gitlab"));
        assert_eq!("gitlab".parse::<Tag>(), Tag::new("gitlab"));
        assert_eq!(
            "##x".parse::<Tag>(),
            rejected("#x", "must not start with '#'")
        );
        assert_eq!("#".parse::<Tag>(), rejected("", "must not be empty"));
        assert_eq!(
            "# x".parse::<Tag>(),
            rejected(" x", "must not contain whitespace")
        );
    }

    #[test]
    fn serde_is_a_plain_validated_string() {
        let t = Tag::new("gitlab").unwrap();
        assert_eq!(serde_json::to_string(&t).unwrap(), "\"gitlab\"");
        assert_eq!(serde_json::from_str::<Tag>("\"gitlab\"").unwrap(), t);
        assert!(serde_json::from_str::<Tag>("\"#gitlab\"").is_err());
        assert_eq!(
            Tag::try_from("#x".to_owned()),
            rejected("#x", "must not start with '#'")
        );
    }

    #[test]
    fn error_message() {
        assert_eq!(
            ModelError::InvalidTag {
                tag: "#x".into(),
                reason: "must not start with '#'"
            }
            .to_string(),
            "invalid tag \"#x\": must not start with '#'"
        );
    }
}
