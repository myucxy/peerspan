use std::fmt;
use thiserror::Error;

#[derive(Clone, PartialEq, Eq)]
pub struct PairingCode(String);

impl PairingCode {
    pub fn parse(value: impl Into<String>) -> Result<Self, PairingCodeError> {
        let value = value.into();
        if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(PairingCodeError::InvalidFormat);
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PairingCode(**redacted**)")
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingCodeError {
    #[error("pairing code must contain exactly six digits")]
    InvalidFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exactly_six_digits() {
        assert_eq!(PairingCode::parse("382104").unwrap().expose(), "382104");
        assert_eq!(
            PairingCode::parse("38210").unwrap_err(),
            PairingCodeError::InvalidFormat
        );
        assert_eq!(
            PairingCode::parse("38A104").unwrap_err(),
            PairingCodeError::InvalidFormat
        );
    }

    #[test]
    fn debug_output_never_leaks_pairing_code() {
        let code = PairingCode::parse("382104").unwrap();
        assert!(!format!("{code:?}").contains("382104"));
    }
}
