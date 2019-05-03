//! # TCW3 — Cross-platform GUI toolkit
//!
//! # Details
//!
//!  - **Color management**: Color values are specified in the sRGB color space,
//!    unless otherwise specified.
//!
#![feature(weak_ptr_eq)]
#![feature(weak_counts)]
pub mod pal;
pub mod ui;
pub mod uicore;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
