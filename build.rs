use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use hpm_metapac::metadata::METADATA;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

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
    cfgs.enable(family_name);

    for p in METADATA.peripherals {
        if let Some(r) = &p.registers {
            cfgs.enable(r.kind);
            cfgs.enable(format!("{}_{}", r.kind, r.version));
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
            if let Some(clock_idx) = sysctl.clock_node {
                g.extend(quote! {
                    impl crate::sysctl::SealedClockPeripheral for peripherals::#pname {
                        const SYSCTL_CLOCK: usize = #clock_idx;
                    }
                    impl crate::sysctl::ClockPeripheral for peripherals::#pname {}
                });
            }
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
        //(("spi", "MISO"), quote!(crate::spi::MisoPin)),
        //(("spi", "MOSI"), quote!(crate::spi::MosiPin)),
        //(("spi", "SCK"), quote!(crate::spi::SckPin)),
        //(("spi", "CS0"), quote!(crate::spi::Cs0Pin)),
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
                    let request = ch.request.expect("DMA request must be specified") as u8;

                    // let channel = format_ident!("{}", ch.name);

                    for channel in METADATA.dma_channels {
                        let channel_ident = format_ident!("{}", channel.name);
                        g.extend(quote! {
                            dma_trait_impl!(#tr, #peri, #channel_ident, #request);
                        });
                    }
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
