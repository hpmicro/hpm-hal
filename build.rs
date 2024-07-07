use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use hpm_metapac::metadata::METADATA;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

// HPM_SOC_IP_FEATURE
fn get_ip_features(chip_family: &str) -> &[&str] {
    match chip_family {
        "hpm67" | "hpm64" => &["ADC16_HAS_TEMPSNS"],
        "hpm63" => &["PWM_COUNTER_RESET"],
        "hpm62" => &["UART_RX_IDLE_DETECT", "PWM_COUNTER_RESET", "PWM_HRPWM"],
        "hpm53" => &[
            "GPTMR_MONITOR",
            "GPTMR_OP_MODE",
            "UART_RX_IDLE_DETECT",
            "UART_FCRR",
            "UART_RX_EN",
            "UART_E00018_FIX",
            "UART_9BIT_MODE",
            "UART_ADDR_MATCH",
            "UART_TRIG_MODE",
            "UART_FINE_FIFO_THRLD",
            "UART_IIR2",
            "I2C_SUPPORT_RESET",
            "SPI_NEW_TRANS_COUNT",
            "SPI_CS_SELECT",
            "SPI_SUPPORT_DIRECTIO",
            "PWM_COUNTER_RESET",
            "ADC16_HAS_MOT_EN",
        ],
        "hpm68" => &[
            "UART_RX_IDLE_DETECT",
            "UART_FCRR",
            "UART_RX_EN",
            "I2C_SUPPORT_RESET",
            "SPI_NEW_TRANS_COUNT",
            "SPI_CS_SELECT",
            "SPI_SUPPORT_DIRECTIO",
            "GPTMR_MONITOR",
            "GPTMR_OP_MODE",
            "DAO_DATA_FORMAT_CONFIG",
            "CAM_INV_DEN",
        ],
        "hpm6e" => &[
            "GPTMR_MONITOR",
            "GPTMR_OP_MODE",
            "GPTMR_CNT_MODE",
            "UART_RX_IDLE_DETECT",
            "UART_FCRR",
            "UART_RX_EN",
            "UART_E00018_FIX",
            "UART_9BIT_MODE",
            "UART_ADDR_MATCH",
            "UART_TRIG_MODE",
            "UART_FINE_FIFO_THRLD",
            "UART_IIR2",
            "I2C_SUPPORT_RESET",
            "SPI_NEW_TRANS_COUNT",
            "SPI_CS_SELECT",
            "SPI_SUPPORT_DIRECTIO",
            "DMAV2_BURST_IN_FIXED_TRANS",
            "DMAV2_BYTE_ORDER_SWAP",
            "ADC16_HAS_MOT_EN",
            "DAO_DATA_FORMAT_CONFIG",
            "QEIV2_ONESHOT_MODE",
            "QEIV2_SW_RESTART_TRG",
            "QEIV2_TIMESTAMP",
            "QEIV2_ADC_THRESHOLD",
            "RDC_IIR",
            "SEI_RX_LATCH_FEATURE",
            "SEI_ASYNCHRONOUS_MODE_V2",
            "SEI_TIMEOUT_REWIND_FEATURE",
            "SEI_HAVE_DAT10_31",
            "SEI_HAVE_INTR64_255",
            "SEI_HAVE_CTRL2_12",
            "SEI_HAVE_PTCD",
            "ENET_HAS_MII_MODE",
            "FFA_FP32",
        ],
        _ => panic!("Unknown chip family: {}", chip_family),
    }
}

fn main() {
    let mut cfgs = CfgSet::new();

    let chip_name = match env::vars()
        .map(|(a, _)| a)
        .filter(|x| x.starts_with("CARGO_FEATURE_HPM"))
        .get_one()
    {
        Ok(x) => x,
        Err(GetOneError::None) => panic!("No hpmxxxx Cargo feature enabled"),
        Err(GetOneError::Multiple) => panic!("Multiple hpmxxxx Cargo features enabled"),
    }
    .strip_prefix("CARGO_FEATURE_")
    .unwrap()
    .to_ascii_lowercase();

    eprintln!("chip: {chip_name}");

    // hpm53, hpm67, etc
    let family_name = chip_name[0..5].to_ascii_lowercase();
    cfgs.enable(&family_name);

    // IP feature gates, usesage: #[cfg(ip_feature_adc16_has_tempsns)]
    for feature in get_ip_features(&family_name) {
        cfgs.enable(&format!("ip_feature_{}", feature.to_ascii_lowercase()));
    }

    for p in METADATA.peripherals {
        if let Some(r) = &p.registers {
            cfgs.enable(r.kind);
            cfgs.enable(format!("{}_{}", r.kind, r.version));

            // cfgs.enable(format!("peri_{}", p.name.to_ascii_lowercase()));
        }
    }

    // ========
    // Generate singletons

    let mut singletons: Vec<String> = Vec::new();
    for p in METADATA.peripherals {
        if let Some(r) = &p.registers {
            match r.kind {
                // Generate singletons per pin, not per port
                "xpi" => {}
                "sysctl" => {}
                // For other peripherals, one singleton per peri
                _ => singletons.push(p.name.to_string()),
            }
        }
    }

    // One singleton per DMA channel
    for c in METADATA.dma_channels {
        singletons.push(c.name.to_string());
    }

    // One singleton per IO Pin
    for p in METADATA.pins {
        singletons.push(p.name.to_string());
    }

    // ========
    // Write singletons

    let mut g = TokenStream::new();

    let singleton_tokens: Vec<_> = singletons.iter().map(|s| format_ident!("{}", s)).collect();

    g.extend(quote! {
        embassy_hal_internal::peripherals_definition!(#(#singleton_tokens),*);
    });

    let singleton_tokens: Vec<_> = singletons.iter().map(|s| format_ident!("{}", s)).collect();

    g.extend(quote! {
        embassy_hal_internal::peripherals_struct!(#(#singleton_tokens),*);
    });

    // ========
    // Generate interrupt declarations

    let mut irqs = Vec::new();
    for irq in METADATA.interrupts {
        irqs.push(format_ident!("{}", irq.name));
    }

    g.extend(quote! {
        crate::interrupt_mod!(
            #(
                #irqs,
            )*
        );
    });

    // ========
    // No need to handle FLASH regions
    // No need to handle clock settings

    // ========
    // Generate ClockPeripheral impls

    for p in METADATA.peripherals {
        if !singletons.contains(&p.name.to_string()) {
            continue;
        }
        let pname = format_ident!("{}", p.name);

        if let Some(sysctl) = &p.sysctl {
            //            if let Some(clock_idx) = sysctl.clock_node {
            let resource_idx = sysctl.resource;
            let clock_idx = sysctl.clock_node.unwrap_or(0xFFFFFFFF);
            g.extend(quote! {
                impl crate::sysctl::SealedClockPeripheral for peripherals::#pname {
                    const SYSCTL_CLOCK: usize = #clock_idx;
                    const SYSCTL_RESOURCE: usize = #resource_idx;
                }
                impl crate::sysctl::ClockPeripheral for peripherals::#pname {}
            });
        }
    }

    // ========
    // Generate pin_trait_impl!
    //    #[rustfmt::skip]
    let signals: HashMap<_, _> = [
        // (kind, signal) => trait
        (("uart", "TXD"), quote!(crate::uart::TxPin)),
        (("uart", "RXD"), quote!(crate::uart::RxPin)),
        (("uart", "CTS"), quote!(crate::uart::CtsPin)),
        (("uart", "RTS"), quote!(crate::uart::RtsPin)),
        (("uart", "DE"), quote!(crate::uart::DePin)),
        (("i2c", "SDA"), quote!(crate::i2c::SdaPin)),
        (("i2c", "SCL"), quote!(crate::i2c::SclPin)),
        (("spi", "SCLK"), quote!(crate::spi::SclkPin)),
        (("spi", "CS0"), quote!(crate::spi::CsPin)),
        (("spi", "CS1"), quote!(crate::spi::CsPin)),
        (("spi", "CS2"), quote!(crate::spi::CsPin)),
        (("spi", "CS3"), quote!(crate::spi::CsPin)),
        (("spi", "MOSI"), quote!(crate::spi::MosiPin)),
        (("spi", "MISO"), quote!(crate::spi::MisoPin)),
        (("spi", "DAT2"), quote!(crate::spi::D2Pin)),
        (("spi", "DAT3"), quote!(crate::spi::D3Pin)),
    ]
    .into();

    for p in METADATA.peripherals {
        if let Some(regs) = &p.registers {
            for pin in p.pins {
                let key = (regs.kind, pin.signal);
                if let Some(tr) = signals.get(&key) {
                    let peri = format_ident!("{}", p.name);

                    let pin_name = format_ident!("{}", pin.pin);

                    let alt = pin.alt.unwrap_or(0);

                    g.extend(quote! {
                        pin_trait_impl!(#tr, #peri, #pin_name, #alt);
                    })
                }

                // Spi is special
                if regs.kind == "spi" && pin.signal.starts_with("CS") {
                    let peri = format_ident!("{}", p.name);
                    let pin_name = format_ident!("{}", pin.pin);
                    let alt = pin.alt.unwrap_or(0);
                    let cs_index: u8 = match pin.signal {
                        "CS0" => 1,
                        "CS1" => 2,
                        "CS2" => 4,
                        "CS3" => 8,
                        // CSN pin is available on hpm67 chips
                        "CSN" => 0,
                        _ => unreachable!("CS pin not found {:?}", pin.signal),
                    };
                    g.extend(quote! {
                        spi_cs_pin_trait_impl!(crate::spi::CsIndexPin, #peri, #pin_name, #alt, #cs_index);
                    });
                }

                // ADC is special
                if regs.kind == "adc" {
                    // TODO
                }
                // if regs.kind == "dac"
            }
        }
    }

    // ========
    // Generate dma_trait_impl!
    let signals: HashMap<_, _> = [
        // (kind, signal) => trait
        (("uart", "RX"), quote!(crate::uart::RxDma)),
        (("uart", "TX"), quote!(crate::uart::TxDma)),
        (("i2c", "GLOBAL"), quote!(crate::i2c::I2cDma)),
        // FEMC
        (("femc", "A00"), quote!(crate::femc::A00Pin)),
        (("femc", "A01"), quote!(crate::femc::A01Pin)),
        (("femc", "A02"), quote!(crate::femc::A02Pin)),
        (("femc", "A03"), quote!(crate::femc::A03Pin)),
        (("femc", "A04"), quote!(crate::femc::A04Pin)),
        (("femc", "A05"), quote!(crate::femc::A05Pin)),
        (("femc", "A06"), quote!(crate::femc::A06Pin)),
        (("femc", "A07"), quote!(crate::femc::A07Pin)),
        (("femc", "A08"), quote!(crate::femc::A08Pin)),
        (("femc", "A09"), quote!(crate::femc::A09Pin)),
        (("femc", "A10"), quote!(crate::femc::A10Pin)),
        (("femc", "A11"), quote!(crate::femc::A11Pin)),
        (("femc", "A12"), quote!(crate::femc::A12Pin)),
        (("femc", "BA0"), quote!(crate::femc::BA0Pin)),
        (("femc", "BA1"), quote!(crate::femc::BA1Pin)),
        (("femc", "CAS"), quote!(crate::femc::CASPin)),
        (("femc", "CKE"), quote!(crate::femc::CKEPin)),
        (("femc", "CLK"), quote!(crate::femc::CLKPin)),
        (("femc", "CS0"), quote!(crate::femc::CS0Pin)),
        (("femc", "CS1"), quote!(crate::femc::CS1Pin)),
        (("femc", "DM0"), quote!(crate::femc::DM0Pin)),
        (("femc", "DM1"), quote!(crate::femc::DM1Pin)),
        (("femc", "DQS"), quote!(crate::femc::DQSPin)),
        (("femc", "DQ00"), quote!(crate::femc::DQ00Pin)),
        (("femc", "DQ01"), quote!(crate::femc::DQ01Pin)),
        (("femc", "DQ02"), quote!(crate::femc::DQ02Pin)),
        (("femc", "DQ03"), quote!(crate::femc::DQ03Pin)),
        (("femc", "DQ04"), quote!(crate::femc::DQ04Pin)),
        (("femc", "DQ05"), quote!(crate::femc::DQ05Pin)),
        (("femc", "DQ06"), quote!(crate::femc::DQ06Pin)),
        (("femc", "DQ07"), quote!(crate::femc::DQ07Pin)),
        (("femc", "DQ08"), quote!(crate::femc::DQ08Pin)),
        (("femc", "DQ09"), quote!(crate::femc::DQ09Pin)),
        (("femc", "DQ10"), quote!(crate::femc::DQ10Pin)),
        (("femc", "DQ11"), quote!(crate::femc::DQ11Pin)),
        (("femc", "DQ12"), quote!(crate::femc::DQ12Pin)),
        (("femc", "DQ13"), quote!(crate::femc::DQ13Pin)),
        (("femc", "DQ14"), quote!(crate::femc::DQ14Pin)),
        (("femc", "DQ15"), quote!(crate::femc::DQ15Pin)),
        (("femc", "DQ16"), quote!(crate::femc::DQ16Pin)),
        (("femc", "DQ17"), quote!(crate::femc::DQ17Pin)),
        (("femc", "DQ18"), quote!(crate::femc::DQ18Pin)),
        (("femc", "DQ19"), quote!(crate::femc::DQ19Pin)),
        (("femc", "DQ20"), quote!(crate::femc::DQ20Pin)),
        (("femc", "DQ21"), quote!(crate::femc::DQ21Pin)),
        (("femc", "DQ22"), quote!(crate::femc::DQ22Pin)),
        (("femc", "DQ23"), quote!(crate::femc::DQ23Pin)),
        (("femc", "DQ24"), quote!(crate::femc::DQ24Pin)),
        (("femc", "DQ25"), quote!(crate::femc::DQ25Pin)),
        (("femc", "DQ26"), quote!(crate::femc::DQ26Pin)),
        (("femc", "DQ27"), quote!(crate::femc::DQ27Pin)),
        (("femc", "DQ28"), quote!(crate::femc::DQ28Pin)),
        (("femc", "DQ29"), quote!(crate::femc::DQ29Pin)),
        (("femc", "DQ30"), quote!(crate::femc::DQ30Pin)),
        (("femc", "DQ31"), quote!(crate::femc::DQ31Pin)),
        (("femc", "RAS"), quote!(crate::femc::RASPin)),
        (("femc", "WE"), quote!(crate::femc::WEPin)),
    ]
    .into();

    for p in METADATA.peripherals {
        if let Some(regs) = &p.registers {
            let mut dupe = HashSet::new();
            for ch in p.dma_channels {
                if let Some(tr) = signals.get(&(regs.kind, ch.signal)) {
                    let peri = format_ident!("{}", p.name);

                    let key = (ch.signal, ch.request.unwrap().to_string());
                    if !dupe.insert(key) {
                        continue;
                    }

                    // request number for peripheral DMA
                    let request = ch.request.expect("DMA request number must be specified") as u8;

                    g.extend(quote! {
                        dma_trait_impl!(#tr, #peri, #request);
                    });
                }
            }
        }
    }

    // ========
    // Write foreach_foo! macrotables

    let mut interrupts_table: Vec<Vec<String>> = Vec::new();
    let mut peripherals_table: Vec<Vec<String>> = Vec::new();
    let mut pins_table: Vec<Vec<String>> = Vec::new();

    // pin name => io pad index
    for p in METADATA.pins {
        pins_table.push(vec![p.name.to_string(), p.index.to_string()]);
    }

    let mut dmas = TokenStream::new();
    for ch in METADATA.dma_channels.iter() {
        let name = format_ident!("{}", ch.name);
        let idx = ch.channel as u8;

        let ch_num = ch.channel as usize;
        let mux_num = ch.dmamux_channel as usize;

        // HDMA or XDMA
        let dma_name = format_ident!("{}", ch.name.split_once('_').expect("DMA channel name format").0);

        g.extend(quote!(dma_channel_impl!(#name, #idx);));

        dmas.extend(quote! {
            crate::dma::ChannelInfo {
                dma: crate::dma::DmaInfo::#dma_name(crate::pac::#dma_name),
                num: #ch_num,
                mux_num: #mux_num,
            },
        });
    }

    // ========
    // Generate DMA IRQs.
    let mut dma_irqs: BTreeMap<&str, &str> = BTreeMap::new();

    for p in METADATA.peripherals {
        if let Some(r) = &p.registers {
            if r.kind == "dma" {
                for irq in p.interrupts {
                    // only 1 global DMA interrupt per controller
                    assert_eq!(irq.signal, "GLOBAL");
                    dma_irqs.insert(irq.interrupt, p.name);
                }
            }
        }
    }

    let dma_irqs: TokenStream = dma_irqs
        .iter()
        .map(|(irq, peri_name)| {
            let irq = format_ident!("{}", irq);
            let peri_name = format_ident!("{}", peri_name);

            quote! {
                #[cfg(feature = "rt")]
                #[allow(non_snake_case)]
                #[no_mangle]
                unsafe extern "riscv-interrupt-m" fn #irq() {
                    <crate::peripherals::#peri_name as crate::dma::ControllerInterrupt>::on_irq();
                }
            }
        })
        .collect();

    g.extend(dma_irqs);

    g.extend(quote! {
        pub(crate) const DMA_CHANNELS: &[crate::dma::ChannelInfo] = &[#dmas];
    });

    for p in METADATA.peripherals {
        let Some(regs) = &p.registers else {
            continue;
        };

        for irq in p.interrupts {
            let row = vec![
                p.name.to_string(),
                regs.kind.to_string(),
                regs.block.to_string(),
                irq.signal.to_string(),
                irq.interrupt.to_ascii_uppercase(),
            ];
            interrupts_table.push(row)
        }

        let row = vec![regs.kind.to_string(), p.name.to_string()];
        peripherals_table.push(row);
    }

    /*
    for irq in METADATA.interrupts {
        let name = irq.name.to_ascii_uppercase();
        interrupts_table.push(vec![name.clone()]);
    }
    */

    let mut m = String::new();

    make_table(&mut m, "foreach_interrupt", &interrupts_table);
    make_table(&mut m, "foreach_peripheral", &peripherals_table);
    make_table(&mut m, "foreach_pin", &pins_table);

    let out_dir = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let out_file = out_dir.join("_macros.rs").to_string_lossy().to_string();
    fs::write(&out_file, m).unwrap();
    rustfmt(&out_file);

    // ========
    // Write generated.rs

    let out_file = out_dir.join("_generated.rs").to_string_lossy().to_string();
    fs::write(&out_file, g.to_string()).unwrap();
    rustfmt(&out_file);
}

fn make_table(out: &mut String, name: &str, data: &Vec<Vec<String>>) {
    write!(
        out,
        "#[allow(unused)]
macro_rules! {} {{
    ($($pat:tt => $code:tt;)*) => {{
        macro_rules! __{}_inner {{
            $(($pat) => $code;)*
            ($_:tt) => {{}}
        }}
",
        name, name
    )
    .unwrap();

    for row in data {
        writeln!(out, "        __{}_inner!(({}));", name, row.join(",")).unwrap();
    }

    write!(
        out,
        "    }};
}}"
    )
    .unwrap();
}

enum GetOneError {
    None,
    Multiple,
}

trait IteratorExt: Iterator {
    fn get_one(self) -> Result<Self::Item, GetOneError>;
}

impl<T: Iterator> IteratorExt for T {
    fn get_one(mut self) -> Result<Self::Item, GetOneError> {
        match self.next() {
            None => Err(GetOneError::None),
            Some(res) => match self.next() {
                Some(_) => Err(GetOneError::Multiple),
                None => Ok(res),
            },
        }
    }
}

/// Helper for emitting cargo instruction for enabling configs (`cargo:rustc-cfg=X`) and declaring
/// them (`cargo:rust-check-cfg=cfg(X)`).
#[derive(Debug)]
pub struct CfgSet {
    enabled: std::collections::HashSet<String>,
    declared: std::collections::HashSet<String>,
    emit_declared: bool,
}

impl CfgSet {
    pub fn new() -> Self {
        Self {
            enabled: std::collections::HashSet::new(),
            declared: std::collections::HashSet::new(),
            emit_declared: is_rustc_nightly(),
        }
    }

    /// Enable a config, which can then be used in `#[cfg(...)]` for conditional compilation.
    ///
    /// All configs that can potentially be enabled should be unconditionally declared using
    /// [`Self::declare()`].
    pub fn enable(&mut self, cfg: impl AsRef<str>) {
        if self.enabled.insert(cfg.as_ref().to_owned()) {
            println!("cargo:rustc-cfg={}", cfg.as_ref());
        }
    }

    pub fn enable_all(&mut self, cfgs: &[impl AsRef<str>]) {
        for cfg in cfgs.iter() {
            self.enable(cfg.as_ref());
        }
    }

    /// Declare a valid config for conditional compilation, without enabling it.
    ///
    /// This enables rustc to check that the configs in `#[cfg(...)]` attributes are valid.
    pub fn declare(&mut self, cfg: impl AsRef<str>) {
        if self.declared.insert(cfg.as_ref().to_owned()) && self.emit_declared {
            println!("cargo:rustc-check-cfg=cfg({})", cfg.as_ref());
        }
    }

    pub fn declare_all(&mut self, cfgs: &[impl AsRef<str>]) {
        for cfg in cfgs.iter() {
            self.declare(cfg.as_ref());
        }
    }

    pub fn set(&mut self, cfg: impl Into<String>, enable: bool) {
        let cfg = cfg.into();
        if enable {
            self.enable(cfg.clone());
        }
        self.declare(cfg);
    }
}

fn is_rustc_nightly() -> bool {
    if env::var_os("EMBASSY_FORCE_CHECK_CFG").is_some() {
        return true;
    }

    let rustc = env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));

    let output = Command::new(rustc)
        .arg("--version")
        .output()
        .expect("failed to run `rustc --version`");

    String::from_utf8_lossy(&output.stdout).contains("nightly")
}

/// rustfmt a given path.
/// Failures are logged to stderr and ignored.
fn rustfmt(path: impl AsRef<Path>) {
    let path = path.as_ref();
    match Command::new("rustfmt").args([path]).output() {
        Err(e) => {
            eprintln!("failed to exec rustfmt {:?}: {:?}", path, e);
        }
        Ok(out) => {
            if !out.status.success() {
                eprintln!("rustfmt {:?} failed:", path);
                eprintln!("=== STDOUT:");
                std::io::stderr().write_all(&out.stdout).unwrap();
                eprintln!("=== STDERR:");
                std::io::stderr().write_all(&out.stderr).unwrap();
            }
        }
    }
}
