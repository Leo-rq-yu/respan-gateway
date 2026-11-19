//! Qwen, on Alibaba Cloud Model Studio's international site: Chat
//! Completions and embeddings as OpenAI serves them, and images as its own
//! API, DashScope, serves them.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_media_http::dashscope::DashScopeImages;
use respan_gateway_media_http::openai::OpenAiEmbeddings;

use respan_gateway_vendor_kit::Vendor;

/// Where its OpenAI-compatible API is.
const COMPATIBLE: &str = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1";

/// Where its own API is.
const NATIVE: &str = "https://dashscope-intl.aliyuncs.com/api/v1";

/// Qwen. Its OpenAI-compatible API draws only some of its image models, at
/// a workspace's own host: every one is served by its own API instead
/// (`dashscope-images`).
pub const VENDOR: Vendor = Vendor {
    id: "qwen",
    key_env: &["DASHSCOPE_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(COMPATIBLE)),
        (&OpenAiEmbeddings, Some(COMPATIBLE)),
        (&DashScopeImages, Some(NATIVE)),
    ],
    ..Vendor::DEFAULT
};
