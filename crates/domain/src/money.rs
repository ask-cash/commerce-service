//! Money as integer minor units plus an ISO 4217 currency.
//!
//! There is deliberately no conversion from floating point. Arithmetic is
//! checked: mixing currencies or overflowing returns an error instead of
//! silently producing a wrong amount.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A three-letter ISO 4217 currency code, stored lowercase (Stripe's format).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Currency([u8; 3]);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid currency code {0:?}: expected three ASCII letters")]
pub struct CurrencyError(String);

impl Currency {
    pub const USD: Currency = Currency(*b"usd");
    pub const EUR: Currency = Currency(*b"eur");
    pub const GBP: Currency = Currency(*b"gbp");
    pub const INR: Currency = Currency(*b"inr");

    pub fn as_str(&self) -> &str {
        // Constructed only from ASCII letters, so always valid UTF-8.
        std::str::from_utf8(&self.0).unwrap_or("???")
    }
}

impl FromStr for Currency {
    type Err = CurrencyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != 3 || !bytes.iter().all(u8::is_ascii_alphabetic) {
            return Err(CurrencyError(s.to_owned()));
        }
        let lower = s.to_ascii_lowercase();
        let b = lower.as_bytes();
        Ok(Currency([b[0], b[1], b[2]]))
    }
}

impl TryFrom<String> for Currency {
    type Error = CurrencyError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Currency> for String {
    fn from(c: Currency) -> Self {
        c.as_str().to_owned()
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An amount in the currency's smallest unit: 1250 USD means $12.50.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    minor: i64,
    currency: Currency,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    #[error("currency mismatch: {left} vs {right}")]
    CurrencyMismatch { left: Currency, right: Currency },
    #[error("amount overflow")]
    Overflow,
}

impl Money {
    pub const fn new(minor: i64, currency: Currency) -> Self {
        Self { minor, currency }
    }

    pub const fn zero(currency: Currency) -> Self {
        Self { minor: 0, currency }
    }

    pub const fn minor(&self) -> i64 {
        self.minor
    }

    pub const fn currency(&self) -> Currency {
        self.currency
    }

    pub const fn is_negative(&self) -> bool {
        self.minor < 0
    }

    pub fn checked_add(self, other: Money) -> Result<Money, MoneyError> {
        self.same_currency(other)?;
        let minor = self.minor.checked_add(other.minor).ok_or(MoneyError::Overflow)?;
        Ok(Money::new(minor, self.currency))
    }

    pub fn checked_sub(self, other: Money) -> Result<Money, MoneyError> {
        self.same_currency(other)?;
        let minor = self.minor.checked_sub(other.minor).ok_or(MoneyError::Overflow)?;
        Ok(Money::new(minor, self.currency))
    }

    pub fn checked_neg(self) -> Result<Money, MoneyError> {
        let minor = self.minor.checked_neg().ok_or(MoneyError::Overflow)?;
        Ok(Money::new(minor, self.currency))
    }

    fn same_currency(&self, other: Money) -> Result<(), MoneyError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            })
        }
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.minor, self.currency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_parses_and_normalises_to_lowercase() {
        assert_eq!("USD".parse::<Currency>(), Ok(Currency::USD));
        assert_eq!(Currency::USD.as_str(), "usd");
        assert!("US".parse::<Currency>().is_err());
        assert!("u5d".parse::<Currency>().is_err());
        assert!("usdx".parse::<Currency>().is_err());
    }

    #[test]
    fn adds_same_currency() {
        let a = Money::new(1250, Currency::USD);
        let b = Money::new(50, Currency::USD);
        assert_eq!(a.checked_add(b), Ok(Money::new(1300, Currency::USD)));
        assert_eq!(a.checked_sub(b), Ok(Money::new(1200, Currency::USD)));
    }

    #[test]
    fn rejects_currency_mismatch() {
        let a = Money::new(100, Currency::USD);
        let b = Money::new(100, Currency::EUR);
        assert!(matches!(a.checked_add(b), Err(MoneyError::CurrencyMismatch { .. })));
    }

    #[test]
    fn rejects_overflow() {
        let a = Money::new(i64::MAX, Currency::USD);
        assert_eq!(a.checked_add(Money::new(1, Currency::USD)), Err(MoneyError::Overflow));
        assert_eq!(
            Money::new(i64::MIN, Currency::USD).checked_neg(),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn serde_round_trip() -> Result<(), serde_json::Error> {
        let m = Money::new(999, Currency::GBP);
        let json = serde_json::to_string(&m)?;
        assert_eq!(json, r#"{"minor":999,"currency":"gbp"}"#);
        assert_eq!(serde_json::from_str::<Money>(&json)?, m);
        assert!(serde_json::from_str::<Money>(r#"{"minor":1,"currency":"12"}"#).is_err());
        Ok(())
    }
}
