use crate::CostError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

const SCALE: u64 = 1_000_000_000;

/// Exact nonnegative currency units, at nanounit resolution. Currency is held by the report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount(u64);
impl Amount {
    pub const ZERO: Self = Self(0);
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }
    pub const fn nanos(self) -> u64 {
        self.0
    }

    /// # Errors
    /// Refuses signs, exponent notation, leading zeros, whitespace, more than nine
    /// fractional digits and values that overflow u64 nanounits. No float is used.
    pub fn parse(value: &str) -> Result<Self, CostError> {
        if value.len() > 30 {
            return Err(CostError::InvalidAmount);
        }
        let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
        if whole.is_empty()
            || (whole.len() > 1 && whole.starts_with('0'))
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || fraction.len() > 9
            || !fraction.bytes().all(|b| b.is_ascii_digit())
            || (value.contains('.') && fraction.is_empty())
        {
            return Err(CostError::InvalidAmount);
        }
        let whole: u64 = whole.parse().map_err(|_| CostError::InvalidAmount)?;
        let fraction: u64 = if fraction.is_empty() {
            0
        } else {
            fraction
                .parse::<u64>()
                .map_err(|_| CostError::InvalidAmount)?
                * 10_u64
                    .pow(9 - u32::try_from(fraction.len()).map_err(|_| CostError::InvalidAmount)?)
        };
        whole
            .checked_mul(SCALE)
            .and_then(|n| n.checked_add(fraction))
            .map(Self)
            .ok_or(CostError::InvalidAmount)
    }
    pub(crate) fn add(self, other: Self) -> Result<Self, CostError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(CostError::Overflow)
    }
}
impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = format!("{}.{:09}", self.0 / SCALE, self.0 % SCALE);
        f.write_str(value.trim_end_matches('0').trim_end_matches('.'))
    }
}
impl Serialize for Amount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Explicit three-letter code. No claim of currency-registry validation or exchange rates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Currency(String);
impl Currency {
    /// # Errors
    /// Refuses anything other than three uppercase ASCII letters.
    pub fn new(value: impl Into<String>) -> Result<Self, CostError> {
        let value = value.into();
        if value.len() != 3 || !value.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(CostError::InvalidCurrency);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rate {
    pub amount: Amount,
    pub per_units: u64,
}
impl Rate {
    /// # Errors
    /// Refuses zero denominators and results outside u64 nanounits. Each line is
    /// rounded upward once, then record/bucket totals sum the rounded line values.
    pub fn price(&self, quantity: u64) -> Result<Amount, CostError> {
        if self.per_units == 0 {
            return Err(CostError::InvalidDenominator);
        }
        let numerator = u128::from(self.amount.nanos()) * u128::from(quantity);
        let nanos = numerator.div_ceil(u128::from(self.per_units));
        u64::try_from(nanos)
            .map(Amount)
            .map_err(|_| CostError::Overflow)
    }
}
