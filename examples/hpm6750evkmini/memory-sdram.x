/* memory-sdram.x - Linker script for SDRAM as main data memory
 *
 * This script places .data and .bss in external SDRAM (0x40000000).
 * Requires SDRAM to be initialized in #[pre_init] before .data/.bss copy.
 *
 * Use with: femc_preinit_memtest example
 *
 * Memory map:
 *   - Code/rodata: XPI0 Flash (0x80000000)
 *   - Fast code:   ILM (0x00000000)
 *   - Fast data:   DLM (0x00080000)
 *   - Main data:   SDRAM (0x40000000) <- .data, .bss, heap
 *   - Stack:       DLM (fast access)
 */

MEMORY
{
    XPI0_HEADER : ORIGIN = 0x80000000, LENGTH = 0x3000 /* bootheader */
    XPI0_APP    : ORIGIN = 0x80003000, LENGTH = 1024K - 0x3000 /* app firmware */

    ILM0        : ORIGIN = 0x00000000, LENGTH =  256K /* instruction local memory */
    DLM0        : ORIGIN = 0x00080000, LENGTH =  256K /* data local memory */

    AXI_SRAM    : ORIGIN = 0x01080000, LENGTH = 1M
    AHB_SRAM    : ORIGIN = 0xF0300000, LENGTH = 32K
    APB_SRAM    : ORIGIN = 0xF40F0000, LENGTH = 8K

    /* External SDRAM - W9812G6JH-6: 16MB (HPM6750EVKMINI) */
    SDRAM       : ORIGIN = 0x40000000, LENGTH = 16M
}

/* Code sections */
REGION_ALIAS("REGION_TEXT", XPI0_APP);
REGION_ALIAS("REGION_FASTTEXT", ILM0);
REGION_ALIAS("REGION_RODATA", XPI0_APP);

/* Fast data in DLM (for interrupt handlers, critical paths) */
REGION_ALIAS("REGION_FASTDATA", DLM0);

/* Main data sections -> SDRAM (requires pre_init SDRAM setup) */
REGION_ALIAS("REGION_DATA", SDRAM);
REGION_ALIAS("REGION_BSS", SDRAM);
REGION_ALIAS("REGION_HEAP", SDRAM);

/* Stack in DLM for fast access (pre_init needs stack before SDRAM is ready) */
REGION_ALIAS("REGION_STACK", DLM0);

/* AHB_SRAM is naturally non-cacheable (on AHB bus, bypasses L1 cache) */
REGION_ALIAS("REGION_NONCACHEABLE_RAM", AHB_SRAM);

/* DMA buffers and non-cacheable data */
SECTIONS {
    .noncacheable_ram (NOLOAD) : ALIGN(4) {
        *(.noncacheable_ram .noncacheable_ram.*)
    } > AHB_SRAM
}
