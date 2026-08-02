/* HPM6750EVKMINI Memory Map for XPI Flash boot (matches C SDK style)
 *
 * IMPORTANT: HPM6750 Hardware Bug - DMA cannot access ILM/DLM reliably!
 * When CPU and DMA access ILM/DLM simultaneously, DMA may read all zeros.
 *
 * Memory Strategy (same as HPM C SDK):
 * - DATA/BSS/HEAP/STACK: AXI_SRAM (DMA accessible, hpm-hal handles cache)
 * - NONCACHEABLE_RAM: AXI_SRAM末尾64KB (PMA配置为non-cacheable)
 * - RTT + NONCACHEABLE: hpm-riscv-rt的hpm67-fix+pma-noncacheable一起配置
 *
 * DMA Buffer Usage:
 *   // Option 1: PMA non-cacheable (64KB, recommended for DMA)
 *   #[link_section = ".noncacheable.bss"]
 *   static mut DMA_BUF: [u8; 1024] = [0; 1024];
 *
 *   // Option 2: Stack/heap (hpm-hal SPI driver auto-handles cache)
 *   let mut buf = [0u8; 256];
 *
 *   // Option 3: Hardware non-cacheable AHB_SRAM (32KB)
 *   #[link_section = ".ahb_sram"]
 *   static mut DMA_BUF: [u8; 256] = [0; 256];
 */

/* Non-cacheable region size (must be power of 2 for PMA NAPOT) */
_noncacheable_size = 64K;

MEMORY
{
    XPI0_HEADER : ORIGIN = 0x80000000, LENGTH = 0x3000 /* bootheader */
    XPI0_APP    : ORIGIN = 0x80003000, LENGTH = 1024K - 0x3000 /* app firmware */

    ILM0        : ORIGIN = 0x00000000, LENGTH = 256K /* instruction local memory */
    DLM0        : ORIGIN = 0x00080000, LENGTH = 256K /* DO NOT USE FOR DMA - hardware bug! */

    /* AXI_SRAM: 1MB total, last 64KB reserved for non-cacheable (via PMA) */
    AXI_SRAM         : ORIGIN = 0x01080000, LENGTH = 1M - _noncacheable_size
    NONCACHEABLE_RAM : ORIGIN = 0x01180000 - _noncacheable_size, LENGTH = _noncacheable_size

    AHB_SRAM    : ORIGIN = 0xF0300000, LENGTH = 32K  /* hardware non-cacheable (AHB bus) */
    APB_SRAM    : ORIGIN = 0xF40F0000, LENGTH = 8K

    SDRAM       : ORIGIN = 0x40000000, LENGTH = 32M
}

REGION_ALIAS("REGION_TEXT", XPI0_APP);
REGION_ALIAS("REGION_FASTTEXT", ILM0);
REGION_ALIAS("REGION_FASTDATA", DLM0);  /* Fast data without DMA access */
REGION_ALIAS("REGION_RODATA", XPI0_APP);

/* Use AXI_SRAM for data/bss/heap/stack - DMA accessible, hpm-hal handles cache */
REGION_ALIAS("REGION_DATA", AXI_SRAM);
REGION_ALIAS("REGION_BSS", AXI_SRAM);
REGION_ALIAS("REGION_HEAP", AXI_SRAM);
REGION_ALIAS("REGION_STACK", AXI_SRAM);

/* Non-cacheable region via PMA (hpm-riscv-rt pma-noncacheable feature) */
REGION_ALIAS("REGION_NONCACHEABLE_RAM", NONCACHEABLE_RAM);

/* Symbols for pma-noncacheable feature */
__noncacheable_start__ = ORIGIN(NONCACHEABLE_RAM);
__noncacheable_end__ = ORIGIN(NONCACHEABLE_RAM) + LENGTH(NONCACHEABLE_RAM);
