//! DeepInfra: Chat Completions, images and embeddings, as OpenAI serves
//! them.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_media_http::openai::{OpenAiEmbeddings, OpenAiImages};

use respan_gateway_vendor_kit::Vendor;

/// Where its OpenAI-compatible API is.
const BASE_URL: &str = "https://api.deepinfra.com/v1/openai";

/// DeepInfra. An edit is OpenAI's form, as it takes one.
pub const VENDOR: Vendor = Vendor {
    id: "deepinfra",
    key_env: &["DEEPINFRA_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&OpenAiImages, Some(BASE_URL)),
        (&OpenAiEmbeddings, Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};
