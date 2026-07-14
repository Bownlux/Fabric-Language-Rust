//! Example fabric-language-rust mod.
//!
//! Packaged by the `:example` Gradle subproject as `natives/<platform>/` inside
//! the `flr-example` jar, whose `fabric.mod.json` declares
//! `{"adapter": "rust", "value": "example_mod::init"}` for the `main`
//! entrypoint.

use fabric_rust::prelude::*;

fn init() {
    info!("Hello from Rust! \u{1F980}");
}

fabric_rust::register_entrypoints! {
    "init" => init,
}
