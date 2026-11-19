//! Perplexity: decisions as System One, at `/decisions`.

use respan_gateway_decision_http::{At, DialectedSystemOne};

use respan_gateway_vendor_kit::Vendor;

/// Where its decisions are.
const BASE_URL: &str = "https://api.perplexity.ai/v1";

/// Perplexity. Its `locate` questions and web grounding (`mode`,
/// `max_staleness`) are its own: a System One client's fields are sent as
/// they are, and one it does not know is its 400.
pub const VENDOR: Vendor = Vendor {
    id: "perplexity",
    key_env: &["PERPLEXITY_API_KEY"],
    formats: &[(&DialectedSystemOne::new(At("/decisions")), Some(BASE_URL))],
    ..Vendor::DEFAULT
};
