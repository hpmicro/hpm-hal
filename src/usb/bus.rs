use core::future::poll_fn;
use core::marker::PhantomData;
use core::sync::atomic::Ordering;
use core::task::Poll;

use embassy_usb_driver::{Direction, EndpointAddress, EndpointInfo, EndpointType, Event, Unsupported};
use embedded_hal::delay::DelayNs;
use hpm_metapac::usb::regs::*;
use riscv::delay::McycleDelay;

use super::{
    ENDPOINT_COUNT, EP_IN_COMPLETE, EP_IN_GENERATION, EP_IN_WAKERS, EP_OUT_COMPLETE, EP_OUT_GENERATION, EP_OUT_WAKERS,
    EndpointState, Instance, init_qhd, local_to_sys_address,
};
use crate::usb::{BUS_WAKER, Config, EpConfig, IRQ_RESET, IRQ_SUSPEND, IRQ_VBUS_CHANGE, reset_dcd_data};

/// USB bus
pub struct Bus<'d, T: Instance> {
    pub(crate) _phantom: PhantomData<&'d T>,
    pub(crate) endpoints_out: [EndpointInfo; ENDPOINT_COUNT],
    pub(crate) endpoints_in: [EndpointInfo; ENDPOINT_COUNT],
    pub(crate) endpoints_enabled_out: [bool; ENDPOINT_COUNT],
    pub(crate) endpoints_enabled_in: [bool; ENDPOINT_COUNT],
    pub(crate) delay: McycleDelay,
    pub(crate) inited: bool,
    pub(crate) config: Config,
    pub(crate) ep_state: &'d EndpointState,
}

/// Implement the `embassy_usb_driver::Bus` trait for `Bus`.
impl<T: Instance> embassy_usb_driver::Bus for Bus<'_, T> {
    /// Enable the USB bus.
    async fn enable(&mut self) {
        // Init the usb phy and device controller
        self.device_init();

        // Set ENDPTLISTADDR, enable interrupts
        {
            let r = T::info().regs;
            // Set endpoint list address (convert to system address for DMA access)
            let qhd_addr = local_to_sys_address(self.ep_state.qhd_list_addr());
            r.endptlistaddr().modify(|w| w.0 = qhd_addr);

            // Clear status
            r.usbsts().write_value(r.usbsts().read());

            // Enable interrupt mask
            r.usbintr().write(|w| {
                w.set_ue(true);
                w.set_uee(true);
                w.set_pce(true);
                w.set_ure(true);
                w.set_sle(true);
            });

            // Enable VBUS change interrupt (A Session Valid Interrupt Enable)
            r.otgsc().modify(|w| {
                w.set_asvis(true); // Clear any pending status first
                w.set_asvie(true); // Enable interrupt
            });
        }
        // Start to run usb device
        self.device_connect();
    }

    /// Disable and powers down the USB peripheral.
    async fn disable(&mut self) {
        self.device_deinit();
    }

    /// Wait for a bus-related event.
    ///
    /// This method should asynchronously wait for an event to happen, then
    /// return it. See [`Event`] for the list of events this method should return.
    async fn poll(&mut self) -> Event {
        poll_fn(|cx| {
            BUS_WAKER.register(cx.waker());
            let r = T::info().regs;

            // Initial VBUS detection
            if !self.inited {
                self.inited = true;
                if self.config.use_internal_vbus {
                    return Poll::Ready(Event::PowerDetected);
                }
                // Check current VBUS state via OTGSC.ASV (A Session Valid)
                if r.otgsc().read().asv() {
                    return Poll::Ready(Event::PowerDetected);
                } else {
                    return Poll::Ready(Event::PowerRemoved);
                }
            }

            // VBUS change event
            if IRQ_VBUS_CHANGE.load(Ordering::Acquire) {
                IRQ_VBUS_CHANGE.store(false, Ordering::Relaxed);
                if self.config.use_internal_vbus {
                    return Poll::Ready(Event::PowerDetected);
                }
                // Check current VBUS state
                if r.otgsc().read().asv() {
                    return Poll::Ready(Event::PowerDetected);
                } else {
                    return Poll::Ready(Event::PowerRemoved);
                }
            }

            // RESET event
            if IRQ_RESET.load(Ordering::Acquire) {
                IRQ_RESET.store(false, Ordering::Relaxed);
                let intr = r.usbintr().read().0 & !0x40;
                r.usbintr().write(|w| w.0 = intr);
                // Reset bus and DCD data before reopening EP0.
                self.endpoints_enabled_in.fill(false);
                self.endpoints_enabled_out.fill(false);
                for generation in &EP_IN_GENERATION {
                    generation.fetch_add(1, Ordering::AcqRel);
                }
                for generation in &EP_OUT_GENERATION {
                    generation.fetch_add(1, Ordering::AcqRel);
                }
                self.device_bus_reset(64);
                // Set ep0 IN/OUT
                self.endpoint_open(EpConfig {
                    transfer: EndpointType::Control as u8,
                    ep_addr: EndpointAddress::from_parts(0, Direction::In),
                    max_packet_size: 64,
                });
                self.endpoint_open(EpConfig {
                    transfer: EndpointType::Control as u8,
                    ep_addr: EndpointAddress::from_parts(0, Direction::Out),
                    max_packet_size: 64,
                });
                // Restore the device interrupt set after EP0 has been rebuilt.
                r.usbintr().write(|w| {
                    w.set_ue(true);
                    w.set_uee(true);
                    w.set_pce(true);
                    w.set_ure(true);
                    w.set_sle(true);
                });
                for w in &EP_IN_WAKERS {
                    w.wake()
                }
                for w in &EP_OUT_WAKERS {
                    w.wake()
                }

                return Poll::Ready(Event::Reset);
            }

            // SUSPEND event
            if IRQ_SUSPEND.load(Ordering::Acquire) {
                IRQ_SUSPEND.store(false, Ordering::Relaxed);
                if r.portsc1().read().susp() {
                    // Note: Host may delay more than 3 ms before and/or after bus reset before doing enumeration.
                    let _device_adr = r.deviceaddr().read().usbadr();
                }
                return Poll::Ready(Event::Suspend);
            }

            Poll::Pending
        })
        .await
    }

    /// Enable or disable an endpoint.
    fn endpoint_set_enabled(&mut self, ep_addr: EndpointAddress, enabled: bool) {
        let was_enabled = if ep_addr.is_in() {
            self.endpoints_enabled_in[ep_addr.index()]
        } else {
            self.endpoints_enabled_out[ep_addr.index()]
        };

        let generation = if ep_addr.is_in() {
            &EP_IN_GENERATION[ep_addr.index()]
        } else {
            &EP_OUT_GENERATION[ep_addr.index()]
        };
        generation.fetch_add(1, Ordering::AcqRel);

        if enabled {
            // Re-selecting a configuration or alternate setting resets the
            // endpoint data toggle and cancels its previous transfer.
            if was_enabled {
                self.endpoint_close(ep_addr);
            }
            let endpoint_list = if ep_addr.direction() == Direction::In {
                self.endpoints_in
            } else {
                self.endpoints_out
            };
            let ep_data = endpoint_list[ep_addr.index()];
            assert!(ep_data.addr == ep_addr);
            self.endpoint_open(EpConfig {
                transfer: ep_data.ep_type as u8,
                ep_addr,
                max_packet_size: ep_data.max_packet_size,
            });
        } else if was_enabled {
            self.endpoint_close(ep_addr);
        }

        if ep_addr.is_in() {
            self.endpoints_enabled_in[ep_addr.index()] = enabled;
            EP_IN_WAKERS[ep_addr.index()].wake();
        } else {
            self.endpoints_enabled_out[ep_addr.index()] = enabled;
            EP_OUT_WAKERS[ep_addr.index()].wake();
        }
    }

    /// Set or clear the STALL condition for an endpoint.
    ///
    /// If the endpoint is an OUT endpoint, it should be prepared to receive data again.
    fn endpoint_set_stalled(&mut self, ep_addr: EndpointAddress, stalled: bool) {
        if stalled {
            self.endpoint_stall(ep_addr);
        } else {
            let enabled = if ep_addr.is_in() {
                self.endpoints_enabled_in[ep_addr.index()]
            } else {
                self.endpoints_enabled_out[ep_addr.index()]
            };
            let generation = if ep_addr.is_in() {
                &EP_IN_GENERATION[ep_addr.index()]
            } else {
                &EP_OUT_GENERATION[ep_addr.index()]
            };
            generation.fetch_add(1, Ordering::AcqRel);

            if enabled {
                self.endpoint_close(ep_addr);
                let ep_data = if ep_addr.is_in() {
                    self.endpoints_in[ep_addr.index()]
                } else {
                    self.endpoints_out[ep_addr.index()]
                };
                self.endpoint_open(EpConfig {
                    transfer: ep_data.ep_type as u8,
                    ep_addr,
                    max_packet_size: ep_data.max_packet_size,
                });
            } else {
                self.endpoint_clean_stall(ep_addr);
            }

            if ep_addr.is_in() {
                EP_IN_WAKERS[ep_addr.index()].wake();
            } else {
                EP_OUT_WAKERS[ep_addr.index()].wake();
            }
        }
    }

    /// Get whether the STALL condition is set for an endpoint.
    fn endpoint_is_stalled(&mut self, ep_addr: EndpointAddress) -> bool {
        let r = T::info().regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).read().txs()
        } else {
            r.endptctrl(ep_addr.index() as usize).read().rxs()
        }
    }

    /// Initiate a remote wakeup of the host by the device.
    ///
    /// This sets the Force Port Resume (FPR) bit in PORTSC1, which causes
    /// the device to drive resume signaling on the bus. The bit is automatically
    /// cleared by hardware when the resume sequence is complete.
    async fn remote_wakeup(&mut self) -> Result<(), Unsupported> {
        let r = T::info().regs;

        // Set Force Port Resume bit
        r.portsc1().modify(|w| w.set_fpr(true));

        // Wait for hardware to complete resume sequence and clear FPR
        while r.portsc1().read().fpr() {}

        Ok(())
    }
}

impl<T: Instance> Bus<'_, T> {
    /// Initialize USB phy
    fn phy_init(&mut self) {
        let r = T::info().regs;
        // Enable dp/dm pulldown
        // In hpm_sdk, this operation is done by `ptr->PHY_CTRL0 &= ~0x001000E0u`.
        // But there's corresponding bits in register, so we write the register directly here.
        r.phy_ctrl0().modify(|w| w.0 = w.0 & (!(0x001000E0)));

        r.otg_ctrl0().modify(|w| {
            w.set_otg_utmi_reset_sw(true);
            w.set_otg_utmi_suspendm_sw(false);
        });

        r.phy_ctrl1().modify(|w| {
            w.set_utmi_cfg_rst_n(false);
        });

        // Wait for reset status
        while !r.otg_ctrl0().read().otg_utmi_reset_sw() {}

        // Set suspend
        r.otg_ctrl0().modify(|w| {
            w.set_otg_utmi_suspendm_sw(true);
        });

        // Delay at least 1us
        self.delay.delay_us(5);

        r.otg_ctrl0().modify(|w| {
            // Disable dm/dp wakeup
            w.set_otg_wkdpdmchg_en(false);
            // Clear reset sw
            w.set_otg_utmi_reset_sw(false);
        });

        // OTG utmi clock detection
        r.phy_status().modify(|w| w.set_utmi_clk_valid(true));
        while !r.phy_status().read().utmi_clk_valid() {}

        r.phy_ctrl0().modify(|w| {
            w.0 |= 0x800;
        });

        // Reset and set suspend
        r.phy_ctrl1().modify(|w| {
            w.set_utmi_cfg_rst_n(true);
            w.set_utmi_otg_suspendm(false);
        });
    }

    // Deinitialize USB phy
    fn phy_deinit(&mut self) {
        let r = T::info().regs;

        r.otg_ctrl0().modify(|w| {
            w.set_otg_utmi_suspendm_sw(true);
            w.set_otg_utmi_reset_sw(false);
        });

        r.phy_ctrl1().modify(|w| {
            w.set_utmi_cfg_rst_n(false);
            w.set_utmi_otg_suspendm(false);
        });
    }

    /// Get port speed: 00: full speed, 01: low speed, 10: high speed, 11: undefined
    #[allow(unused)]
    pub(crate) fn get_port_speed(&mut self) -> u8 {
        let r = T::info().regs;

        r.portsc1().read().pspd()
    }

    /// Reset USB bus
    fn device_bus_reset(&mut self, ep0_max_packet_size: u16) {
        let r = T::info().regs;
        // For each endpoint, first set the transfer type to ANY type other than control.
        // This is because the default transfer type is control, according to hpm_sdk,
        // leaving an un-configured endpoint control will cause undefined behavior
        // for the data PID tracking on the active endpoint.
        for i in 1..ENDPOINT_COUNT {
            r.endptctrl(i as usize).write(|w| {
                w.set_txt(EndpointType::Bulk as u8);
                w.set_rxt(EndpointType::Bulk as u8);
            });
        }
        // Clear all registers(by writing 1 to any non-zero bits)
        r.endptnak().write_value(r.endptnak().read());
        r.endptnaken().write(|w| w.0 = 0);
        r.usbsts().write_value(r.usbsts().read());
        r.endptsetupstat().write_value(r.endptsetupstat().read());
        r.endptcomplete().write_value(r.endptcomplete().read());
        EP_IN_COMPLETE.store(0, Ordering::Release);
        EP_OUT_COMPLETE.store(0, Ordering::Release);
        while r.endptprime().read().0 != 0 {}

        r.endptflush().write(|w| w.0 = 0xFFFF_FFFF);

        while r.endptflush().read().0 != 0 {}

        // Reset DCD_DATA
        unsafe {
            reset_dcd_data(self.ep_state, ep0_max_packet_size);
        }
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
    }

    /// Initialize USB device controller driver
    fn device_init(&mut self) {
        // Initialize phy first
        self.phy_init();

        let r = T::info().regs;

        // Reset controller
        r.usbcmd().modify(|w| w.set_rst(true));
        while r.usbcmd().read().rst() {}

        // Set mode to device IMMEDIATELY after reset
        r.usbmode().modify(|w| w.set_cm(0b10));

        assert_eq!(r.usbmode().read().cm(), 0b10);

        r.usbmode().modify(|w| {
            // Set little endian
            w.set_es(false);
            // Disable setup lockout, please refer to "Control Endpoint Operation" section in RM
            w.set_slom(true);
        });

        r.portsc1().modify(|w| {
            // Parallel interface signal
            w.set_sts(false);
            // Parallel transceiver width
            w.set_ptw(false);
            // Force Full-Speed mode if configured
            w.set_pfsc(self.config.force_full_speed);
        });

        // Do not use interrupt threshold
        r.usbcmd().modify(|w| {
            w.set_itc(0);
        });

        // Enable VBUS discharge
        r.otgsc().modify(|w| {
            w.set_vd(true);
        });
    }

    /// Set device address
    fn device_set_address(&mut self, addr: u8) {
        let r = T::info().regs;
        r.deviceaddr().modify(|w| {
            w.set_usbadr(addr);
            w.set_usbadra(true);
        });
    }

    /// Deinitialize USB device controller driver
    fn device_deinit(&mut self) {
        let r = T::info().regs;

        // Stop first
        r.usbcmd().modify(|w| w.set_rs(false));

        // Reset controller
        r.usbcmd().modify(|w| w.set_rst(true));
        while r.usbcmd().read().rst() {}

        // Disable phy
        self.phy_deinit();

        // Reset endpoint list address register, status register and interrupt enable register
        r.endptlistaddr().write_value(Endptlistaddr(0));
        r.usbsts().write_value(Usbsts::default());
        r.usbintr().write_value(Usbintr(0));
    }

    /// Connect by enabling internal pull-up resistor on D+/D-
    fn device_connect(&mut self) {
        let r = T::info().regs;
        r.usbcmd().modify(|w| {
            w.set_rs(true);
        });
    }

    /// Open the endpoint
    fn endpoint_open(&mut self, ep_config: EpConfig) {
        // defmt::info!("Enabling endpoint: {:?}", ep_config.ep_addr);
        if ep_config.ep_addr.index() >= ENDPOINT_COUNT {
            return;
        }

        // Prepare queue head
        unsafe { init_qhd(self.ep_state, &ep_config) };

        // Open endpoint
        let ep_num = ep_config.ep_addr.index();

        // Enable EP control
        let r = T::info().regs;
        r.endptctrl(ep_num as usize).modify(|w| {
            // Clear the RXT or TXT bits
            if ep_config.ep_addr.is_in() {
                w.set_txt(0);
                w.set_txe(true);
                w.set_txr(true);
                w.set_txt(ep_config.transfer as u8);
            } else {
                w.set_rxt(0);
                w.set_rxe(true);
                w.set_rxr(true);
                w.set_rxt(ep_config.transfer as u8);
            }
        });
    }

    /// Close the endpoint
    fn endpoint_close(&mut self, ep_addr: EndpointAddress) {
        let r = T::info().regs;

        let ep_bit = 1 << ep_addr.index();

        // Flush the endpoint first
        if ep_addr.is_in() {
            loop {
                r.endptflush().modify(|w| w.set_fetb(ep_bit));
                while (r.endptflush().read().fetb() & ep_bit) != 0 {}
                if r.endptstat().read().etbr() & ep_bit == 0 {
                    break;
                }
            }
        } else {
            loop {
                r.endptflush().modify(|w| w.set_ferb(ep_bit));
                while (r.endptflush().read().ferb() & ep_bit) != 0 {}
                if r.endptstat().read().erbr() & ep_bit == 0 {
                    break;
                }
            }
        }

        // Disable endpoint
        r.endptctrl(ep_addr.index() as usize).modify(|w| {
            if ep_addr.is_in() {
                w.set_txt(0);
                w.set_txe(false);
                w.set_txs(false);
            } else {
                w.set_rxt(0);
                w.set_rxe(false);
                w.set_rxs(false);
            }
        });

        // Set transfer type back to ANY type other than control
        r.endptctrl(ep_addr.index() as usize).modify(|w| {
            if ep_addr.is_in() {
                w.set_txt(EndpointType::Bulk as u8);
            } else {
                w.set_rxt(EndpointType::Bulk as u8);
            }
        });
    }

    fn endpoint_stall(&mut self, ep_addr: EndpointAddress) {
        let r = T::info().regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).modify(|w| w.set_txs(true));
        } else {
            r.endptctrl(ep_addr.index() as usize).modify(|w| w.set_rxs(true));
        }
    }

    fn endpoint_clean_stall(&mut self, ep_addr: EndpointAddress) {
        let r = T::info().regs;

        r.endptctrl(ep_addr.index() as usize).modify(|w| {
            if ep_addr.is_in() {
                // Data toggle also need to be reset
                w.set_txr(true);
                w.set_txs(false);
            } else {
                w.set_rxr(true);
                w.set_rxs(false);
            }
        });
    }
}
