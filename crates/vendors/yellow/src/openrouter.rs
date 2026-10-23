//! OpenRouter: Chat Completions, embeddings, transcriptions and speech, for
//! many vendors' models; speech in its dialect.

use std::borrow::Cow;
use std::collections::HashMap;

use respan_gateway_conversation::Output;
use respan_gateway_conversation_http::dialect::{Call, Dialect, Dialected};
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::{Fault, Filter};
use respan_gateway_media::speech;
use respan_gateway_media_http::dialect::Dialect as MediaDialect;
use respan_gateway_media_http::dialect::Dialected as MediaDialected;
use respan_gateway_media_http::openai::{OpenAiEmbeddings, OpenAiSpeech, OpenAiTranscriptions};
use serde_json::Value;

use respan_gateway_vendor_kit::{
    Body, BoxFuture, Model, Models, Vendor, VendorProvider, built_wrong, chat, http, json, models,
};

/// Where its API is.
const BASE_URL: &str = "https://openrouter.ai/api/v1";

/// OpenRouter. Its models' names are `<vendor>/<model>`: a configured
/// provider named `<vendor>` takes the name first.
pub const VENDOR: Vendor = Vendor {
    id: "openrouter",
    key_env: &["OPENROUTER_API_KEY"],
    formats: &[
        (&Dialected::new(OpenAiChat, OpenRouter), Some(BASE_URL)),
        (&OpenAiEmbeddings, Some(BASE_URL)),
        (&OpenAiTranscriptions, Some(BASE_URL)),
        (
            &MediaDialected::new(OpenAiSpeech, OpenRouterSpeech),
            Some(BASE_URL),
        ),
    ],
    models: list_models,
    ..Vendor::DEFAULT
};

/// OpenRouter's Chat Completions: a choice's `native_finish_reason` only
/// describes it.
#[derive(Debug, Clone, Copy)]
pub struct OpenRouter;

impl Dialect for OpenRouter {
    fn output(&self, call: &Call) -> Option<Box<dyn Filter<Output>>> {
        chat::read(call, &["native_finish_reason"], false)
    }
}

/// OpenRouter's speech: its audio is `mp3` or `pcm`, and `pcm` when none is
/// asked, so a call that names none is sent for `mp3`, as OpenAI's is, and
/// any other format fails `bad_request` before sending. It is not asked to
/// stream.
#[derive(Debug, Clone, Copy)]
pub struct OpenRouterSpeech;

impl MediaDialect<speech::Input, speech::Output> for OpenRouterSpeech {
    fn input<'a>(&self, input: &'a speech::Input) -> Result<Cow<'a, speech::Input>, Fault> {
        match input.audio_format.as_deref() {
            Some("mp3" | "pcm") => Ok(Cow::Borrowed(input)),
            Some(other) => Err(Fault::bad_request(format!(
                "openrouter speech takes 'mp3' or 'pcm', not '{other}'"
            ))),
            None => {
                let mut input = input.clone();
                input.audio_format = Some("mp3".to_owned());
                Ok(Cow::Owned(input))
            }
        }
    }

    fn streams(&self, _: bool) -> bool {
        false
    }
}

/// The kind of call that serves a model, from its `output_modalities`: a
/// model that outputs text chats, whatever else it outputs; else the kind
/// of its first output. An output no format serves, such as `rerank`,
/// names itself.
fn kind_of(output: &[&str]) -> String {
    if output.contains(&"text") {
        return "chat".to_owned();
    }
    match output.first() {
        Some(&"embeddings") => "embedding",
        Some(&"decisions") => "decision",
        Some(other) => other,
        None => "chat",
    }
    .to_owned()
}

/// Its models: `GET /models?output_modalities=all`, which lists those that
/// output more than text, each of the kind its outputs say.
fn list_models(p: &VendorProvider) -> BoxFuture<'_, Models> {
    Box::pin(async move {
        let call = http::Request::get("/models?output_modalities=all").body(Body::default());
        let (answer, _) = p.client()?.send(call.map_err(built_wrong)?).await?;
        Ok(listed_with_kinds(&json(answer).await?))
    })
}

/// The models of `answer`, each of the kind its outputs say.
fn listed_with_kinds(answer: &Value) -> Vec<Model> {
    let cards = answer["data"].as_array().into_iter().flatten();
    let kinds: HashMap<&str, String> = cards
        .filter_map(|m| {
            let output = m["architecture"]["output_modalities"].as_array()?;
            let output: Vec<&str> = output.iter().filter_map(Value::as_str).collect();
            Some((m["id"].as_str()?, kind_of(&output)))
        })
        .collect();
    let mut listed = models::listed(answer);
    for model in &mut listed {
        model.kind = kinds.get(model.id.as_str()).cloned();
    }
    listed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_models_outputs_say_its_kind() {
        let card = |id: &str, out: Value| json!({ "id": id, "architecture": { "output_modalities": out } });
        let answer = json!({ "data": [
            card("a/chat", json!(["text"])),
            card("a/nano", json!(["image", "text"])),
            card("a/draw", json!(["image"])),
            card("a/embed", json!(["embeddings"])),
            card("a/tts", json!(["speech"])),
            card("a/stt", json!(["transcription"])),
            card("a/clip", json!(["video"])),
            card("a/rank", json!(["rerank"])),
            card("a/decide", json!(["decisions"])),
            json!({ "id": "a/bare" }),
        ] });
        let kinds: Vec<_> = listed_with_kinds(&answer)
            .into_iter()
            .map(|m| m.kind.unwrap_or_default())
            .collect();
        assert_eq!(
            kinds,
            [
                "chat",
                "chat",
                "image",
                "embedding",
                "speech",
                "transcription",
                "video",
                "rerank",
                "decision",
                ""
            ]
        );
    }

    #[test]
    fn speech_is_asked_for_mp3_unless_it_names_a_format() {
        let mut input = speech::Input::new("hi");
        let sent = OpenRouterSpeech.input(&input).expect("input");
        assert_eq!(sent.audio_format.as_deref(), Some("mp3"));
        input.audio_format = Some("pcm".to_owned());
        assert!(OpenRouterSpeech.input(&input).is_ok());
        input.audio_format = Some("opus".to_owned());
        assert!(OpenRouterSpeech.input(&input).is_err());
    }
}
