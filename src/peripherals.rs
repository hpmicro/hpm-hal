use crate::pac;

// We need to export this in the hal for the drivers to use

crate::peripherals! {
    GPIO0 <= GPIO0,
    // Fast GPIO controller
    FGPIO0 <= FGPIO,
    // Power domain GPIO controller
    PGPIO <= PGPIO,


    UART0 <= UART0,
    UART2 <= UART2,

    I2C0 <= I2C0,
    I2C1 <= I2C1,

    SPI1 <= SPI1,
}
