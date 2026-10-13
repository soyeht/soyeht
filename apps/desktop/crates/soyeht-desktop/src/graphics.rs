//! What the application says about the graphics stack, so that a machine without a graphics
//! card (a virtual machine, a remote session) is told what to install instead of being left
//! with a window that never appears.
//!
//! GPUI draws through Vulkan, or OpenGL when there is no Vulkan driver. Without a card, Mesa's
//! software renderer (llvmpipe, in the Vulkan driver package) does the drawing. It is slow, and
//! it compiles every shader on the CPU through LLVM, so a Mesa and LLVM pair with a code
//! generation bug takes the whole process down on the first frame (Fedora 44 on arm64 with
//! Mesa 26.0.3 and LLVM 22.1.8 does; Mesa 26.2.4 does not).

use gpui::GpuSpecs;
use std::fmt::Display;

/// The packages that provide the Vulkan driver, with Mesa's software renderer, per distribution.
const DRIVER_PACKAGES: &str = "mesa-vulkan-drivers on Fedora and Ubuntu, vulkan-swrast on Arch";

/// The line printed when the window cannot be opened.
pub(crate) fn open_failure(error: &dyn Display, linux: bool) -> String {
    let mut text = format!("soyeht: the window could not be opened: {error}");
    if linux {
        text.push_str(&format!(
            "\nsoyeht: the window needs a Vulkan or OpenGL driver and a Wayland or X11 session. \
             Install the driver ({DRIVER_PACKAGES}) and update Mesa, then start again."
        ));
    }
    text
}

/// The line printed once the window exists, when it draws on the processor. `None` on a card.
pub(crate) fn software_note(specs: Option<&GpuSpecs>) -> Option<String> {
    let specs = specs.filter(|specs| specs.is_software_emulated)?;
    Some(format!(
        "soyeht: drawing on a software renderer: {}, {}. It is slow, and an old Mesa can crash on \
         it: keep the Mesa driver up to date ({DRIVER_PACKAGES}).",
        specs.device_name, specs.driver_info
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs(software: bool) -> GpuSpecs {
        GpuSpecs {
            is_software_emulated: software,
            device_name: "llvmpipe (LLVM 22.1.8, 128 bits)".into(),
            driver_name: "llvmpipe".into(),
            driver_info: "Mesa 26.0.3 (LLVM 22.1.8)".into(),
        }
    }

    #[test]
    fn a_card_prints_nothing() {
        assert_eq!(software_note(Some(&specs(false))), None);
        assert_eq!(software_note(None), None);
    }

    #[test]
    fn a_software_renderer_names_itself_and_the_package() {
        let note = software_note(Some(&specs(true))).expect("a note");
        assert!(note.contains("llvmpipe (LLVM 22.1.8, 128 bits)"));
        assert!(note.contains("Mesa 26.0.3 (LLVM 22.1.8)"));
        assert!(note.contains("mesa-vulkan-drivers"));
    }

    #[test]
    fn only_linux_gets_the_driver_advice() {
        let linux = open_failure(&"No GPU adapters found", true);
        assert!(linux.starts_with("soyeht: the window could not be opened: No GPU adapters found"));
        assert!(linux.contains("mesa-vulkan-drivers"));
        let other = open_failure(&"No GPU adapters found", false);
        assert!(!other.contains("Vulkan"));
    }
}
