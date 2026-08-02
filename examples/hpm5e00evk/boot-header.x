__nor_cfg_option_load_addr__ = ORIGIN(XPI0_HEADER) + 0x400;
__boot_header_load_addr__ = ORIGIN(XPI0_HEADER) + 0x1000;
__app_load_addr__ = ORIGIN(REGION_TEXT);
__app_offset__ = __app_load_addr__ - __boot_header_load_addr__;
__fw_size__ = LOADADDR(.data) + SIZEOF(.data) - __app_load_addr__;

SECTIONS
{
    .nor_cfg_option __nor_cfg_option_load_addr__ :
    {
        LONG(0xfcf90002)
        LONG(0x00000005)
        LONG(0x00001000)
        LONG(0x00000000)
    } > XPI0_HEADER

    .boot_header __boot_header_load_addr__ :
    {
        __boot_header_start__ = .;
        BYTE(0xbf)
        BYTE(0x10)
        SHORT(0x0090)
        LONG(0)
        SHORT(0)
        BYTE(0)
        BYTE(1)
        SHORT(0)
        SHORT(0)

        LONG(__app_offset__)
        LONG(__fw_size__)
        LONG(0)
        LONG(0)
        LONG(__app_load_addr__)
        LONG(0)
        LONG(_hpm_start)
        LONG(0)
        . += 64;
        . += 32;
        __boot_header_end__ = .;
    } > XPI0_HEADER
} INSERT BEFORE .text;
