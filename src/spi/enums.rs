//! Enums for HPM SPI
//!

/// Time between CS active and SCLK edge.
#[derive(Copy, Clone)]
pub enum ChipSelect2SCLK {
    _1HalfSclk,
    _2HalfSclk,
    _3HalfSclk,
    _4HalfSclk,
}

impl Into<u8> for ChipSelect2SCLK {
    fn into(self) -> u8 {
        match self {
            ChipSelect2SCLK::_1HalfSclk => 0x00,
            ChipSelect2SCLK::_2HalfSclk => 0x01,
            ChipSelect2SCLK::_3HalfSclk => 0x02,
            ChipSelect2SCLK::_4HalfSclk => 0x03,
        }
    }
}

/// Time the Chip Select line stays high.
#[derive(Copy, Clone)]
pub enum ChipSelectHighTime {
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

impl Into<u8> for ChipSelectHighTime {
    fn into(self) -> u8 {
        match self {
            ChipSelectHighTime::_1HalfSclk => 0b0000,
            ChipSelectHighTime::_2HalfSclk => 0b0001,
            ChipSelectHighTime::_3HalfSclk => 0b0010,
            ChipSelectHighTime::_4HalfSclk => 0b0011,
            ChipSelectHighTime::_5HalfSclk => 0b0100,
            ChipSelectHighTime::_6HalfSclk => 0b0101,
            ChipSelectHighTime::_7HalfSclk => 0b0110,
            ChipSelectHighTime::_8HalfSclk => 0b0111,
            ChipSelectHighTime::_9HalfSclk => 0b1000,
            ChipSelectHighTime::_10HalfSclk => 0b1001,
            ChipSelectHighTime::_11HalfSclk => 0b1010,
            ChipSelectHighTime::_12HalfSclk => 0b1011,
            ChipSelectHighTime::_13HalfSclk => 0b1100,
            ChipSelectHighTime::_14HalfSclk => 0b1101,
            ChipSelectHighTime::_15HalfSclk => 0b1110,
            ChipSelectHighTime::_16HalfSclk => 0b1111,
        }
    }
}

/// SPI data length
#[derive(Copy, Clone)]
pub enum DataLength {
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

impl Into<u8> for DataLength {
    fn into(self) -> u8 {
        match self {
            DataLength::_1Bit => 0x00,
            DataLength::_2Bit => 0x01,
            DataLength::_3Bit => 0x02,
            DataLength::_4Bit => 0x03,
            DataLength::_5Bit => 0x04,
            DataLength::_6Bit => 0x05,
            DataLength::_7Bit => 0x06,
            DataLength::_8Bit => 0x07,
            DataLength::_9Bit => 0x08,
            DataLength::_10Bit => 0x09,
            DataLength::_11Bit => 0x0a,
            DataLength::_12Bit => 0x0b,
            DataLength::_13Bit => 0x0c,
            DataLength::_14Bit => 0x0d,
            DataLength::_15Bit => 0x0e,
            DataLength::_16Bit => 0x0f,
            DataLength::_17Bit => 0x10,
            DataLength::_18Bit => 0x11,
            DataLength::_19Bit => 0x12,
            DataLength::_20Bit => 0x13,
            DataLength::_21Bit => 0x14,
            DataLength::_22Bit => 0x15,
            DataLength::_23Bit => 0x16,
            DataLength::_24Bit => 0x17,
            DataLength::_25Bit => 0x18,
            DataLength::_26Bit => 0x19,
            DataLength::_27Bit => 0x1a,
            DataLength::_28Bit => 0x1b,
            DataLength::_29Bit => 0x1c,
            DataLength::_30Bit => 0x1d,
            DataLength::_31Bit => 0x1e,
            DataLength::_32Bit => 0x1f,
        }
    }
}
