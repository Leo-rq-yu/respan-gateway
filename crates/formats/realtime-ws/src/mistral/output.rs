//! What the model says: Mistral's server events as the kind's [`Output`],
//! read at the provider's edge.

use respan_gateway_core::Fault;
use respan_gateway_http::bytes::Bytes;
use respan_gateway_realtime::{Cause, Output, Usage};
use serde_json::Value;

use super::{NAME, session};
use crate::json::{self, at, number, remove, string, take, text};

/// A provider's events as the kind's.
#[derive(Debug)]
pub(super) struct Provider;

impl Provider {
    pub(super) fn read(&mut self, raw: Bytes, out: &mut Vec<Output>) -> Result<(), Fault> {
        let v: Value = serde_json::from_slice(&raw)
            .map_err(|_| Fault::upstream("a provider's event is not JSON"))?;
        match decode(&v) {
            Some(events) => out.extend(events),
            None => out.push(Output::native(NAME, v)),
        }
        Ok(())
    }
}

/// The events of `v`; `None` for one the kind has no form for.
fn decode(v: &Value) -> Option<Vec<Output>> {
    let mut rest = v.clone();
    let rest = &mut rest;
    let kind = take(rest, &["type"], string, text)?;
    let events = match kind.as_str() {
        "session.created" => vec![Output::Opened(session::decode(rest.clone()))],
        "session.updated" => vec![Output::Updated(session::decode(rest.clone()))],
        "transcription.text.delta" => {
            let text = take(rest, &["text"], string, text)?;
            vec![transcript(text, false, rest)]
        }
        "transcription.done" => {
            let used = usage(rest);
            let text = take(rest, &["text"], string, text)?;
            let mut events = vec![transcript(text, true, rest)];
            events.extend(used.map(Output::Usage));
            events
        }
        "error" => {
            let code = take(rest, &["error", "code"], |c| c.as_u64(), number);
            let message = match at(rest, &["error", "message"])? {
                Value::String(message) => message.clone(),
                other => other.to_string(),
            };
            remove(rest, &["error", "message"]);
            let cause = match code {
                Some(400..500) => Cause::Invalid,
                _ => Cause::Provider,
            };
            vec![Output::Error {
                cause,
                code: code.map(|c| c.to_string()),
                message,
                native: json::native(NAME, std::mem::take(rest)),
            }]
        }
        _ => return None,
    };
    Some(events)
}

fn transcript(text: String, complete: bool, rest: &mut Value) -> Output {
    Output::InputTranscript {
        text,
        complete,
        native: json::native(NAME, std::mem::take(rest)),
    }
}

/// The tokens a session's end reports, taken from its `usage`. Its input is
/// the user's audio. The seconds of audio it also reports, and its service
/// tier, stay native.
fn usage(v: &mut Value) -> Option<Usage> {
    let count = |name: &str| at(v, &["usage", name])?.as_u64();
    let (input, output) = (
        count("prompt_tokens")?,
        count("completion_tokens").unwrap_or(0),
    );
    for name in ["prompt_tokens", "completion_tokens", "total_tokens"] {
        remove(v, &["usage", name]);
    }
    let mut usage = Usage::default();
    usage.tokens.input_tokens = input;
    usage.tokens.output_tokens = output;
    usage.input_audio_tokens = input;
    Some(usage).filter(|u| *u != Usage::default())
}
