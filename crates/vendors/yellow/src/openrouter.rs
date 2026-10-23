//! OpenRouter: Chat Completions, embeddings, transcriptions, speech,
//! images and videos, for many vendors' models; speech and images in its dialect.

use std::borrow::Cow;
use std::collections::HashMap;

use base64::Engine as _;
use respan_gateway_conversation::Output;
use respan_gateway_conversation::{Media, MediaSource, Usage};
use respan_gateway_conversation_http::dialect::{Call, Dialect, Dialected};
use respan_gateway_conversation_http::fields::{kept, native_of};
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::NativeValue;
use respan_gateway_core::{Fault, Filter};
use respan_gateway_http::bytes::Bytes;
use respan_gateway_http::http::response::Parts as ResponseHead;
use respan_gateway_http::{Decoder, Whole};
use respan_gateway_media::image;
use respan_gateway_media::speech;
use respan_gateway_media_http::dialect::Dialect as MediaDialect;
use respan_gateway_media_http::dialect::Dialected as MediaDialected;
use respan_gateway_media_http::openai::{
    OpenAiEmbeddings, OpenAiImages, OpenAiSpeech, OpenAiTranscriptions,
};
use respan_gateway_media_http::openrouter::OpenRouterVideos;
use serde_json::{Map, Value, json};

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
        (
            &MediaDialected::new(OpenAiImages, OpenRouterImages),
            Some(BASE_URL),
        ),
        (&OpenRouterVideos, Some(BASE_URL)),
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

/// OpenRouter's images: it serves them at `/images`, edits too, which it
/// takes as a generation naming its source images by `input_references`
/// (`{"type": "image_url", "image_url": {"url"}}`); a mask has no place
/// there and fails `bad_request` before sending. It answers whole, in JSON,
/// each image with its `media_type`, and usage as `prompt_tokens` and
/// `completion_tokens` beside its `cost`; it is not asked to stream.
#[derive(Debug, Clone, Copy)]
pub struct OpenRouterImages;

impl MediaDialect<image::Input, image::Output> for OpenRouterImages {
    fn input<'a>(&self, input: &'a image::Input) -> Result<Cow<'a, image::Input>, Fault> {
        if input.mask.is_some() {
            return Err(Fault::bad_request("openrouter images take no 'mask'"));
        }
        if input.images.is_empty() {
            return Ok(Cow::Borrowed(input));
        }
        let link = |media: &Media| {
            let url = match &media.source {
                MediaSource::Url(url) => url.clone(),
                MediaSource::Bytes(bytes) => format!(
                    "data:{};base64,{}",
                    media.mime_type,
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ),
            };
            json!({ "type": "image_url", "image_url": { "url": url } })
        };
        let mut fields = kept(OpenAiImages::NAME, &input.native)?;
        let images: Vec<Value> = input.images.iter().map(link).collect();
        fields.insert("input_references".into(), images.into());
        let mut input = input.clone();
        input.images.clear();
        input.native = native_of(OpenAiImages::NAME, fields);
        Ok(Cow::Owned(input))
    }

    fn path(&self) -> Option<&'static str> {
        Some("/images")
    }

    fn read_response(&self, head: &ResponseHead) -> Option<Box<dyn Decoder<image::Output>>> {
        head.status
            .is_success()
            .then(|| Box::new(Whole::new(images)) as _)
    }

    fn streams(&self, _: bool) -> bool {
        false
    }
}

/// The events of a whole answer: its [`image::Start`], an [`image::Image`]
/// of each item of `data`, and its usage, `cost` and the rest native.
fn images(raw: Bytes, out: &mut Vec<image::Output>) -> Result<(), Fault> {
    let answer: Value = serde_json::from_slice(&raw)
        .map_err(|_| Fault::upstream("openrouter images answered no JSON"))?;
    let items = answer["data"]
        .as_array()
        .ok_or_else(|| Fault::upstream("openrouter images answered no 'data'"))?;
    out.push(image::Output::Start(image::Start::new(
        answer["created"].as_u64(),
    )));
    for item in items {
        let bytes = item["b64_json"]
            .as_str()
            .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
            .ok_or_else(|| Fault::upstream("openrouter images answered no base64 'b64_json'"))?;
        let mime = item["media_type"].as_str().unwrap_or("image/png");
        let media = Media::new(mime, MediaSource::Bytes(Bytes::from(bytes)));
        out.push(image::Output::Image(image::Image::new(media)));
    }
    if let Some(usage) = answer["usage"].as_object() {
        let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        out.push(image::Output::Usage(Usage::new(
            count("prompt_tokens"),
            count("completion_tokens"),
        )));
        const READ: &[&str] = &["prompt_tokens", "completion_tokens", "total_tokens"];
        let rest: Map<String, Value> = usage
            .iter()
            .filter(|(k, _)| !READ.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !rest.is_empty() {
            let native = NativeValue::new(OpenAiImages::NAME, json!({ "usage": rest }));
            out.push(image::Output::Native(native));
        }
    }
    Ok(())
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
    fn it_speaks_every_media_format_it_serves() {
        let names = [
            "openai-chat",
            "openai-embeddings",
            "openai-transcriptions",
            "openai-speech",
            "openai-images",
            "openrouter-videos",
        ];
        for name in names {
            assert!(VENDOR.format(name).is_some(), "{name}");
        }
    }

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

    fn decode(body: &str) -> Vec<image::Output> {
        let head = http::Response::builder()
            .body(())
            .expect("head")
            .into_parts()
            .0;
        let mut decoder = OpenRouterImages.read_response(&head).expect("read");
        let mut out = Vec::new();
        decoder
            .decode(Bytes::copy_from_slice(body.as_bytes()), &mut out)
            .expect("decode");
        decoder.end(Ok(()), &mut out).expect("end");
        out
    }

    #[test]
    fn images_are_served_at_images_and_an_edit_names_its_sources() {
        assert_eq!(OpenRouterImages.path(), Some("/images"));
        let mut input = image::Input::new("add a hat");
        input.images.push(Media::new(
            "image/png",
            MediaSource::Url("https://x/a.png".into()),
        ));
        let sent = OpenRouterImages.input(&input).expect("input");
        assert!(sent.images.is_empty());
        let fields = kept(OpenAiImages::NAME, &sent.native).expect("native");
        assert_eq!(
            fields["input_references"],
            json!([{ "type": "image_url", "image_url": { "url": "https://x/a.png" } }])
        );
        input.mask = Some(input.images[0].clone());
        assert!(OpenRouterImages.input(&input).is_err());
    }

    #[test]
    fn an_images_answer_is_its_images_and_usage() {
        let out = decode(
            r#"{"created": 7, "data": [{"b64_json": "SUQz", "media_type": "image/webp"}],
                "usage": {"prompt_tokens": 2, "completion_tokens": 5, "total_tokens": 7, "cost": 0.04}}"#,
        );
        assert_eq!(out.len(), 4);
        let image::Output::Image(first) = &out[1] else {
            panic!("no image")
        };
        assert_eq!(first.media.mime_type, "image/webp");
        assert!(
            matches!(&out[2], image::Output::Usage(u) if u.input_tokens == 2 && u.output_tokens == 5)
        );
        assert!(matches!(&out[3], image::Output::Native(_)));
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
