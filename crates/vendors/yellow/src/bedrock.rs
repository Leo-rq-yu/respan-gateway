//! Amazon Bedrock: Stability AI's image models, as its `InvokeModel`
//! serves them.

use respan_gateway_media_http::bedrock::BedrockImages;

use respan_gateway_vendor_kit::{BoxFuture, Model, Models, Vendor, VendorProvider};

/// Where its runtime API is, in the region `AWS_REGION` names.
const BASE_URL: &str = "https://bedrock-runtime.{AWS_REGION}.amazonaws.com";

/// Bedrock, its key a Bedrock API key, which its SDKs read from
/// `AWS_BEARER_TOKEN_BEDROCK`.
pub const VENDOR: Vendor = Vendor {
    id: "bedrock",
    key_env: &["AWS_BEARER_TOKEN_BEDROCK"],
    formats: &[(&BedrockImages, Some(BASE_URL))],
    models: list_models,
    ..Vendor::DEFAULT
};

/// The Stability models it serves, as their ids name them: its runtime
/// API lists none, and its control API takes no Bedrock API key.
const MODELS: &[&str] = &[
    "stability.sd3-5-large-v1:0",
    "stability.stable-image-ultra-v1:1",
    "stability.stable-image-core-v1:1",
];

/// Its models: [`MODELS`].
fn list_models(_: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async { Ok(MODELS.iter().map(|id| Model::new(*id)).collect()) })
}
