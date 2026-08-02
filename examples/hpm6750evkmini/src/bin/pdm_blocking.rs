//! PDM microphone blocking read example for HPM6750EVKMini
//!
//! Hardware: SPH0641LM4H PDM mic on PY10 (CLK) + PY11 (DAT)
//!
//! This example reads PDM microphone data using blocking mode.
//! Audio clock is manually configured as a temporary workaround.

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::pdm::{self, ChannelMask, Config};
use {defmt_rtt as _, panic_halt as _};

/// Debug: Print clock configuration registers
fn debug_print_clock_regs() {
    use hpm_hal::pac::{SYSCTL, PLLCTL, clocks};

    // Check PLL3 configuration
    // Fout = Fref/refdiv*(fbdiv + frac/2^24)/postdiv1
    let pll3_cfg0 = PLLCTL.pll(3).cfg0().read();
    let pll3_cfg2 = PLLCTL.pll(3).cfg2().read();
    let pll3_freq = PLLCTL.pll(3).freq().read();
    let pll3_div0 = PLLCTL.pll(3).div0().read();

    info!(
        "[CLK] PLL3 cfg0: refdiv={}, postdiv1={}, dsmpd={}",
        pll3_cfg0.refdiv(),
        pll3_cfg0.postdiv1(),
        pll3_cfg0.dsmpd()  // 1=int mode, 0=frac mode
    );
    info!(
        "[CLK] PLL3 cfg2: fbdiv_int={}",
        pll3_cfg2.fbdiv_int()
    );
    info!(
        "[CLK] PLL3 freq: fbdiv_frac={}, frac=0x{:06x}",
        pll3_freq.fbdiv_frac(),
        pll3_freq.frac()
    );
    info!(
        "[CLK] PLL3 div0: div={}, enable={}, response={}",
        pll3_div0.div(),
        pll3_div0.enable(),
        pll3_div0.response()
    );

    // Calculate PLL3 output (assuming 24MHz ref)
    // PLL3_VCO = 24MHz * fbdiv / refdiv / postdiv1
    // PLL3_CLK0 = PLL3_VCO / (div0 + 1)
    let refdiv = pll3_cfg0.refdiv().max(1) as u32;
    let postdiv1 = pll3_cfg0.postdiv1().max(1) as u32;
    let fbdiv = if pll3_cfg0.dsmpd() {
        pll3_cfg2.fbdiv_int() as u32
    } else {
        pll3_freq.fbdiv_frac() as u32
    };
    let div0 = pll3_div0.div() as u32 + 1;

    // Simplified calculation (ignoring frac for now)
    let pll3_vco_mhz = 24 * fbdiv / refdiv / postdiv1;
    let pll3_clk0_mhz = pll3_vco_mhz / div0;
    info!(
        "[CLK] PLL3 estimated: VCO~{}MHz, CLK0~{}MHz",
        pll3_vco_mhz, pll3_clk0_mhz
    );

    let aud0 = SYSCTL.clock(clocks::AUD0).read();
    info!(
        "[CLK] AUD0: mux={}, div={}, loc_busy={}",
        aud0.mux() as u8,
        aud0.div(),
        aud0.loc_busy()
    );

    let i2s0clk = SYSCTL.i2sclk(0).read();
    info!(
        "[CLK] I2S0CLK: mux={}, loc_busy={}",
        i2s0clk.mux() as u8,
        i2s0clk.loc_busy()
    );

    // AUD0 = PLL3_CLK0 / (div + 1)
    // I2S0_MCLK = AUD0 (when mux=1)
    let aud0_div = aud0.div() as u32 + 1;
    let aud0_mhz = pll3_clk0_mhz / aud0_div;
    info!(
        "[CLK] Estimated: AUD0~{}MHz (MCLK for I2S0/PDM)",
        aud0_mhz
    );
}

/// Debug: Print PDM registers
fn debug_print_pdm_regs() {
    use hpm_hal::pac::PDM;

    let ctrl = PDM.ctrl().read();
    info!(
        "[PDM] CTRL: hfdiv={}, clk_oe={}, dec_aft_cic={}, div_bypass={}",
        ctrl.pdm_clk_hfdiv(),
        ctrl.pdm_clk_oe(),
        ctrl.dec_aft_cic(),
        ctrl.pdm_clk_div_bypass()
    );

    let ch_ctrl = PDM.ch_ctrl().read();
    info!(
        "[PDM] CH_CTRL: ch_en=0x{:02x}, ch_pol=0x{:02x}",
        ch_ctrl.ch_en(),
        ch_ctrl.ch_pol()
    );

    let cic_cfg = PDM.cic_cfg().read();
    info!(
        "[PDM] CIC_CFG: sgd={}, dec_ratio={}, post_scale={}",
        cic_cfg.sgd() as u8,
        cic_cfg.cic_dec_ratio(),
        cic_cfg.post_scale()
    );

    let run = PDM.run().read();
    info!("[PDM] RUN: pdm_en={}", run.pdm_en());

    // Check status/error register
    let st = PDM.st().read();
    info!(
        "[PDM] ST: cic_sat=0x{:02x}, cic_ovld={}, ofifo_ovfl={}, filt_crx={}",
        st.cic_sat_err(),
        st.cic_ovld_err(),
        st.ofifo_ovfl_err(),
        st.filt_crx_err()
    );

    // Calculate PDM_CLK and sample rate
    // PDM_CLK = MCLK / (2 * (hfdiv + 1))  when bypass=0
    // Sample rate = PDM_CLK / CIC_DEC_RATIO / DEC_AFT_CIC
    let hfdiv = ctrl.pdm_clk_hfdiv() as u32;
    let cic_dec = cic_cfg.cic_dec_ratio() as u32;
    let dec_aft_cic = ctrl.dec_aft_cic() as u32;
    info!(
        "[PDM] Calculated: pdm_clk=MCLK/{}, sample_rate=MCLK/{}",
        2 * (hfdiv + 1),
        2 * (hfdiv + 1) * cic_dec * dec_aft_cic
    );
}

/// Debug: Print I2S0 registers
fn debug_print_i2s_regs() {
    use hpm_hal::pac::I2S0;

    let ctrl = I2S0.ctrl().read();
    info!(
        "[I2S0] CTRL: i2s_en={}, rx_en={}, tx_en={}",
        ctrl.i2s_en(),
        ctrl.rx_en(),
        ctrl.tx_en()
    );

    let cfgr = I2S0.cfgr().read();
    info!(
        "[I2S0] CFGR: bclk_div={}, tdm_en={}, ch_max={}, std={}, datsiz={}, chsiz={}",
        cfgr.bclk_div(),
        cfgr.tdm_en(),
        cfgr.ch_max(),
        cfgr.std() as u8,
        cfgr.datsiz() as u8,
        cfgr.chsiz() as u8
    );

    // Check master/slave mode
    // mck_sel_op, bclk_sel_op, fclk_sel_op: 0=internal(master), 1=external(slave)
    info!(
        "[I2S0] CFGR: mck_sel_op={}, bclk_sel_op={}, fclk_sel_op={}, bclk_gateoff={}",
        cfgr.mck_sel_op(),
        cfgr.bclk_sel_op(),
        cfgr.fclk_sel_op(),
        cfgr.bclk_gateoff()
    );

    let misc_cfgr = I2S0.misc_cfgr().read();
    info!(
        "[I2S0] MISC_CFGR: mclkoe={}, mclk_gateoff={}",
        misc_cfgr.mclkoe(),
        misc_cfgr.mclk_gateoff()
    );

    let rxdslot0 = I2S0.rxdslot(0).read();
    info!("[I2S0] RXDSLOT0: en=0x{:04x}", rxdslot0.en());

    let fifo = I2S0.rfifo_fillings().read();
    info!("[I2S0] RFIFO_FILLINGS: rx0={}", fifo.rx0());

    // Calculate I2S frame rate
    // BCLK = MCLK / bclk_div
    // Frame rate = BCLK / (channel_length * channels)
    // channel_length: 0=16bit, 1=32bit
    // channels = ch_max (in TDM mode)
    let bclk_div = cfgr.bclk_div();
    let ch_len_bits: u32 = if cfgr.chsiz() as u8 == 1 { 32 } else { 16 };
    let ch_num = cfgr.ch_max() as u32;
    info!(
        "[I2S0] Calculated: bclk_div={}, ch_len={}bit, ch_num={}",
        bclk_div, ch_len_bits, ch_num
    );
}

/// Debug: Print pin configuration
fn debug_print_pin_config() {
    use hpm_hal::pac::{IOC, PIOC};

    // PY10 = pin 10 in port Y (port 14)
    // PY11 = pin 11 in port Y (port 14)
    // Pad index = port * 32 + pin
    const PY10_PAD: usize = 14 * 32 + 10; // 458
    const PY11_PAD: usize = 14 * 32 + 11; // 459

    let py10_ioc = IOC.pad(PY10_PAD).func_ctl().read();
    let py11_ioc = IOC.pad(PY11_PAD).func_ctl().read();
    info!(
        "[PIN] IOC: PY10_alt={}, PY11_alt={}",
        py10_ioc.alt_select(),
        py11_ioc.alt_select()
    );

    // PIOC uses different indexing - just pin number within PY port
    let py10_pioc = PIOC.pad(10).func_ctl().read();
    let py11_pioc = PIOC.pad(11).func_ctl().read();
    info!(
        "[PIN] PIOC: PY10_alt={}, PY11_alt={}",
        py10_pioc.alt_select(),
        py11_pioc.alt_select()
    );
}

/// Debug: Verify audio clock configuration
///
/// Audio clocks are now configured by hpm_hal::init() via sysctl::Config::default():
/// - AUD0/AUD1/AUD2 = PLL3_CLK0 / 25 = 24.576MHz
/// - I2S clock source is set by PDM driver in configure_i2s0_for_pdm()
fn verify_audio_clock() {
    let aud0_freq = hpm_hal::sysctl::get_audio_clock_freq(0);
    info!("Audio clock AUD0: {} Hz", aud0_freq.0);
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let config = hpm_hal::Config::default();
    let p = hpm_hal::init(config);

    info!("=== PDM blocking read example ===");

    // Audio clocks are configured by hpm_hal::init() via Config::default()
    verify_audio_clock();

    info!("--- After clock config ---");
    debug_print_clock_regs();

    // PDM configuration: stereo (Ch0 + Ch4 on D0)
    let mut pdm_config = Config::default();
    pdm_config.channels = ChannelMask::DUAL_STEREO;

    info!(
        "PDM config: channels=0x{:04x}, cic_ratio={}, sigma_delta_order={:?}",
        pdm_config.channels.0,
        pdm_config.cic_decimation_ratio,
        pdm_config.sigma_delta_order
    );

    // Create PDM driver
    // PY10 = PDM_CLK, PY11 = PDM_D0
    let mut pdm = pdm::Pdm::new(
        p.PDM,
        p.I2S0,
        p.PY10, // CLK
        p.PY11, // D0
        pdm_config,
    );

    info!("--- After PDM::new() ---");
    debug_print_pin_config();
    debug_print_pdm_regs();
    debug_print_i2s_regs();

    // Start PDM
    pdm.start();

    info!("--- After PDM::start() ---");
    debug_print_pdm_regs();
    debug_print_i2s_regs();

    // Wait for CIC filter to stabilize and clear startup transient errors
    Timer::after_millis(10).await;
    pdm.clear_errors();
    info!("Cleared startup transient errors");

    // Read buffer
    let mut buf = [0u32; 64];
    let mut sample_count: u32 = 0;

    loop {
        // Read samples (blocking)
        pdm.read_blocking(&mut buf);
        sample_count += buf.len() as u32;

        // Check for errors (should not occur after stabilization)
        if let Err(e) = pdm.check_errors() {
            info!("PDM error: {:?}", e);
            pdm.clear_errors();
        }

        // Analyze first few samples
        let mut ch0_sum: i64 = 0;
        let mut ch4_sum: i64 = 0;
        let mut ch0_count = 0;
        let mut ch4_count = 0;

        for &raw in &buf {
            let sample = pdm::extract_sample(raw);
            let ch_id = pdm::extract_channel_id(raw);

            match ch_id {
                0 => {
                    ch0_sum += sample as i64;
                    ch0_count += 1;
                }
                4 => {
                    ch4_sum += sample as i64;
                    ch4_count += 1;
                }
                _ => {}
            }
        }

        // Print statistics every ~1 second (16000 samples @ 16kHz)
        if sample_count % 16000 < 64 {
            let ch0_avg = if ch0_count > 0 { ch0_sum / ch0_count as i64 } else { 0 };
            let ch4_avg = if ch4_count > 0 { ch4_sum / ch4_count as i64 } else { 0 };

            info!(
                "Samples: {}, Ch0 avg: {}, Ch4 avg: {}, First raw: 0x{:08x}",
                sample_count,
                ch0_avg,
                ch4_avg,
                buf[0]
            );
        }

        Timer::after_millis(1).await;
    }
}

