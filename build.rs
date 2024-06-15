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

    // TODO: interrupt mod

    // ========
    // Write foreach_foo! macrotables

    //let mut flash_regions_table: Vec<Vec<String>> = Vec::new();
    //let mut interrupts_table: Vec<Vec<String>> = Vec::new();
    //let mut peripherals_table: Vec<Vec<String>> = Vec::new();
    let mut pins_table: Vec<Vec<String>> = Vec::new();

    // pin name => io pad index
    for p in METADATA.pins {
        pins_table.push(vec![p.name.to_string(), p.index.to_string()]);
    }

    let mut m = String::new();

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
