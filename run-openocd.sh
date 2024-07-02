#!/bin/bash


# export HPM_SDK_BASE=/path/to/hpm_sdk

export HPM_SDK_BASE=`pwd`/../hpm_sdk

# openocd -f $OPENOCD_CFG/probes/ft2232.cfg -f $OPENOCD_CFG/soc/hpm5300.cfg -f $OPENOCD_CFG/boards/hpm5300evk.cfg

export FAMILY=hpm6e00; export BOARD=hpm6e00evk

# export FAMILY=hpm5300; export BOARD=hpm5301evklite
# export FAMILY=hpm5300; export BOARD=hpm5300evk

# export FAMILY=hpm6300; export BOARD=hpm6300evk


export PROBE=ft2232
# export PROBE=cmsis_dap

openocd -c "set HPM_SDK_BASE ${HPM_SDK_BASE}; set BOARD ${BOARD}; set PROBE ${PROBE};" -f ${HPM_SDK_BASE}/boards/openocd/${FAMILY}_all_in_one.cfg
