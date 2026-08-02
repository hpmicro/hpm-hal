/* HPM6E00EVK Memory Map for XPI Flash boot
 *
 * Memory Strategy:
 * - DATA/BSS/HEAP/STACK: DLM (fast local memory)
 * - NONCACHEABLE_RAM: AXI_SRAM末尾64KB (PMA配置为non-cacheable)
 *
 * DMA Buffer Usage:
 *   // PMA non-cacheable region (recommended for DMA/Ethernet)
 *   #[link_section = ".noncacheable"]
 *   static mut DMA_BUF: [u8; 1024] = [0; 1024];
 */

/* Non-cacheable region size (must be power of 2 for PMA NAPOT) */
_noncacheable_size = 64K;

MEMORY
{
    /* MX25L12833FM2I-10G: 16M */
    XPI0_HEADER : ORIGIN = 0x80000000, LENGTH = 0x3000 /* bootheader */
    XPI0_APP    : ORIGIN = 0x80003000, LENGTH = 16M - 0x3000 /* app firmware */

    ILM0        : ORIGIN = 0x00000000, LENGTH = 256K /* instruction local memory */
    DLM0        : ORIGIN = 0x00200000, LENGTH = 256K /* data local memory */

    /* AXI_SRAM: 768KB total (0x01200000-0x012C0000), last 64KB reserved for non-cacheable */
    AXI_SRAM         : ORIGIN = 0x01200000, LENGTH = 768K - _noncacheable_size
    NONCACHEABLE_RAM : ORIGIN = 0x012C0000 - _noncacheable_size, LENGTH = _noncacheable_size

    SHARE_RAM   : ORIGIN = 0x012FC000, LENGTH = 16K
    AHB_SRAM    : ORIGIN = 0xF0200000, LENGTH = 32K
}

REGION_ALIAS("REGION_TEXT", XPI0_APP);
REGION_ALIAS("REGION_FASTTEXT", ILM0);
REGION_ALIAS("REGION_FASTDATA", DLM0);
REGION_ALIAS("REGION_RODATA", XPI0_APP);
REGION_ALIAS("REGION_DATA", DLM0);
REGION_ALIAS("REGION_BSS", DLM0);
REGION_ALIAS("REGION_HEAP", DLM0);
REGION_ALIAS("REGION_STACK", DLM0);

/* Non-cacheable region via PMA (hpm-riscv-rt pma-noncacheable feature) */
REGION_ALIAS("REGION_NONCACHEABLE_RAM", NONCACHEABLE_RAM);

/* Symbols for pma-noncacheable feature */
__noncacheable_start__ = ORIGIN(NONCACHEABLE_RAM);
__noncacheable_end__ = ORIGIN(NONCACHEABLE_RAM) + LENGTH(NONCACHEABLE_RAM);

