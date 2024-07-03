//! Enums for HPM SPI
//!

/// Time between CS active and SCLK edge.
#[derive(Copy, Clone)]
pub enum Cs2Sclk {
    _1HalfSclk,
    _2HalfSclk,
    _3HalfSclk,
    _4HalfSclk,
}

impl Into<u8> for Cs2Sclk {
    fn into(self) -> u8 {
        match self {
            Cs2Sclk::_1HalfSclk => 0x00,
            Cs2Sclk::_2HalfSclk => 0x01,
            Cs2Sclk::_3HalfSclk => 0x02,
            Cs2Sclk::_4HalfSclk => 0x03,
        }
    }
}

/// Time the Chip Select line stays high.
#[derive(Copy, Clone)]
pub enum CsHighTime {
    _1HalfSclk,
    _2HalfSclk,
    _3HalfSclk,
    _4HalfSclk,
    _5HalfSclk,
    _6HalfSclk,
    _7HalfSclk,
    _8HalfSclk,
    _9HalfSclk,
    _10HalfSclk,
    _11HalfSclk,
    _12HalfSclk,
    _13HalfSclk,
    _14HalfSclk,
    _15HalfSclk,
    _16HalfSclk,
}

impl Into<u8> for CsHighTime {
    fn into(self) -> u8 {
        match self {
            CsHighTime::_1HalfSclk => 0b0000,
            CsHighTime::_2HalfSclk => 0b0001,
            CsHighTime::_3HalfSclk => 0b0010,
            CsHighTime::_4HalfSclk => 0b0011,
            CsHighTime::_5HalfSclk => 0b0100,
            CsHighTime::_6HalfSclk => 0b0101,
            CsHighTime::_7HalfSclk => 0b0110,
            CsHighTime::_8HalfSclk => 0b0111,
            CsHighTime::_9HalfSclk => 0b1000,
            CsHighTime::_10HalfSclk => 0b1001,
            CsHighTime::_11HalfSclk => 0b1010,
            CsHighTime::_12HalfSclk => 0b1011,
            CsHighTime::_13HalfSclk => 0b1100,
            CsHighTime::_14HalfSclk => 0b1101,
            CsHighTime::_15HalfSclk => 0b1110,
            CsHighTime::_16HalfSclk => 0b1111,
        }
    }
}

/// SPI data length
#[derive(Copy, Clone)]
pub enum DataLen {
    _1Bit,
    _2Bit,
    _3Bit,
    _4Bit,
    _5Bit,
    _6Bit,
    _7Bit,
    _8Bit,
    _9Bit,
    _10Bit,
    _11Bit,
    _12Bit,
    _13Bit,
    _14Bit,
    _15Bit,
    _16Bit,
    _17Bit,
    _18Bit,
    _19Bit,
    _20Bit,
    _21Bit,
    _22Bit,
    _23Bit,
    _24Bit,
    _25Bit,
    _26Bit,
    _27Bit,
    _28Bit,
    _29Bit,
    _30Bit,
    _31Bit,
    _32Bit,
}

impl Into<u8> for DataLen {
    fn into(self) -> u8 {
        match self {
            DataLen::_1Bit => 0x00,
            DataLen::_2Bit => 0x01,
            DataLen::_3Bit => 0x02,
            DataLen::_4Bit => 0x03,
            DataLen::_5Bit => 0x04,
            DataLen::_6Bit => 0x05,
            DataLen::_7Bit => 0x06,
            DataLen::_8Bit => 0x07,
            DataLen::_9Bit => 0x08,
            DataLen::_10Bit => 0x09,
            DataLen::_11Bit => 0x0a,
            DataLen::_12Bit => 0x0b,
            DataLen::_13Bit => 0x0c,
            DataLen::_14Bit => 0x0d,
            DataLen::_15Bit => 0x0e,
            DataLen::_16Bit => 0x0f,
            DataLen::_17Bit => 0x10,
            DataLen::_18Bit => 0x11,
            DataLen::_19Bit => 0x12,
            DataLen::_20Bit => 0x13,
            DataLen::_21Bit => 0x14,
            DataLen::_22Bit => 0x15,
            DataLen::_23Bit => 0x16,
            DataLen::_24Bit => 0x17,
            DataLen::_25Bit => 0x18,
            DataLen::_26Bit => 0x19,
            DataLen::_27Bit => 0x1a,
            DataLen::_28Bit => 0x1b,
            DataLen::_29Bit => 0x1c,
            DataLen::_30Bit => 0x1d,
            DataLen::_31Bit => 0x1e,
            DataLen::_32Bit => 0x1f,
        }
    }
}
