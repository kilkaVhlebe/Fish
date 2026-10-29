//! Example mod: a standard key exported as an object and a custom key exported as `init`.
//! Used to test that the loader checks entrypoint kinds against the manifest.

static API: u32 = 1;

fn api() -> &'static u32 {
    &API
}

fn setup() {}

fish_api::export_mod! {
    main: object [api],
    "kinds:custom": [setup],
}
