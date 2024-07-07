#!/bin/bash

_PWD=$(realpath $(dirname $0))

# if path to HPM_SDK_BASE is not set, then set it to default value
if [ -z "$HPM_SDK_BASE" ]; then
    export HPM_SDK_BASE=$(realpath "${_PWD}/../hpm_sdk")
fi

if [ ! -d "$HPM_SDK_BASE" ]; then
    echo "HPM_SDK_BASE is not set correctly. Please set it to the correct path."
    exit 1
fi

echo "Using HPM_SDK_BASE=${HPM_SDK_BASE}"

# openocd -f $OPENOCD_CFG/probes/ft2232.cfg -f $OPENOCD_CFG/soc/hpm5300.cfg -f $OPENOCD_CFG/boards/hpm5300evk.cfg

# export FAMILY=hpm6e00; export BOARD=hpm6e00evk

# export FAMILY=hpm5300; export BOARD=hpm5301evklite
export FAMILY=hpm5300; export BOARD=hpm5300evk

# export FAMILY=hpm6300; export BOARD=hpm6300evk

# export FAMILY=hpm6750; export BOARD=hpm6750evkmini
# export FAMILY=hpm6750; export BOARD=hpm6750evk
# export FAMILY=hpm6750; export BOARD=hpm6750evk2

export PROBE=ft2232
# export PROBE=cmsis_dap

openocd -c "set HPM_SDK_BASE ${HPM_SDK_BASE}; set BOARD ${BOARD}; set PROBE ${PROBE};" -f ${HPM_SDK_BASE}/boards/openocd/${FAMILY}_all_in_one.cfg
