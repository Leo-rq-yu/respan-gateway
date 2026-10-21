//! EmpirioLabs: Chat Completions, Responses, Messages and embeddings, as
//! OpenAI and Anthropic serve them, and its own video API.

use respan_gateway_conversation_http::anthropic::AnthropicMessages;
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_conversation_http::responses::OpenAiResponses;
use respan_gateway_media_http::empiriolabs::EmpirioLabsVideos;
use respan_gateway_media_http::openai::OpenAiEmbeddings;

use respan_gateway_vendor_kit::Vendor;

/// Where its API is.
const BASE_URL: &str = "https://api.empiriolabs.ai/v1";

/// Where its Messages API is: the format adds the `/v1`.
const MESSAGES_URL: &str = "https://api.empiriolabs.ai";

/// EmpirioLabs.
pub const VENDOR: Vendor = Vendor {
    id: "empiriolabs",
    key_env: &["EMPIRIOLABS_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&OpenAiResponses, Some(BASE_URL)),
        (&AnthropicMessages, Some(MESSAGES_URL)),
        (&OpenAiEmbeddings, Some(BASE_URL)),
        (&EmpirioLabsVideos, Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};
