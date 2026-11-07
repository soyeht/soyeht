//! The names and calls that `gpui_linux` makes on the blur module, and the promise that blur stays off.

use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::globals::GlobalList;
use wayland_client::{delegate_noop, Dispatch, Proxy, QueueHandle};
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur::OrgKdeKwinBlur;
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur_manager::OrgKdeKwinBlurManager;

/// What `gpui_linux` does with a blur manager and a blur object (window.rs, client.rs): it compiles
/// only while the stand-in has these methods with these signatures.
#[allow(dead_code)]
fn calls_gpui_makes<D>(manager: &OrgKdeKwinBlurManager, surface: &WlSurface, queue: &QueueHandle<D>)
where
    D: Dispatch<OrgKdeKwinBlur, ()> + 'static,
{
    let blur = manager.create(surface, queue, ());
    blur.commit();
    manager.unset(surface);
    blur.release();
}

struct State;

// How `gpui_linux` ignores the events of both (client.rs), and how it looks for the manager.
delegate_noop!(State: ignore OrgKdeKwinBlurManager);
delegate_noop!(State: ignore OrgKdeKwinBlur);

#[allow(dead_code)]
fn bind_manager(globals: &GlobalList, queue: &QueueHandle<State>) -> Option<OrgKdeKwinBlurManager> {
    globals.bind(queue, 1..=1, ()).ok()
}

#[test]
fn no_compositor_advertises_the_manager() {
    let name = OrgKdeKwinBlurManager::interface().name;
    assert_eq!(name, "soyeht_unadvertised_blur_manager");
    assert!(!name.contains("kwin"), "a compositor that offered the KDE name would be bound");
    assert_eq!(OrgKdeKwinBlur::interface().name, "soyeht_unadvertised_blur");
}
