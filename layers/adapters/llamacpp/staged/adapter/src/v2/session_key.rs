//! The durable conversation identity an OUTER attaches to an inference.
//!
//! `session_id` names a pipeline session and `request_id` names one turn;
//! neither survives a process restart as "the same conversation", which is
//! what a persisted KV record has to be keyed by. `session_key` is that
//! third identity, and the grammar is enforced rather than advisory: two
//! different conversations that pick the same raw string become the same
//! record, and byte comparison cannot detect that collision afterwards.
//!
//! The contract is owned by docs/kv-state-store-convention.md; this module is
//! its executable half.

/// Why a candidate key was refused. Each variant is a negative test in the
/// convention's `sk-v1` contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionKeyError {
    /// Missing the `sk1:` version prefix.
    MissingPrefix,
    /// Empty, or longer than 512 bytes counting the prefix.
    Length,
    /// A C0 or C1 control character.
    ControlCharacter,
    /// No `/` after the prefix, so there is no owner namespace.
    MissingSeparator,
    /// Owner or conversation is empty or entirely whitespace.
    EmptyComponent,
}

impl SessionKeyError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingPrefix => "session key must start with sk1:",
            Self::Length => "session key must be 1..=512 bytes including the prefix",
            Self::ControlCharacter => "session key must not contain control characters",
            Self::MissingSeparator => "session key must be sk1:<owner>/<conversation>",
            Self::EmptyComponent => "session key owner and conversation must be non-blank",
        }
    }
}

pub const PREFIX: &str = "sk1:";
pub const MAX_BYTES: usize = 512;

/// A validated `sk1:<owner>/<conversation>` key.
///
/// Holds the exact bytes it was given. Unicode normalisation is deliberately
/// not applied: comparison and hashing are byte equality, so two keys that
/// differ only in NFC/NFD are different conversations. Normalising here would
/// silently merge them, and an OUTER that wants them merged can normalise
/// before minting the key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionKey {
    raw: String,
    separator: usize,
}

impl SessionKey {
    pub fn parse(raw: &str) -> Result<Self, SessionKeyError> {
        if raw.len() > MAX_BYTES || raw.is_empty() {
            return Err(SessionKeyError::Length);
        }
        let Some(body) = raw.strip_prefix(PREFIX) else {
            return Err(SessionKeyError::MissingPrefix);
        };
        if raw.chars().any(|c| c.is_control()) {
            return Err(SessionKeyError::ControlCharacter);
        }
        // The separator is the first `/` after the prefix; later slashes are
        // data, so a conversation id may itself be a path.
        let Some(offset) = body.find('/') else {
            return Err(SessionKeyError::MissingSeparator);
        };
        let owner = &body[..offset];
        let conversation = &body[offset + 1..];
        if owner.trim().is_empty() || conversation.trim().is_empty() {
            return Err(SessionKeyError::EmptyComponent);
        }
        Ok(Self {
            raw: raw.to_owned(),
            separator: PREFIX.len() + offset,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn owner(&self) -> &str {
        &self.raw[PREFIX.len()..self.separator]
    }

    pub fn conversation(&self) -> &str {
        &self.raw[self.separator + 1..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_well_formed_key() {
        let key = SessionKey::parse("sk1:tenant-a/conv-7f3c").expect("valid");
        assert_eq!(key.owner(), "tenant-a");
        assert_eq!(key.conversation(), "conv-7f3c");
        assert_eq!(key.as_str(), "sk1:tenant-a/conv-7f3c");
    }

    #[test]
    fn splits_only_at_the_first_separator() {
        let key = SessionKey::parse("sk1:tenant-a/team/conv/1").expect("valid");
        assert_eq!(key.owner(), "tenant-a");
        assert_eq!(key.conversation(), "team/conv/1");
    }

    #[test]
    fn rejects_a_missing_prefix() {
        assert_eq!(SessionKey::parse("tenant-a/conv"), Err(SessionKeyError::MissingPrefix));
    }

    #[test]
    fn rejects_a_missing_separator() {
        assert_eq!(SessionKey::parse("sk1:tenant-a"), Err(SessionKeyError::MissingSeparator));
    }

    #[test]
    fn rejects_blank_components() {
        assert_eq!(SessionKey::parse("sk1:/conv"), Err(SessionKeyError::EmptyComponent));
        assert_eq!(SessionKey::parse("sk1:tenant/"), Err(SessionKeyError::EmptyComponent));
        assert_eq!(SessionKey::parse("sk1:   /conv"), Err(SessionKeyError::EmptyComponent));
        assert_eq!(SessionKey::parse("sk1:tenant/   "), Err(SessionKeyError::EmptyComponent));
    }

    #[test]
    fn rejects_control_characters() {
        assert_eq!(
            SessionKey::parse("sk1:tenant\u{7}a/conv"),
            Err(SessionKeyError::ControlCharacter)
        );
    }

    #[test]
    fn length_is_measured_in_bytes_including_the_prefix() {
        let fits = format!("sk1:owner/{}", "c".repeat(MAX_BYTES - PREFIX.len() - "owner/".len()));
        assert_eq!(fits.len(), MAX_BYTES);
        assert!(SessionKey::parse(&fits).is_ok());
        assert_eq!(SessionKey::parse(&format!("{fits}c")), Err(SessionKeyError::Length));
        assert_eq!(SessionKey::parse(""), Err(SessionKeyError::Length));
    }

    #[test]
    fn compares_by_bytes_so_nfc_and_nfd_are_different_conversations() {
        // U+AC00 versus U+1100 U+1161 - the same rendered syllable.
        let composed = SessionKey::parse("sk1:owner/\u{AC00}").expect("valid");
        let decomposed = SessionKey::parse("sk1:owner/\u{1100}\u{1161}").expect("valid");
        assert_ne!(composed, decomposed);
    }

    #[test]
    fn the_same_conversation_under_a_different_owner_is_a_different_key() {
        let first = SessionKey::parse("sk1:tenant-a/conv").expect("valid");
        let second = SessionKey::parse("sk1:tenant-b/conv").expect("valid");
        assert_ne!(first, second);
        assert_eq!(first.conversation(), second.conversation());
    }
}
