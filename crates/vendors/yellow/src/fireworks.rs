//! Fireworks: Chat Completions, embeddings and decisions, as OpenAI serves
//! them, and decisions as System One too. Its decision models read no
//! images: one is its 400.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_decision_http::SystemOne;
use respan_gateway_decision_http::openai::OpenAiDecisions;
use respan_gateway_media_http::openai::OpenAiEmbeddings;

use respan_gateway_vendor_kit::Vendor;

/// Where its API is.
const BASE_URL: &str = "https://api.fireworks.ai/inference/v1";

/// Fireworks.
pub const VENDOR: Vendor = Vendor {
    id: "fireworks",
    key_env: &["FIREWORKS_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&OpenAiEmbeddings, Some(BASE_URL)),
        (&SystemOne, Some(BASE_URL)),
        (&OpenAiDecisions::new(), Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};
