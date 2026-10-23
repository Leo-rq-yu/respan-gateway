//! Amazon Bedrock: Cohere Embed, Titan Embeddings and Stability AI images on
//! InvokeModel, at a region's runtime, with a Bedrock API key.

use respan_gateway_media_http::bedrock::{BedrockEmbeddings, BedrockImages};
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
        (&BedrockEmbeddings, Some(BASE_URL)),
        (&BedrockImages, Some(BASE_URL)),
    ],
    models: list_models,
    ..Vendor::DEFAULT
};

/// Its embedding and Stability AI image models: the region's control plane,
/// its first format's host without `-runtime`, lists them.
fn list_models(p: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async move {
        let call = http::Request::get("/foundation-models").body(Body::default());
        let parts = p.parts()?;
        let base = parts
            .first()
            .map_or(BASE_URL, |part| part.base_url.as_str());
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

/// The kind of a listed model that a format here serves: embeddings by
/// their output, images only of Stability AI's.
fn kind_of(m: &Value) -> Option<&'static str> {
    let outputs = m["outputModalities"].as_array()?;
    let has = |name: &str| outputs.iter().any(|o| o.as_str() == Some(name));
    if has("EMBEDDING") {
        Some("embedding")
    } else if has("IMAGE") && m["providerName"].as_str() == Some("Stability AI") {
        Some("image")
    } else {
        None
    }
}
