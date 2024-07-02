MEMORY
{
    XPI0_HEADER : ORIGIN = 0x80000000, LENGTH = 0x3000 /* bootheader */
    XPI0_APP    : ORIGIN = 0x80003000, LENGTH = 1024K - 0x3000 /* app firmware */

    ILM0       : ORIGIN = 0x00000000, LENGTH =  256K /* instruction local memory */
    DLM0       : ORIGIN = 0x00080000, LENGTH =  256K /* data local memory */

    AXI_SRAM    : ORIGIN = 0x01080000, LENGTH = 1M
    AHB_SRAM    : ORIGIN = 0xF0300000, LENGTH = 32K
    /* PMIC_MEM */
    APB_SRAM    : ORIGIN = 0xF40F0000, LENGTH = 8K
}

REGION_ALIAS("REGION_TEXT", ILM0);
REGION_ALIAS("REGION_RODATA", ILM0);
REGION_ALIAS("REGION_DATA", DLM0);
REGION_ALIAS("REGION_BSS", DLM0)
REGION_ALIAS("REGION_HEAP", DLM0);
REGION_ALIAS("REGION_STACK", DLM0);
REGION_ALIAS("REGION_FASTTEXT", ILM0);

/*
SECTIONS
{
    .fast : ALIGN(4)
    {
        . = ALIGN(8);
         __fast_load_addr__ = LOADADDR(.fast);
        __fast_start_addr__ = .;
        PROVIDE(__ramfunc_start__ = .);
        *(.fast)
        *(.fast.*)
        . = ALIGN(8);
        PROVIDE(__ramfunc_end__ = .);
        __fast_end_addr__ = .;
    } > ILM
}

*/
