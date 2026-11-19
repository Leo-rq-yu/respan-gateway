//! Baseten: decisions as System One, at its Model APIs.

use respan_gateway_decision_http::SystemOne;

use respan_gateway_vendor_kit::Vendor;

/// Where its Model APIs are.
const BASE_URL: &str = "https://inference.baseten.co/v1";

/// Baseten. Its decision models are named as it lists them, such as
/// `decision-models/baseten-elephant-9b-latest`; its `service_tier` is a
/// System One client's to send.
pub const VENDOR: Vendor = Vendor {
    id: "baseten",
    key_env: &["BASETEN_API_KEY"],
    formats: &[(&SystemOne, Some(BASE_URL))],
    ..Vendor::DEFAULT
};
