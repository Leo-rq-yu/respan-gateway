//! Amazon Bedrock: chat on Converse, and Cohere Embed and Titan Embeddings
//! on InvokeModel, at a region's runtime, with a Bedrock API key.

use respan_gateway_conversation_http::bedrock::BedrockConverse;
use respan_gateway_media_http::bedrock::BedrockEmbeddings;
use serde_json::Value;

use respan_gateway_vendor_kit::{
    Body, BoxFuture, Model, Models, Vendor, VendorProvider, built_wrong, http, json,
};

/// Its region's runtime.
const BASE_URL: &str = "https://bedrock-runtime.{AWS_REGION}.amazonaws.com";

/// Amazon Bedrock.
pub const VENDOR: Vendor = Vendor {
    id: "bedrock",
    key_env: &["AWS_BEARER_TOKEN_BEDROCK"],
    formats: &[
        (&BedrockConverse, Some(BASE_URL)),
        (&BedrockEmbeddings, Some(BASE_URL)),
    ],
    models: list_models,
    ..Vendor::DEFAULT
};

/// Its models: the region's control plane, its runtime's host without
/// `-runtime`, lists them.
fn list_models(p: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async move {
        let call = http::Request::get("/foundation-models").body(Body::default());
        let base = p
            .base_url_of(BedrockConverse.name())
            .or_else(|| p.base_url_of(BedrockEmbeddings::NAME))
            .unwrap_or(BASE_URL);
        let control = p.client_at(&base.replacen("bedrock-runtime.", "bedrock.", 1))?;
        let (answer, _) = control.send(call.map_err(built_wrong)?).await?;
        let answer = json(answer).await?;
        let summaries = answer["modelSummaries"].as_array().into_iter().flatten();
        let model = |m: &Value| {
            let mut model = Model::new(m["modelId"].as_str()?);
            model.display_name = m["modelName"].as_str().map(str::to_owned);
            model.kind = Some(kind_of(m)?.to_owned());
            Some(model)
        };
        Ok(summaries.filter_map(model).collect())
    })
}

/// The kind of a model summary: an embedding model embeds; one that answers
/// in text and is called by its own id (not only through an inference
/// profile) chats.
fn kind_of(m: &Value) -> Option<&'static str> {
    let has = |key: &str, name: &str| {
        let names = m[key].as_array().into_iter().flatten();
        names.into_iter().any(|n| n == name)
    };
    if has("outputModalities", "EMBEDDING") {
        Some("embedding")
    } else if has("outputModalities", "TEXT") && has("inferenceTypes", "ON_DEMAND") {
        Some("chat")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_model_is_an_embedding_or_a_chat_by_what_it_outputs_and_how_it_is_called() {
        let kind = |outputs: Value, types: Value| {
            kind_of(&json!({ "outputModalities": outputs, "inferenceTypes": types }))
        };
        assert_eq!(
            kind(json!(["EMBEDDING"]), json!(["ON_DEMAND"])),
            Some("embedding")
        );
        assert_eq!(
            kind(json!(["TEXT"]), json!(["ON_DEMAND", "PROVISIONED"])),
            Some("chat")
        );
        assert_eq!(kind(json!(["TEXT"]), json!(["INFERENCE_PROFILE"])), None);
        assert_eq!(kind(json!(["IMAGE"]), json!(["ON_DEMAND"])), None);
    }
}
