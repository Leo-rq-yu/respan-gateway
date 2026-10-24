//! DeepInfra: Chat Completions, images and embeddings, as OpenAI serves them.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_media_http::openai::{OpenAiEmbeddings, OpenAiImages};

use serde_json::Value;

use respan_gateway_vendor_kit::{
    Body, BoxFuture, Model, Models, Vendor, VendorProvider, built_wrong, http, json, models,
};

/// Where its API is.
const BASE_URL: &str = "https://api.deepinfra.com/v1/openai";

/// DeepInfra.
pub const VENDOR: Vendor = Vendor {
    id: "deepinfra",
    key_env: &["DEEPINFRA_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&OpenAiImages, Some(BASE_URL)),
        (&OpenAiEmbeddings, Some(BASE_URL)),
    ],
    models: list_models,
    ..Vendor::DEFAULT
};

/// The kind each of its tags says a model is, the first it has.
const KINDS: &[(&str, &str)] = &[
    ("chat", "chat"),
    ("embed", "embedding"),
    ("image-gen", "image"),
    ("video-gen", "video"),
    ("tts", "speech"),
    ("stt", "transcription"),
];

/// Its models: `GET /models` as OpenAI lists them, each tagged in its
/// `metadata` with what it does.
fn list_models(p: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async move {
        let call = http::Request::get("/models").body(Body::default());
        let (answer, _) = p.client()?.send(call.map_err(built_wrong)?).await?;
        Ok(listed(&json(answer).await?))
    })
}

/// The models of its list, each of the kind its tags say, else of its
/// name's.
fn listed(answer: &Value) -> Vec<Model> {
    let entries = answer["data"].as_array().into_iter().flatten();
    let model = |m: &Value| {
        let mut model = models::listed(&Value::Array(vec![m.clone()])).pop()?;
        let tags = m["metadata"]["tags"].as_array();
        let has = |tag: &str| tags.is_some_and(|t| t.iter().any(|t| t == tag));
        let kind = KINDS.iter().find(|(tag, _)| has(tag));
        model.kind = kind.map(|(_, kind)| (*kind).to_owned());
        Some(model)
    };
    entries.filter_map(model).collect()
}
