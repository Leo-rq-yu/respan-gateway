//! Google Vertex AI: Gemini's `generateContent` and Chat Completions, at a
//! project's own regional URL, with a Google access token.

use respan_gateway_conversation_http::Schema;
use respan_gateway_conversation_http::dialect::{Dialect, Dialected};
use respan_gateway_conversation_http::gemini::GeminiGenerate;
use respan_gateway_conversation_http::openai::OpenAiChat;
use serde_json::Value;

use respan_gateway_vendor_kit::{
    Body, BoxFuture, KeyIn, Model, Models, Vendor, VendorProvider, built_wrong, http,
    json as body_json, models,
};

/// Where Gemini's own API is: a project and location's Google models. A
/// `global` location is at `https://aiplatform.googleapis.com/v1/projects/..`
/// instead, so it is given as the `base_url`.
const API: &str = "https://{GOOGLE_CLOUD_LOCATION}-aiplatform.googleapis.com/v1/projects/{GOOGLE_CLOUD_PROJECT}/locations/{GOOGLE_CLOUD_LOCATION}/publishers/google";

/// Its OpenAI-compatible API, beside its own; a model is named
/// `google/gemini-2.5-flash` in it.
const OPENAI_API: &str = "https://{GOOGLE_CLOUD_LOCATION}-aiplatform.googleapis.com/v1/projects/{GOOGLE_CLOUD_PROJECT}/locations/{GOOGLE_CLOUD_LOCATION}/endpoints/openapi";

/// Where its models are listed, after its origin.
const MODELS: &str = "/v1beta1/publishers/google/models?pageSize=1000";

/// What starts the name of a publisher model in its list.
const MODEL_PREFIX: &str = "publishers/google/models/";

/// Google Vertex AI. The key is a Google access token, such as
/// `gcloud auth print-access-token` prints, which expires: the gateway
/// sends the one it is given.
pub const VENDOR: Vendor = Vendor {
    id: "vertex",
    key_env: &["GOOGLE_VERTEX_ACCESS_TOKEN", "CLOUDSDK_AUTH_ACCESS_TOKEN"],
    formats: &[
        (&Dialected::new(GeminiGenerate, Vertex), Some(API)),
        (&OpenAiChat, Some(OPENAI_API)),
    ],
    models: list_models,
    ..Vendor::DEFAULT
};

/// Vertex's `generateContent`: its key a bearer token, not Gemini's
/// `x-goog-api-key`.
#[derive(Debug, Clone, Copy)]
pub struct Vertex;

impl Dialect for Vertex {
    fn key(&self) -> Option<KeyIn> {
        Some(KeyIn::Bearer)
    }
}

/// Its models, as its publisher list has them (`{"publisherModels":
/// [{"name": "publishers/google/models/.."}]}`), their ids alone, at its
/// own API's origin (its base before `/v1/`): the list is not under a
/// project. As OpenAI lists them for a provider of Chat Completions alone.
fn list_models(p: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async move {
        let Some(base) = p.base_url_of(GeminiGenerate::NAME) else {
            return models::openai(p).await;
        };
        let origin = base.split_once("/v1/").map_or(base, |(origin, _)| origin);
        let call = http::Request::get(MODELS).body(Body::default());
        let (answer, _) = p
            .client_at(origin)?
            .send(call.map_err(built_wrong)?)
            .await?;
        Ok(listed(&body_json(answer).await?))
    })
}

/// The models of a publisher list.
fn listed(answer: &Value) -> Vec<Model> {
    let models = answer["publisherModels"].as_array().into_iter().flatten();
    let model = |m: &Value| {
        let name = m["name"].as_str()?;
        Some(Model::new(name.strip_prefix(MODEL_PREFIX).unwrap_or(name)))
    };
    models.filter_map(model).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use respan_gateway_conversation::Input;
    use respan_gateway_vendor_kit::Upstream;
    use serde_json::json;

    fn input(model: &str) -> Input {
        let mut input = Input::default();
        input.model = Some(model.to_owned());
        input
    }

    #[test]
    fn its_calls_are_under_the_project_and_location() {
        let base = API
            .replace("{GOOGLE_CLOUD_LOCATION}", "us-central1")
            .replace("{GOOGLE_CLOUD_PROJECT}", "p1");
        let path = GeminiGenerate
            .target(&input("gemini-2.5-flash"), true)
            .expect("target")
            .expect("path");
        assert_eq!(
            format!("{base}{path}"),
            "https://us-central1-aiplatform.googleapis.com/v1/projects/p1/locations/us-central1\
             /publishers/google/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn its_key_is_a_bearer_token() {
        let format = Dialected::new(GeminiGenerate, Vertex);
        assert_eq!(Upstream::key(&format), KeyIn::Bearer);
    }

    #[test]
    fn its_models_lose_the_publisher_path() {
        let answer = json!({"publisherModels": [
            {"name": "publishers/google/models/gemini-2.5-pro"},
            {"name": "gemini-x"},
            {"versionId": "none"},
        ]});
        let ids: Vec<_> = listed(&answer).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["gemini-2.5-pro", "gemini-x"]);
    }
}
