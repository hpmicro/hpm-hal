//! Family dispatch for the HPM6E/HPM5E SYSCTL register generation.

#[cfg(hpm5e)]
pub(super) use super::Pll;
pub(super) use super::clock_add_to_group;

#[cfg(hpm5e)]
#[path = "v6e_hpm5e.rs"]
mod family;
#[cfg(hpm6e)]
#[path = "v6e_hpm6e.rs"]
mod family;

pub use family::*;
