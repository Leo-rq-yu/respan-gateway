//! Google Vertex AI: at a project and location, Gemini's `generateContent`,
//! Claude in Anthropic's Messages, and the other models of its Model Garden
//! in Chat Completions; Gemini's embeddings in OpenAI's; with a Google
//! access token. And its express mode ([`EXPRESS`]): Gemini's
//! `generateContent` alone, with an API key.

use respan_gateway_conversation::{Input, Output};
use respan_gateway_conversation_http::Schema;
use respan_gateway_conversation_http::anthropic::AnthropicMessages;
use respan_gateway_conversation_http::gemini::GeminiGenerate;
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::futures::future::join_all;
use respan_gateway_core::{Fault, Filter};
use respan_gateway_http::bytes::Bytes;
use respan_gateway_http::http::request::Parts as RequestHead;
use respan_gateway_http::path::set_path;
use respan_gateway_media::embedding;
use respan_gateway_media_http::openai::OpenAiEmbeddings;
use serde_json::{Map, Value};

use respan_gateway_vendor_kit::{
    Ask, Body, BoxFuture, Dialect, Dialected, KeyIn, Model, Models, Rewrite, Vendor,
    VendorProvider, built_wrong, http, json as body_json, located,
};

/// A project's location, under which each of its APIs is.
macro_rules! location {
    () => {
        "https://{GOOGLE_CLOUD_LOCATION}-aiplatform.googleapis.com/v1/projects/{GOOGLE_CLOUD_PROJECT}/locations/{GOOGLE_CLOUD_LOCATION}"
    };
}

/// Google's models, in Gemini's own API.
const API: &str = concat!(location!(), "/publishers/google");

/// Anthropic's models, each at `/models/{model}:rawPredict`.
const ANTHROPIC: &str = concat!(location!(), "/publishers/anthropic");

/// The location itself: Chat Completions are at `/endpoints/openapi`, but
/// for Mistral's models, each at its own `rawPredict` ([`OpenApi`]).
const LOCATION: &str = location!();

/// Its OpenAI-compatible API, of Chat Completions and embeddings; a model
/// is named `google/gemini-3.5-flash` in it.
const OPENAI_API: &str = concat!(location!(), "/endpoints/openapi");

/// Where express mode's Gemini is: under no project, at the global host.
const EXPRESS_API: &str = "https://aiplatform.googleapis.com/v1/publishers/google";

/// Where its models are listed, after its origin: each publisher's, a page
/// of at most 300 at a time.
const MODELS: &str = "/v1beta1/publishers";

/// The publishers whose models it serves without a deployment of one's
/// own, and who each is as a model's maker. Google's models are named as
/// they are, and Anthropic's, in their own formats; the others as
/// `{publisher}/{model}`, as Chat Completions names them.
const PUBLISHERS: &[(&str, &str)] = &[
    ("google", "google"),
    ("anthropic", "anthropic"),
    ("meta", "meta"),
    ("mistralai", "mistral"),
    ("deepseek-ai", "deepseek"),
    ("qwen", "qwen"),
    ("openai", "openai"),
    ("moonshotai", "moonshotai"),
    ("minimaxai", "minimax"),
    ("zai-org", "zai"),
    ("xai", "xai"),
];

/// Google Vertex AI. The key is a Google access token, such as `gcloud
/// auth print-access-token` prints, which expires within the hour: the
/// gateway sends the one it is given, so the host gives it a fresh one.
/// The `global` location is at the host of no region.
pub const VENDOR: Vendor = Vendor {
    id: "vertex",
    key_env: &["GOOGLE_VERTEX_ACCESS_TOKEN", "CLOUDSDK_AUTH_ACCESS_TOKEN"],
    formats: &[
        (&Dialected::new(GeminiGenerate, Vertex), Some(API)),
        (&Dialected::new(AnthropicMessages, Claude), Some(ANTHROPIC)),
        (&Dialected::new(OpenAiChat, OpenApi), Some(LOCATION)),
        (
            &Dialected::new(OpenAiEmbeddings, Embeddings),
            Some(OPENAI_API),
        ),
    ],
    models: list_models,
    format_for: Some(format_for),
    locate,
};

/// Vertex AI in express mode: Gemini's `generateContent` under no project,
/// with an API key in `x-goog-api-key`, as Gemini's API takes it. It lists
/// no models, as its API serves `generateContent`, `streamGenerateContent`
/// and `countTokens` alone.
pub const EXPRESS: Vendor = Vendor {
    id: "vertex-express",
    key_env: &["GOOGLE_VERTEX_API_KEY"],
    formats: &[(&GeminiGenerate, Some(EXPRESS_API))],
    models: list_none,
    ..Vendor::DEFAULT
};

/// No models: express mode lists none.
fn list_none(_: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async { Ok(Vec::new()) })
}

/// `url` located, its `global` location at the host of no region,
/// `aiplatform.googleapis.com`, as Google has it: no host is
/// `global-aiplatform.googleapis.com`.
fn locate(url: &str) -> Result<String, &str> {
    let url = located(url)?;
    Ok(match url.strip_prefix("https://global-aiplatform.") {
        Some(rest) => format!("https://aiplatform.{rest}"),
        None => url,
    })
}

/// The format a chat with `model` goes in: Claude's own, Anthropic's
/// Messages; Google's, the client's own if it is Chat Completions, else
/// `generateContent`; another publisher's, Chat Completions. A model its
/// list does not have is Google's unless it is named `{publisher}/..` or
/// `claude-..`.
fn format_for(ask: &Ask, model: &Model) -> Option<&'static str> {
    if ask.kind != "chat" {
        return None;
    }
    let maker = model.maker.as_deref();
    let id = model.id.as_str();
    Some(match maker {
        Some("anthropic") => AnthropicMessages::NAME,
        None if id.starts_with("claude-") => AnthropicMessages::NAME,
        Some("google") | None if !id.contains('/') => match ask.format {
            OpenAiChat::NAME => OpenAiChat::NAME,
            _ => GeminiGenerate::NAME,
        },
        _ => OpenAiChat::NAME,
    })
}

/// Vertex's `generateContent`: its key a bearer token, not Gemini's
/// `x-goog-api-key`.
#[derive(Debug, Clone, Copy)]
pub struct Vertex;

impl Dialect for Vertex {
    type Input = Input;
    type Output = Output;

    fn key(&self) -> Option<KeyIn> {
        Some(KeyIn::Bearer)
    }
}

/// Claude's Anthropic Messages as Vertex serves them: the spec's but the
/// call, posted to the model's `rawPredict`, or `streamRawPredict` for a
/// stream, the model not in the body but its version
/// (`anthropic_version`), and the key a bearer token.
#[derive(Debug, Clone, Copy)]
pub struct Claude;

impl Dialect for Claude {
    type Input = Input;
    type Output = Output;

    fn key(&self) -> Option<KeyIn> {
        Some(KeyIn::Bearer)
    }

    fn request(
        &self,
        head: &mut RequestHead,
        body: Option<&Bytes>,
    ) -> Result<Option<Rewrite>, Fault> {
        let Some(mut call) = body.map(json_call).transpose()? else {
            return Ok(None);
        };
        let model = call.get("model").and_then(Value::as_str);
        let model = segment(model, "claude-sonnet-5-5")?;
        set_path(head, &format!("/models/{model}:{}", predict(&call)))?;
        call.shift_remove("model");
        call.insert("anthropic_version".into(), "vertex-2023-10-16".into());
        Ok(Some(rewritten(call)))
    }
}

/// Its Chat Completions, at its OpenAI-compatible API: a Google model
/// named as it is (`gemini-3.5-flash`) is `google/gemini-3.5-flash` there.
/// A chat with one of Mistral's (`mistralai/..`) is posted to the model's
/// own `rawPredict`, or `streamRawPredict` for a stream, named without its
/// publisher, as Vertex serves no Mistral model at its OpenAI-compatible
/// API.
#[derive(Debug, Clone, Copy)]
pub struct OpenApi;

impl Dialect for OpenApi {
    type Input = Input;
    type Output = Output;

    fn inputs(&self) -> Option<Box<dyn Filter<Input>>> {
        Some(Box::new(|input: &mut Input| {
            input.model = published(input.model.take());
            Ok(())
        }))
    }

    fn key(&self) -> Option<KeyIn> {
        Some(KeyIn::Bearer)
    }

    fn request(
        &self,
        head: &mut RequestHead,
        body: Option<&Bytes>,
    ) -> Result<Option<Rewrite>, Fault> {
        let Some(mut call) = body.map(json_call).transpose()? else {
            return Ok(None);
        };
        let model = call.get("model").and_then(Value::as_str);
        let Some(model) = model.and_then(|m| m.strip_prefix("mistralai/")) else {
            set_path(head, "/endpoints/openapi/chat/completions")?;
            return Ok(None);
        };
        let model = segment(Some(model), "mistralai/mistral-medium-3")?.to_owned();
        let at = format!("/publishers/mistralai/models/{model}:{}", predict(&call));
        set_path(head, &at)?;
        call.insert("model".into(), model.into());
        Ok(Some(rewritten(call)))
    }
}

/// Its embeddings, at its OpenAI-compatible API, a Google model named
/// with its publisher, as its Chat Completions name it ([`OpenApi`]).
#[derive(Debug, Clone, Copy)]
pub struct Embeddings;

impl Dialect for Embeddings {
    type Input = embedding::Input;
    type Output = embedding::Output;

    fn inputs(&self) -> Option<Box<dyn Filter<embedding::Input>>> {
        Some(Box::new(|input: &mut embedding::Input| {
            input.model = published(input.model.take());
            Ok(())
        }))
    }

    fn key(&self) -> Option<KeyIn> {
        Some(KeyIn::Bearer)
    }
}

/// A model named as its OpenAI-compatible API names it: with its
/// publisher, `google` if it has none.
fn published(model: Option<String>) -> Option<String> {
    let model = model?;
    Some(match model.contains('/') {
        true => model,
        false => format!("google/{model}"),
    })
}

/// The method a model's own endpoint takes `call` at: `streamRawPredict`
/// if it asks for a stream, else `rawPredict`.
fn predict(call: &Map<String, Value>) -> &'static str {
    match call.get("stream").and_then(Value::as_bool) {
        Some(true) => "streamRawPredict",
        _ => "rawPredict",
    }
}

/// A call's body as the JSON object its format wrote.
fn json_call(body: &Bytes) -> Result<Map<String, Value>, Fault> {
    serde_json::from_slice(body)
        .map_err(|e| Fault::internal("a Vertex AI call is no JSON object").with_source(e))
}

/// A call's body rewritten as `call`.
fn rewritten(call: Map<String, Value>) -> Rewrite {
    Box::new(move |_: Bytes| {
        serde_json::to_vec(&call)
            .map(Bytes::from)
            .map_err(|e| Fault::internal("a Vertex AI call could not be written").with_source(e))
    })
}

/// `model` as one segment of a path, refused if it is not one, as `like`.
fn segment<'a>(model: Option<&'a str>, like: &str) -> Result<&'a str, Fault> {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@');
    match model {
        Some(m) if !m.is_empty() && m != "." && m != ".." && m.chars().all(plain) => Ok(m),
        _ => Err(Fault::bad_request(format!(
            "a Vertex AI call needs a model, such as {like}"
        ))),
    }
}

/// Its models: each publisher's list ([`PUBLISHERS`]) at its API's origin
/// (its base before `/v1/`), as the lists are under no project; as OpenAI
/// lists them for a provider of none of its own APIs. A list says no
/// kind, limits or prices, so a model has its id and maker alone.
fn list_models(p: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async move {
        let base = [
            GeminiGenerate::NAME,
            AnthropicMessages::NAME,
            OpenAiChat::NAME,
        ]
        .iter()
        .find_map(|f| p.base_url_of(f));
        let Some(base) = base else {
            return respan_gateway_vendor_kit::models::openai(p).await;
        };
        let origin = base.split_once("/v1/").map_or(base, |(origin, _)| origin);
        let client = p.client_at(origin)?;
        let lists = PUBLISHERS.iter().map(|&(publisher, maker)| {
            let client = &client;
            async move {
                let mut models = Vec::new();
                let mut page = String::new();
                loop {
                    let mut at = format!("{MODELS}/{publisher}/models?pageSize=300");
                    if !page.is_empty() {
                        at += &format!("&pageToken={page}");
                    }
                    let call = http::Request::get(at).body(Body::default());
                    let (answer, _) = client.send(call.map_err(built_wrong)?).await?;
                    let answer = body_json(answer).await?;
                    models.extend(listed(&answer, publisher, maker));
                    match answer["nextPageToken"].as_str() {
                        Some(next) if !next.is_empty() => page = next.to_owned(),
                        _ => return Ok::<_, Fault>(models),
                    }
                }
            }
        });
        let mut models = Vec::new();
        for listed in join_all(lists).await {
            models.extend(listed?);
        }
        Ok(models)
    })
}

/// The models of `publisher`'s list that a call reaches: none that must
/// be deployed first (`deploy`), and only those Vertex gives a resource
/// or access to; of Google's, only those with a resource, and none of its
/// open checkpoints, which are notebooks.
fn listed(answer: &Value, publisher: &str, maker: &str) -> Vec<Model> {
    let prefix = format!("publishers/{publisher}/models/");
    let models = answer["publisherModels"].as_array().into_iter().flatten();
    let model = |m: &Value| {
        let actions = m["supportedActions"].as_object();
        let can = |action: &str| actions.is_some_and(|a| a.contains_key(action));
        let open = m["openSourceCategory"].as_str().unwrap_or_default();
        let resource = m.get("publisherModelTemplate").is_some();
        let served = !can("deploy")
            && match publisher {
                "google" => resource && !open.contains("OSS"),
                _ => resource || can("requestAccess"),
            };
        let name = m["name"].as_str().filter(|_| served)?;
        let id = name.strip_prefix(prefix.as_str()).unwrap_or(name);
        let mut model = Model::new(match publisher {
            "google" | "anthropic" => id.to_owned(),
            _ => format!("{publisher}/{id}"),
        });
        model.maker = Some(maker.to_owned());
        Some(model)
    };
    models.filter_map(model).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
            .target(&input("gemini-3.5-flash"), true)
            .expect("target")
            .expect("path");
        assert_eq!(
            format!("{base}{path}"),
            "https://us-central1-aiplatform.googleapis.com/v1/projects/p1/locations/us-central1\
             /publishers/google/models/gemini-3.5-flash:streamGenerateContent?alt=sse"
        );
        let formats = VENDOR.formats.iter().map(|(f, _)| f.key());
        assert!(formats.clone().all(|key| key == KeyIn::Bearer));
        assert_eq!(formats.count(), 4);
        assert_eq!(
            Upstream::key(&GeminiGenerate),
            KeyIn::Header("x-goog-api-key")
        );
    }

    #[test]
    fn the_global_location_is_at_the_host_of_no_region() {
        let url = "https://global-aiplatform.googleapis.com/v1/projects/p/locations/global";
        assert_eq!(
            locate(url).as_deref(),
            Ok("https://aiplatform.googleapis.com/v1/projects/p/locations/global")
        );
        let regional = "https://us-east5-aiplatform.googleapis.com/v1";
        assert_eq!(locate(regional).as_deref(), Ok(regional));
    }

    /// The path `dialect` posts `call` to, and its body, as the format
    /// wrote it at `/chat`.
    fn called<D: Dialect>(dialect: &D, call: Value) -> Result<(String, Value), Fault> {
        let head = http::Request::post("/chat").body(()).expect("head");
        let mut head = head.into_parts().0;
        let body = Bytes::from(call.to_string());
        let body = match dialect.request(&mut head, Some(&body))? {
            Some(rewrite) => rewrite(body)?,
            None => body,
        };
        let body = serde_json::from_slice(&body).expect("JSON");
        Ok((head.uri.to_string(), body))
    }

    #[test]
    fn claude_is_posted_to_its_raw_predict_without_its_model() {
        let call = json!({"model": "claude-haiku-5-5", "max_tokens": 5, "messages": []});
        let (path, body) = called(&Claude, call).expect("called");
        assert_eq!(path, "/models/claude-haiku-5-5:rawPredict");
        assert_eq!(
            body,
            json!({"max_tokens": 5, "messages": [], "anthropic_version": "vertex-2023-10-16"})
        );
        let streamed = json!({"model": "claude-haiku-5-5", "stream": true});
        let (path, _) = called(&Claude, streamed).expect("called");
        assert_eq!(path, "/models/claude-haiku-5-5:streamRawPredict");
        assert!(called(&Claude, json!({"model": "claude-x/../y"})).is_err());
    }

    #[test]
    fn chat_completions_name_google_models_by_their_publisher() {
        let call = |model: &str| {
            let mut out = Vec::new();
            let mut inputs = OpenApi.inputs().expect("inputs");
            inputs.edit(input(model), &mut out).expect("edited");
            let model = out.pop().and_then(|i| i.model);
            let (path, body) = called(&OpenApi, json!({ "model": model })).expect("called");
            (body["model"].as_str().map(str::to_owned), path)
        };
        let chat = "/endpoints/openapi/chat/completions".to_owned();
        assert_eq!(
            call("gemini-3.5-flash"),
            (Some("google/gemini-3.5-flash".into()), chat.clone())
        );
        assert_eq!(
            call("meta/llama-4-maverick-17b-128e-instruct-maas"),
            (
                Some("meta/llama-4-maverick-17b-128e-instruct-maas".into()),
                chat
            )
        );
        assert_eq!(
            call("mistralai/mistral-medium-3"),
            (
                Some("mistral-medium-3".into()),
                "/publishers/mistralai/models/mistral-medium-3:rawPredict".to_owned()
            )
        );
    }

    #[test]
    fn a_chat_goes_in_its_makers_format() {
        let ask = |format| Ask {
            kind: "chat",
            format,
            model: None,
            client: None,
        };
        let made = |id: &str, maker: Option<&str>| {
            let mut model = Model::new(id);
            model.maker = maker.map(str::to_owned);
            model
        };
        let claude = made("claude-haiku-5-5", Some("anthropic"));
        let gemini = made("gemini-3.5-flash", Some("google"));
        let llama = made("meta/llama-4", Some("meta"));
        let chat = ask(OpenAiChat::NAME);
        let messages = ask(AnthropicMessages::NAME);
        assert_eq!(format_for(&chat, &claude), Some(AnthropicMessages::NAME));
        assert_eq!(format_for(&messages, &gemini), Some(GeminiGenerate::NAME));
        assert_eq!(format_for(&chat, &gemini), Some(OpenAiChat::NAME));
        assert_eq!(format_for(&messages, &llama), Some(OpenAiChat::NAME));
        // Unlisted, by its name.
        assert_eq!(
            format_for(&chat, &made("claude-x", None)),
            Some(AnthropicMessages::NAME)
        );
        assert_eq!(
            format_for(&messages, &made("gemini-x", None)),
            Some(GeminiGenerate::NAME)
        );
        let embedding = Ask {
            kind: "embedding",
            ..chat
        };
        assert_eq!(format_for(&embedding, &gemini), None);
    }

    #[test]
    fn its_models_are_those_a_call_reaches() {
        let answer = json!({"publisherModels": [
            {"name": "publishers/google/models/gemini-3.5-flash",
             "publisherModelTemplate": "projects/{project}/..",
             "supportedActions": {"openGenerationAiStudio": {}}},
            {"name": "publishers/google/models/gemma3",
             "supportedActions": {"deploy": {}, "multiDeployVertex": {}}},
            {"name": "publishers/google/models/occupancy-analytics",
             "supportedActions": {"createApplication": {}}},
            {"name": "publishers/google/models/weather-next-v2",
             "openSourceCategory": "PROPRIETARY",
             "supportedActions": {"requestAccess": {}}},
            {"name": "publishers/google/models/imageclassification-proprietary-vit",
             "openSourceCategory": "THIRD_PARTY_OWNED_OSS",
             "publisherModelTemplate": "projects/{project}/.."},
        ]});
        let ids = |answer, publisher, maker| -> Vec<(String, Option<String>)> {
            listed(answer, publisher, maker)
                .into_iter()
                .map(|m| (m.id, m.maker))
                .collect()
        };
        assert_eq!(
            ids(&answer, "google", "google"),
            [("gemini-3.5-flash".into(), Some("google".into()))]
        );
        let anthropic = json!({"publisherModels": [
            {"name": "publishers/anthropic/models/claude-haiku-4-5",
             "openSourceCategory": "PROPRIETARY",
             "supportedActions": {"requestAccess": {}}},
        ]});
        assert_eq!(
            ids(&anthropic, "anthropic", "anthropic"),
            [("claude-haiku-4-5".into(), Some("anthropic".into()))]
        );
        let meta = json!({"publisherModels": [
            {"name": "publishers/meta/models/llama-4-maverick-17b-128e-instruct-maas",
             "openSourceCategory": "THIRD_PARTY_OWNED_OSS",
             "supportedActions": {"requestAccess": {}}},
            {"name": "publishers/meta/models/sam3",
             "supportedActions": {"openNotebook": {}}},
        ]});
        assert_eq!(
            ids(&meta, "meta", "meta"),
            [(
                "meta/llama-4-maverick-17b-128e-instruct-maas".into(),
                Some("meta".into())
            )]
        );
    }
}
