//! What language the machine is set to.
//!
//! Asked once, at startup, so an app that has never been configured opens in
//! the language its owner already reads. A preference overrides it; this is
//! only the answer for when there is no preference yet.

/// The user's preferred language as a BCP-47 tag — `pt-BR`, `de`, `fr-CA`.
///
/// `None` when the system will not say, which is not a failure: the caller
/// falls back to the source language, which is what an unrecognised tag would
/// have done anyway.
#[must_use]
pub fn preferred() -> Option<String> {
    imp::preferred()
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;

    pub fn preferred() -> Option<String> {
        // LOCALE_NAME_MAX_LENGTH is 85; the API writes a wide string and
        // returns its length including the terminator.
        let mut buffer = [0u16; 85];
        let written = unsafe { GetUserDefaultLocaleName(&mut buffer) };
        let len = usize::try_from(written).ok()?.checked_sub(1)?;
        (len > 0).then(|| String::from_utf16_lossy(&buffer[..len]))
    }
}

#[cfg(not(windows))]
mod imp {
    /// The environment, in the order POSIX gives them precedence.
    ///
    /// `LC_ALL` overrides everything, `LC_MESSAGES` is the one that governs
    /// interface text specifically, and `LANG` is the fallback. The value looks
    /// like `pt_BR.UTF-8`, so the encoding and the underscore are trimmed off
    /// into the BCP-47 shape the rest of the app uses.
    pub fn preferred() -> Option<String> {
        let raw = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|key| std::env::var(key).ok())
            .filter(|v| !v.is_empty() && v != "C" && v != "POSIX")?;
        let tag = raw.split(['.', '@']).next()?.replace('_', "-");
        (!tag.is_empty()).then_some(tag)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_machine_answers_with_something_shaped_like_a_tag() {
        // Not asserting *which* language — that is the machine's business, and
        // a test that demanded one would fail on somebody else's desk. What is
        // asserted is the shape, because the caller matches on a prefix.
        if let Some(tag) = super::preferred() {
            assert!(!tag.is_empty());
            assert!(!tag.contains('_'), "{tag} should be BCP-47, not POSIX");
            assert!(!tag.contains('.'), "{tag} still carries an encoding");
        }
    }
}
