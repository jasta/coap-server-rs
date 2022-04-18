//! Bit of a toy implementation of a "proper" u24 representation.  Isn't learning Rust fun? :)

use std::fmt::{Debug, Display, Formatter};
use std::ops::{Add, AddAssign};

#[derive(Copy, Clone, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub struct u24(u32);

impl u24 {
    pub const MIN: u24 = u24(0);
    pub const MAX: u24 = u24(0xffffff);
    pub const BITS: u32 = 24;

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0
            .checked_add(rhs.0)
            .and_then(|x| u24::try_from(x).ok())
    }

    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.0
            .checked_sub(rhs.0)
            .and_then(|x| u24::try_from(x).ok())
    }

    pub fn wrapping_add(self, rhs: Self) -> Self {
        u24::try_from(self.0.wrapping_add(rhs.0) & 0xffffff).unwrap()
    }

    pub fn wrapping_sub(self, rhs: Self) -> Self {
        u24::try_from(self.0.wrapping_sub(rhs.0) & 0xffffff).unwrap()
    }

    pub fn from_le_bytes(bytes: [u8; 3]) -> Self {
        u24::try_from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])).unwrap()
    }
}

impl Display for u24 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Debug for u24 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl Add for u24 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        u24::try_from(self.0 + rhs.0).expect("attempt to add with overflow")
    }
}

impl AddAssign for u24 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl From<u8> for u24 {
    fn from(value: u8) -> Self {
        u24(u32::from(value))
    }
}

impl From<u16> for u24 {
    fn from(value: u16) -> Self {
        u24(u32::from(value))
    }
}

impl TryFrom<u32> for u24 {
    type Error = TryFromCustomIntError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value > u24::MAX.0 {
            Err(TryFromCustomIntError)
        } else {
            Ok(u24(value))
        }
    }
}

impl TryFrom<u64> for u24 {
    type Error = TryFromCustomIntError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        u24::try_from(u32::try_from(value).map_err(|_| TryFromCustomIntError {})?)
    }
}

impl TryFrom<u24> for u8 {
    type Error = TryFromCustomIntError;

    fn try_from(value: u24) -> Result<Self, Self::Error> {
        u8::try_from(value.0).map_err(|_| TryFromCustomIntError {})
    }
}

impl TryFrom<u24> for u16 {
    type Error = TryFromCustomIntError;

    fn try_from(value: u24) -> Result<Self, Self::Error> {
        u16::try_from(value.0).map_err(|_| TryFromCustomIntError {})
    }
}

impl From<u24> for u32 {
    fn from(value: u24) -> Self {
        value.0
    }
}

impl From<u24> for u64 {
    fn from(value: u24) -> Self {
        u64::from(value.0)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TryFromCustomIntError;

impl Display for TryFromCustomIntError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("out of range integral type conversion attempted")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_overflow_panics() {
        let x = u24::MAX;
        let _y = x + u24::from(1u8);
    }

    #[test]
    fn test_wrapping_add() {
        let x = u24::MAX;
        let y = x.wrapping_add(u24::from(1u8));
        assert_eq!(y, u24::MIN);
    }
}
