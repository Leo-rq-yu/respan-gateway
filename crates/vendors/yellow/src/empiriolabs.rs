//! EmpirioLabs AI: Chat Completions, Responses, Messages and embeddings, as
//! OpenAI and Anthropic serve them, and its own video API.

use respan_gateway_conversation_http::anthropic::AnthropicMessages;
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_conversation_http::responses::OpenAiResponses;
use respan_gateway_media_http::empiriolabs::EmpirioLabsVideos;
use respan_gateway_media_http::openai::OpenAiEmbeddings;

use serde_json::Value;

use respan_gateway_vendor_kit::{
    Body, BoxFuture, Model, Models, Vendor, VendorProvider, built_wrong, http, json, models,
};

/// Where its API is.
const BASE_URL: &str = "https://api.empiriolabs.ai/v1";

/// Where its Messages API is: the format adds the `/v1`.
const MESSAGES_URL: &str = "https://api.empiriolabs.ai";

/// EmpirioLabs AI.
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
    models: list_models,
    ..Vendor::DEFAULT
};

/// Its models: `GET /models` as OpenAI lists them, each with a `category`
/// and `type` that say its kind.
fn list_models(p: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async move {
        let call = http::Request::get("/models").body(Body::default());
        let (answer, _) = p.client()?.send(call.map_err(built_wrong)?).await?;
        Ok(listed(&json(answer).await?))
    })
}

/// The models of its list, each of the kind it says. A category the gateway
/// has no kind for is its own, which no format serves.
fn listed(answer: &Value) -> Vec<Model> {
    let entries = answer["data"].as_array().into_iter().flatten();
    let model = |m: &Value| {
        let mut model = models::listed(&Value::Array(vec![m.clone()])).pop()?;
        let (category, kind) = (m["category"].as_str()?, m["type"].as_str());
        let kind = match (category, kind) {
            ("text", _) => "chat",
            ("audio", Some("tts")) => "speech",
            ("audio" | "transcription", Some("realtime")) => "realtime",
            ("transcription", _) => "transcription",
            (category, _) => category,
        };
        model.kind = Some(kind.to_owned());
        Some(model)
    };
    entries.filter_map(model).collect()
}
