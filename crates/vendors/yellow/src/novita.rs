//! Novita AI: Chat Completions and images, as OpenAI serves them.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_media_http::openai::OpenAiImages;

use respan_gateway_vendor_kit::Vendor;

/// Where its OpenAI-compatible API is.
const BASE_URL: &str = "https://api.novita.ai/openai/v1";

/// Novita AI. Only some of its image models are served at its
/// OpenAI-compatible API; the rest are its own API's tasks, which no format
/// here speaks.
pub const VENDOR: Vendor = Vendor {
    id: "novita",
    key_env: &["NOVITA_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&OpenAiImages, Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};
