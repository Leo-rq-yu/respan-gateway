//! Meta's Llama API: Chat Completions and images, as OpenAI serves them.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_media_http::openai::OpenAiImages;

use respan_gateway_vendor_kit::Vendor;

/// Where its OpenAI-compatible API is.
const BASE_URL: &str = "https://api.llama.com/compat/v1";

/// Meta. Its images take a size as an aspect ratio alone, and refuse a
/// mask; it says so itself.
pub const VENDOR: Vendor = Vendor {
    id: "meta",
    key_env: &["LLAMA_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&OpenAiImages, Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};
