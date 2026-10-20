//! Mistral: Chat Completions, embeddings and transcriptions, as OpenAI
//! serves them, and speech, as it serves its own.

use base64::Engine as _;
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_conversation_http::read_provider_error;
use respan_gateway_core::{Connection, Fault, Format, Provider};
use respan_gateway_http::bytes::{Bytes, BytesMut};
use respan_gateway_http::http::{self, Method};
use respan_gateway_http::{Connect, Decoder, Encoder, ProviderIo, Upstream};
use respan_gateway_media::speech::{Input, Output};
use respan_gateway_media_http::openai::{OpenAiEmbeddings, OpenAiSpeech, OpenAiTranscriptions};
use serde_json::{Map, Value};

use respan_gateway_vendor_kit::Vendor;

/// Where its API is.
const BASE_URL: &str = "https://api.mistral.ai/v1";

/// Mistral.
pub const VENDOR: Vendor = Vendor {
    id: "mistral",
    key_env: &["MISTRAL_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&OpenAiEmbeddings, Some(BASE_URL)),
        (&OpenAiTranscriptions, Some(BASE_URL)),
        (&MistralSpeech, Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};

/// Mistral's speech, a provider format of OpenAI Speech's name, so a client
/// of OpenAI Speech is relayed to it. It departs from the spec: it answers
/// every `response_format` as JSON, `{"audio_data": <base64>}`, which is
/// decoded into one [`Output::Audio`], and it refuses (422) `instructions`,
/// `speed` and `stream_format`, so a request with `instructions` or `speed`
/// fails `bad_request` before it is sent, and a stream is not asked of it.
/// Its `pcm` is 32-bit float at 24 kHz, not OpenAI's 16-bit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MistralSpeech;

const SPEECH: &str = "/audio/speech";
const AUDIO_DATA: &str = "audio_data";

/// Its spec's: a bearer key, over HTTP, beside its base URL.
impl Upstream for MistralSpeech {
    fn name(&self) -> &str {
        OpenAiSpeech::NAME
    }

    fn provider(&self, name: String, connect: Connect) -> Provider {
        Provider::new(name, *self, move |x| connect(x))
    }
}

impl Format<ProviderIo, Output, Input> for MistralSpeech {
    fn name(&self) -> &str {
        OpenAiSpeech::NAME
    }

    fn bind(&self, io: ProviderIo) -> Result<Connection<Output, Input>, Fault> {
        Ok(io.with_refusal(read_provider_error).bind(
            |head, input: Option<&Input>| {
                let input = input.ok_or_else(|| {
                    Fault::config("mistral speech does not accept streaming inputs")
                })?;
                post(head, input)
            },
            |_, _| Some(Box::new(AudioData::default()) as Box<dyn Decoder<Output>>),
        ))
    }
}

/// The call of `input`, in JSON.
fn post(head: &mut http::request::Parts, input: &Input) -> Result<Box<dyn Encoder<Input>>, Fault> {
    if input.instructions.is_some() {
        return Err(Fault::bad_request("mistral speech takes no 'instructions'"));
    }
    if input.speed.is_some() {
        return Err(Fault::bad_request("mistral speech takes no 'speed'"));
    }
    head.method = Method::POST;
    respan_gateway_http::path::set_path(head, SPEECH)?;
    head.headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    let mut v = Map::new();
    if let Some(model) = &input.model {
        v.insert("model".into(), model.clone().into());
    }
    v.insert("input".into(), input.text.clone().into());
    if let Some(voice) = &input.voice {
        v.insert("voice".into(), voice.clone().into());
    }
    if let Some(format) = &input.audio_format {
        v.insert("response_format".into(), format.clone().into());
    }
    let body = Bytes::from(serde_json::to_vec(&v).map_err(Fault::from_error)?);
    Ok(Box::new(move |_: &Input, out: &mut Vec<Bytes>| {
        out.push(body.clone());
        Ok(())
    }))
}

/// The answer, read whole: its `audio_data`, decoded, as one chunk.
#[derive(Default)]
struct AudioData {
    read: BytesMut,
}

impl Decoder<Output> for AudioData {
    fn decode(&mut self, raw: Bytes, _: &mut Vec<Output>) -> Result<(), Fault> {
        self.read.extend_from_slice(&raw);
        Ok(())
    }

    fn end(&mut self, end: Result<(), Fault>, out: &mut Vec<Output>) -> Result<(), Fault> {
        end?;
        let answer: Value = serde_json::from_slice(&self.read)
            .map_err(|_| Fault::upstream("mistral speech answered no JSON"))?;
        let audio = answer
            .get(AUDIO_DATA)
            .and_then(Value::as_str)
            .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
            .ok_or_else(|| Fault::upstream("mistral speech answered no base64 'audio_data'"))?;
        out.push(Output::Audio(Bytes::from(audio)));
        Ok(())
    }
}
