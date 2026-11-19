//! Concentrate: decisions as OpenAI serves them, and as System One.

use respan_gateway_decision_http::SystemOne;
use respan_gateway_decision_http::openai::OpenAiDecisions;

use respan_gateway_vendor_kit::Vendor;

/// Where its API is.
const BASE_URL: &str = "https://api.concentrate.ai/v1";

/// Concentrate. Its Decisions come first, so a client of another contract is
/// translated to them; a System One client is sent to its System One as it
/// is, which alone asks a `box`.
pub const VENDOR: Vendor = Vendor {
    id: "concentrate",
    key_env: &["CONCENTRATE_API_KEY"],
    formats: &[
        (&OpenAiDecisions::new(), Some(BASE_URL)),
        (&SystemOne, Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};
