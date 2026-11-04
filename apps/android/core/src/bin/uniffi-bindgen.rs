//! Writes the Kotlin bindings of the core from the built library (`scripts/check_android.sh`).

fn main() {
    uniffi::uniffi_bindgen_main();
}
