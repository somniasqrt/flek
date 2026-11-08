//! A string that never shows up in logs.

use std::fmt;

use zeroize::Zeroize;

/// Holds a token or cookie. `Debug` and `Display` print only the prefix
/// (`xoxc-…***`); the value is reachable solely through [`Secret::expose`].
/// Deliberately not `Serialize`.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Raw value. Only for building requests or writing to the keychain.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn redacted(&self) -> String {
        match self.0.find('-') {
            Some(i) if i <= 5 => format!("{}…***", &self.0[..=i]),
            _ => "***".to_owned(),
        }
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_prefixes() {
        assert_eq!(
            format!("{:?}", Secret::new("xoxc-123-456-abc")),
            "xoxc-…***"
        );
        assert_eq!(format!("{}", Secret::new("xoxd-abc%2Fdef")), "xoxd-…***");
    }

    #[test]
    fn redacts_values_without_prefix() {
        assert_eq!(format!("{:?}", Secret::new("hunter2")), "***");
        assert_eq!(format!("{:?}", Secret::new("averylongword-x")), "***");
        assert_eq!(format!("{:?}", Secret::new("")), "***");
    }
}
