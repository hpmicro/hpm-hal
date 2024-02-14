/*
 * Copyright (c) 2023 HPMicro
 * SPDX-License-Identifier: BSD-3-Clause
 */

ENTRY(_start)

STACK_SIZE = _stack_size;
HEAP_SIZE = _heap_size;

MEMORY
{
    ILM (wx) : ORIGIN = 0x00000000, LENGTH = 128K
    DLM (w) : ORIGIN = 0x00080000, LENGTH = 128K
    AHB_SRAM (w) : ORIGIN = 0xF0400000, LENGTH = 32k
}

SECTIONS
{
    .start : {
        . = ALIGN(8);
        KEEP(*(.start))
    } > ILM

    .vectors : {
        . = ALIGN(8);
        KEEP(*(.isr_vector))
        KEEP(*(.vector_table))
        KEEP(*(.isr_s_vector))
        KEEP(*(.vector_s_table))
        . = ALIGN(8);
    } > ILM

    .rel : {
        KEEP(*(.rel*))
    } > ILM

    .text : {
        . = ALIGN(8);
        *(.text)
        *(.text*)
        *(.rodata)
        *(.rodata*)
        *(.srodata)
        *(.srodata*)

        *(.hash)
        *(.dyn*)
        *(.gnu*)
        *(.pl*)

        KEEP(*(.eh_frame))
        *(.eh_frame*)

        KEEP (*(.init))
        KEEP (*(.fini))

        /* section information for usbh class */
        . = ALIGN(8);
        __usbh_class_info_start__ = .;
        KEEP(*(.usbh_class_info))
        __usbh_class_info_end__ = .;

        /* RT-Thread related sections - Start */
        /* section information for finsh shell */
        . = ALIGN(4);
        __fsymtab_start = .;
        KEEP(*(FSymTab))
        __fsymtab_end = .;
        . = ALIGN(4);
        __vsymtab_start = .;
        KEEP(*(VSymTab))
        __vsymtab_end = .;
        . = ALIGN(4);

        . = ALIGN(4);
        __rt_init_start = .;
        KEEP(*(SORT(.rti_fn*)))
        __rt_init_end = .;
        . = ALIGN(4);

        /* section information for modules */
        . = ALIGN(4);
        __rtmsymtab_start = .;
        KEEP(*(RTMSymTab))
        __rtmsymtab_end = .;

        /* RT-Thread related sections - end */
        . = ALIGN(8);
        PROVIDE (__etext = .);
        PROVIDE (_etext = .);
        PROVIDE (etext = .);
    } > ILM

    __data_load_addr__ = etext;
    .data : AT(__data_load_addr__) {
        . = ALIGN(8);
        __data_start__ = .;
        __global_pointer$ = . + 0x800;
        *(.data)
        *(.data*)
        *(.sdata)
        *(.sdata*)

        KEEP(*(.jcr))
        KEEP(*(.dynamic))
        KEEP(*(.got*))
        KEEP(*(.got))
        KEEP(*(.gcc_except_table))
        KEEP(*(.gcc_except_table.*))

        . = ALIGN(8);
        PROVIDE(__preinit_array_start = .);
        KEEP(*(.preinit_array))
        PROVIDE(__preinit_array_end = .);

        . = ALIGN(8);
        PROVIDE(__init_array_start = .);
        KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*)))
        KEEP(*(.init_array))
        PROVIDE(__init_array_end = .);

        . = ALIGN(8);
        PROVIDE(__finit_array_start = .);
        KEEP(*(SORT_BY_INIT_PRIORITY(.finit_array.*)))
        KEEP(*(.finit_array))
        PROVIDE(__finit_array_end = .);

        . = ALIGN(8);
        KEEP(*crtbegin*.o(.ctors))
        KEEP(*(EXCLUDE_FILE (*crtend*.o) .ctors))
        KEEP(*(SORT(.ctors.*)))
        KEEP(*(.ctors))

        . = ALIGN(8);
        KEEP(*crtbegin*.o(.dtors))
        KEEP(*(EXCLUDE_FILE (*crtend*.o) .dtors))
        KEEP(*(SORT(.dtors.*)))
        KEEP(*(.dtors))

        . = ALIGN(8);
        __data_end__ = .;
        PROVIDE (__edata = .);
        PROVIDE (_edata = .);
        PROVIDE (edata = .);
    } > DLM

    __fast_load_addr__ = etext + SIZEOF(.data);
    .fast : AT(__fast_load_addr__) {
        . = ALIGN(8);
        PROVIDE(__ramfunc_start__ = .);
        *(.fast)
        *(.fast.*)
        . = ALIGN(8);
        PROVIDE(__ramfunc_end__ = .);
    } > ILM

    .bss (NOLOAD) : {
        . = ALIGN(8);
        __bss_start__ = .;
        *(.bss)
        *(.bss*)
        *(.sbss*)
        *(.scommon)
        *(.scommon*)
        *(.dynsbss*)
        *(COMMON)
        . = ALIGN(8);
        _end = .;
        __bss_end__ = .;
    } > DLM

    .tbss (NOLOAD) : {
        . = ALIGN(8);
        PROVIDE(__tbss_start__ = .);
        __thread_pointer$ = .;
        *(.tbss)
        *(.tbss.*)
        *(.gnu.linkonce.tb.*)
        *(.tcommon)
        . = ALIGN(8);
        PROVIDE(__tbss_end__ = .);
    } > DLM

    __tdata_load_addr__ = etext + SIZEOF(.data) + SIZEOF(.fast);
    .tdata : AT(__tdata_load_addr__) {
        . = ALIGN(8);
        PROVIDE(__tdata_start__ = .);
        *(.tdata)
        *(.tdata.*)
        *(.gnu.linkonce.td.*)
        . = ALIGN(8);
        PROVIDE(__tdata_end__ = .);
    } > DLM

    .framebuffer (NOLOAD) : {
        . = ALIGN(8);
        KEEP(*(.framebuffer))
        . = ALIGN(8);
    } > DLM

    __noncacheable_init_load_addr__ = etext + SIZEOF(.data) + SIZEOF(.fast) + SIZEOF(.tdata);
    .noncacheable.init : AT(__noncacheable_init_load_addr__) {
        . = ALIGN(8);
        __noncacheable_init_start__ = .;
        KEEP(*(.noncacheable.init))
        __noncacheable_init_end__ = .;
        . = ALIGN(8);
    } > DLM

    .noncacheable.bss (NOLOAD) : {
        . = ALIGN(8);
        KEEP(*(.noncacheable))
        __noncacheable_bss_start__ = .;
        KEEP(*(.noncacheable.bss))
        __noncacheable_bss_end__ = .;
        . = ALIGN(8);
    } > DLM

    .ahb_sram (NOLOAD) : {
        KEEP(*(.ahb_sram))
    } > AHB_SRAM

    .fast_ram (NOLOAD) : {
        KEEP(*(.fast_ram))
    } > DLM

    .heap (NOLOAD) : {
        . = ALIGN(8);
        __heap_start__ = .;
        . += HEAP_SIZE;
        __heap_end__ = .;
    } > DLM

    .stack (NOLOAD) : {
        . = ALIGN(8);
        __stack_base__ = .;
        . += STACK_SIZE;
        . = ALIGN(8);
        PROVIDE (_stack = .);
        PROVIDE (_stack_safe = .);
    } > DLM

    __fw_size__ = SIZEOF(.start) + SIZEOF(.vectors) + SIZEOF(.rel) + SIZEOF(.text) + SIZEOF(.data) + SIZEOF(.fast) + SIZEOF(.tdata) + SIZEOF(.noncacheable.init);
    ASSERT(__fw_size__ <= LENGTH(ILM), "******  FAILED! ILM has not enough space!  ******")
}
