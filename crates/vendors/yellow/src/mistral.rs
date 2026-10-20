//! Mistral: Chat Completions, embeddings, transcriptions and speech, as
//! OpenAI serves them, the last two in its dialect.

use std::borrow::Cow;

use base64::Engine as _;
use respan_gateway_conversation_http::fields::{kept, native_of};
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::{Fault, Filter};
use respan_gateway_http::bytes::{Bytes, BytesMut};
use respan_gateway_media::{speech, transcription};
use respan_gateway_media_http::dialect::{Dialect, Dialected};
use respan_gateway_media_http::openai::{OpenAiEmbeddings, OpenAiSpeech, OpenAiTranscriptions};
use serde_json::Value;

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
        (
            &Dialected::new(OpenAiTranscriptions, Mistral),
            Some(BASE_URL),
        ),
        (&Dialected::new(OpenAiSpeech, Mistral), Some(BASE_URL)),
    ],
    ..Vendor::DEFAULT
};

/// Mistral's transcriptions and speech.
///
/// Its transcriptions take no `prompt` and no translation, which fail
/// `bad_request` before sending, and no `response_format`, which is not
/// sent: the client's is rendered from its JSON.
///
/// Its speech takes the voice as `voice_id`, refuses (422) `instructions`
/// and `speed`, which fail `bad_request` before sending, and answers every
/// `response_format` as JSON, `{"audio_data": <base64>}`, decoded into one
/// [`speech::Output::Audio`]. Its `pcm` is 32-bit float at 24 kHz, not
/// OpenAI's 16-bit.
///
/// Neither is asked to stream, as its events are not OpenAI's; a client
/// that asked still gets a stream, framed at the client edge.
#[derive(Debug, Clone, Copy)]
pub struct Mistral;

impl Dialect<transcription::Input, transcription::Output> for Mistral {
    fn input<'a>(
        &self,
        input: &'a transcription::Input,
    ) -> Result<Cow<'a, transcription::Input>, Fault> {
        if input.task == transcription::Task::Translate {
            return Err(Fault::bad_request("mistral does not translate audio"));
        }
        if input.prompt.is_some() {
            return Err(Fault::bad_request(
                "mistral transcriptions take no 'prompt'",
            ));
        }
        let mut fields = kept(OpenAiTranscriptions::NAME, &input.native)?;
        if fields.shift_remove("response_format").is_none() {
            return Ok(Cow::Borrowed(input));
        }
        let mut input = input.clone();
        input.native = native_of(OpenAiTranscriptions::NAME, fields);
        Ok(Cow::Owned(input))
    }

    fn streams(&self, _: bool) -> bool {
        false
    }
}

impl Dialect<speech::Input, speech::Output> for Mistral {
    fn input<'a>(&self, input: &'a speech::Input) -> Result<Cow<'a, speech::Input>, Fault> {
        if input.instructions.is_some() {
            return Err(Fault::bad_request("mistral speech takes no 'instructions'"));
        }
        if input.speed.is_some() {
            return Err(Fault::bad_request("mistral speech takes no 'speed'"));
        }
        let Some(voice) = &input.voice else {
            return Ok(Cow::Borrowed(input));
        };
        let mut fields = kept(OpenAiSpeech::NAME, &input.native)?;
        fields
            .entry("voice_id")
            .or_insert_with(|| voice.clone().into());
        let mut input = input.clone();
        input.voice = None;
        input.native = native_of(OpenAiSpeech::NAME, fields);
        Ok(Cow::Owned(input))
    }

    fn output(&self) -> Option<Box<dyn Filter<speech::Output>>> {
        Some(Box::new(AudioData::default()))
    }

    fn streams(&self, _: bool) -> bool {
        false
    }
}

const AUDIO_DATA: &str = "audio_data";

/// The answer, read whole: its `audio_data`, decoded, as one chunk.
#[derive(Default)]
struct AudioData {
    read: BytesMut,
}

impl Filter<speech::Output> for AudioData {
    fn edit(&mut self, output: speech::Output, out: &mut Vec<speech::Output>) -> Result<(), Fault> {
        match output {
            speech::Output::Audio(bytes) => self.read.extend_from_slice(&bytes),
            output => out.push(output),
        }
        Ok(())
    }

    fn end(&mut self, end: &Result<(), Fault>, out: &mut Vec<speech::Output>) -> Result<(), Fault> {
        if end.is_err() {
            return Ok(());
        }
        let answer: Value = serde_json::from_slice(&self.read)
            .map_err(|_| Fault::upstream("mistral speech answered no JSON"))?;
        let audio = answer
            .get(AUDIO_DATA)
            .and_then(Value::as_str)
            .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
            .ok_or_else(|| Fault::upstream("mistral speech answered no base64 'audio_data'"))?;
        out.push(speech::Output::Audio(Bytes::from(audio)));
        Ok(())
    }
}
