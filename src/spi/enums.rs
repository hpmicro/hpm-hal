//! Enums for HPM SPI
//!

/// SPI mode
pub enum Mode {
    Master,
    Slave,
}

/// SPI transfer mode
pub enum TransferMode {
    WriteReadTogether,
    WriteOnly,
    ReadOnly,
    WriteRead,
    ReadWrite,
    WriteDummyRead,
    ReadDummyWrite,
    /// Master mode with CmdEn or AddrEn only
    NoData,
    DummyWrite,
    DummyRead,
}

impl Into<u8> for TransferMode {
    fn into(self) -> u8 {
        match self {
            TransferMode::WriteReadTogether => 0x0,
            TransferMode::WriteOnly => 0x1,
            TransferMode::ReadOnly => 0x2,
            TransferMode::WriteRead => 0x3,
            TransferMode::ReadWrite => 0x4,
            TransferMode::WriteDummyRead => 0x5,
            TransferMode::ReadDummyWrite => 0x6,
            TransferMode::NoData => 0x7,
            TransferMode::DummyWrite => 0x8,
            TransferMode::DummyRead => 0x9,
        }
    }
}

/// Time between CS active and first SCLK edge.
#[derive(Copy, Clone)]
pub enum ChipSelect2SCLK {
    _1HalfSclk,
    _2HalfSclk,
    _3HalfSclk,
    _4HalfSclk,
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

/// SPI lane width
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum SpiWidth {
    /// None
    NONE,
    /// Single lane
    SING,
    /// Dual lanes
    DUAL,
    /// Quad lanes
    QUAD,
}

/// SPI data length
#[derive(Copy, Clone)]
pub enum DataLength {
    _1bit,
    _2bit,
    _3bit,
    _4bit,
    _5bit,
    _6bit,
    _7bit,
    _8bit,
    _9bit,
    _10bit,
    _11bit,
    _12bit,
    _13bit,
    _14bit,
    _15bit,
    _16bit,
    _17bit,
    _18bit,
    _19bit,
    _20bit,
    _21bit,
    _22bit,
    _23bit,
    _24bit,
    _25bit,
    _26bit,
    _27bit,
    _28bit,
    _29bit,
    _30bit,
    _31bit,
    _32bit,
}

impl Into<u8> for DataLength {
    fn into(self) -> u8 {
        match self {
            DataLength::_1bit => 0x00,
            DataLength::_2bit => 0x01,
            DataLength::_3bit => 0x02,
            DataLength::_4bit => 0x03,
            DataLength::_5bit => 0x04,
            DataLength::_6bit => 0x05,
            DataLength::_7bit => 0x06,
            DataLength::_8bit => 0x07,
            DataLength::_9bit => 0x08,
            DataLength::_10bit => 0x09,
            DataLength::_11bit => 0x0a,
            DataLength::_12bit => 0x0b,
            DataLength::_13bit => 0x0c,
            DataLength::_14bit => 0x0d,
            DataLength::_15bit => 0x0e,
            DataLength::_16bit => 0x0f,
            DataLength::_17bit => 0x10,
            DataLength::_18bit => 0x11,
            DataLength::_19bit => 0x12,
            DataLength::_20bit => 0x13,
            DataLength::_21bit => 0x14,
            DataLength::_22bit => 0x15,
            DataLength::_23bit => 0x16,
            DataLength::_24bit => 0x17,
            DataLength::_25bit => 0x18,
            DataLength::_26bit => 0x19,
            DataLength::_27bit => 0x1a,
            DataLength::_28bit => 0x1b,
            DataLength::_29bit => 0x1c,
            DataLength::_30bit => 0x1d,
            DataLength::_31bit => 0x1e,
            DataLength::_32bit => 0x1f,
        }
    }
}

/// SPI Address size
#[derive(Copy, Clone)]
pub enum AddressSize {
    /// 8-bit address
    _8bit,
    /// 16-bit address
    _16bit,
    /// 24-bit address
    _24bit,
    /// 32-bit address
    _32bit,
}

pub enum FiFoSize {
    /// 2 bytes
    _2bytes,
    /// 4 bytes
    _4bytes,
    /// 8 bytes
    _8bytes,
    /// 16 bytes
    _16bytes,
    /// 32 bytes
    _32bytes,
    /// 64 bytes
    _64bytes,
    /// 128 bytes
    _128bytes,
}

impl Into<u8> for FiFoSize {
    fn into(self) -> u8 {
        match self {
            FiFoSize::_2bytes => 0x0,
            FiFoSize::_4bytes => 0x01,
            FiFoSize::_8bytes => 0x02,
            FiFoSize::_16bytes => 0x03,
            FiFoSize::_32bytes => 0x04,
            FiFoSize::_64bytes => 0x05,
            FiFoSize::_128bytes => 0x06,
        }
    }
}

#[derive(Clone, Copy)]
pub enum SlaveModeCommand {
    /// Read controller status, single lane
    ReadControllerStatusSingle,
    /// Read controller status, dual lanes
    ReadControllerStatusDual,
    /// Read controller status, quad lanes
    ReadControllerStatusQuad,
    /// Read data, single lane
    ReadDataSingle,
    /// Read data, dual lanes
    ReadDataDual,
    /// Read data, quad lanes
    ReadDataQuad,
    /// Write data, single lane
    WriteDataSingle,
    /// Write data, dual lanes
    WriteDataDual,
    /// Write data, quad lanes
    WriteDataQuad,
    /// Customized command
    CustomizedCommand(u8),
}

impl Into<u8> for SlaveModeCommand {
    fn into(self) -> u8 {
        match self {
            SlaveModeCommand::ReadControllerStatusSingle => 0x05,
            SlaveModeCommand::ReadControllerStatusDual => 0x15,
            SlaveModeCommand::ReadControllerStatusQuad => 0x25,
            SlaveModeCommand::ReadDataSingle => 0x0b,
            SlaveModeCommand::ReadDataDual => 0x0c,
            SlaveModeCommand::ReadDataQuad => 0x0e,
            SlaveModeCommand::WriteDataSingle => 0x51,
            SlaveModeCommand::WriteDataDual => 0x52,
            SlaveModeCommand::WriteDataQuad => 0x54,
            SlaveModeCommand::CustomizedCommand(c) => c,
        }
    }
}
