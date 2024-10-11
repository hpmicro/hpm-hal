MEMORY
{
    ILM0       : ORIGIN = 0x00000000, LENGTH =  128K /* instruction local memory */
    DLM0       : ORIGIN = 0x00080000, LENGTH =  128K /* data local memory */

    AXI_SRAM    : ORIGIN = 0x01080000, LENGTH = 256K
    AHB_SRAM    : ORIGIN = 0xF0300000, LENGTH = 32K
}
REGION_ALIAS("REGION_TEXT", ILM0);
REGION_ALIAS("REGION_RODATA", ILM0);
REGION_ALIAS("REGION_DATA", DLM0);
REGION_ALIAS("REGION_BSS", DLM0)
REGION_ALIAS("REGION_HEAP", DLM0);
REGION_ALIAS("REGION_STACK", DLM0);
REGION_ALIAS("REGION_FASTTEXT", ILM0);
