//! Amazon Bedrock: Cohere Embed and Titan Embeddings on InvokeModel, at a
//! region's runtime, with a Bedrock API key.

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
    formats: &[(&BedrockEmbeddings, Some(BASE_URL))],
    models: list_models,
    ..Vendor::DEFAULT
};

/// Its embedding models: the region's control plane, its runtime's host
/// without `-runtime`, lists them.
fn list_models(p: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async move {
        let list = "/foundation-models?byOutputModality=EMBEDDING";
        let call = http::Request::get(list).body(Body::default());
        let base = p.base_url_of(BedrockEmbeddings::NAME).unwrap_or(BASE_URL);
        let control = p.client_at(&base.replacen("bedrock-runtime.", "bedrock.", 1))?;
        let (answer, _) = control.send(call.map_err(built_wrong)?).await?;
        let answer = json(answer).await?;
        let summaries = answer["modelSummaries"].as_array().into_iter().flatten();
        let model = |m: &Value| {
            let mut model = Model::new(m["modelId"].as_str()?);
            model.display_name = m["modelName"].as_str().map(str::to_owned);
            model.kind = Some("embedding".to_owned());
            Some(model)
        };
        Ok(summaries.filter_map(model).collect())
    })
}
