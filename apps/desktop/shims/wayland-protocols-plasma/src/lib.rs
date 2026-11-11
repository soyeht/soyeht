//! The part of `wayland-protocols-plasma` that GPUI's Linux backend uses: the blur module of the
//! client side, and only the names `gpui_linux` imports.
//!
//! The registry crate carries KDE's protocol files, 17 of them under LGPL-2.1-or-later, and Soyeht
//! is Apache-2.0 (ADR 0149). Soyeht never asks for a blurred window, so this crate replaces it, by
//! `[patch.crates-io]` in `apps/desktop/Cargo.toml`, with a protocol written here (in `protocol/`)
//! from the calls GPUI makes. Its interfaces carry names that no compositor advertises: GPUI looks
//! for the manager global, finds none, and blur stays off.

pub mod blur {
    pub mod client {
        mod generated {
            #![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
            #![allow(non_upper_case_globals, non_snake_case, unused_imports)]
            #![allow(missing_docs, clippy::all)]

            use wayland_client;
            use wayland_client::protocol::*;

            pub mod __interfaces {
                use wayland_client::protocol::__interfaces::*;
                wayland_scanner::generate_interfaces!("protocol/unadvertised-blur.xml");
            }
            use self::__interfaces::*;

            wayland_scanner::generate_client_code!("protocol/unadvertised-blur.xml");
        }

        /// The blur object of one surface, under the module and type names GPUI imports.
        pub mod org_kde_kwin_blur {
            pub use super::generated::soyeht_unadvertised_blur::SoyehtUnadvertisedBlur as OrgKdeKwinBlur;
        }

        /// The manager of blur objects, under the module and type names GPUI imports.
        pub mod org_kde_kwin_blur_manager {
            pub use super::generated::soyeht_unadvertised_blur_manager::SoyehtUnadvertisedBlurManager as OrgKdeKwinBlurManager;
        }
    }
}
