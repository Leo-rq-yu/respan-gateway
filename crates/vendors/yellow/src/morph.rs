//! Morph: decisions as System One, as TypeSafe serves it.

use respan_gateway_decision_http::SystemOne;

use respan_gateway_vendor_kit::Vendor;

/// Where its API is.
const BASE_URL: &str = "https://api.morphllm.com/v1";

/// Morph.
pub const VENDOR: Vendor = Vendor {
    id: "morph",
    key_env: &["MORPH_API_KEY"],
    formats: &[(&SystemOne, Some(BASE_URL))],
    ..Vendor::DEFAULT
};
