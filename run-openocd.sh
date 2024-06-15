#!/bin/bash


export HPM_SDK_BASE=/path/to/hpm_sdk

# openocd -f $OPENOCD_CFG/probes/ft2232.cfg -f $OPENOCD_CFG/soc/hpm5300.cfg -f $OPENOCD_CFG/boards/hpm5300evk.cfg

export BOARD=hpm5301evklite
# export BOARD=hpm5300evk
export PROBE=cmsis_dap
# export PROBE=ft2232

openocd -c "set HPM_SDK_BASE ${HPM_SDK_BASE}; set BOARD ${BOARD}; set PROBE ${PROBE};" -f ${HPM_SDK_BASE}/boards/openocd/hpm5300_all_in_one.cfg
