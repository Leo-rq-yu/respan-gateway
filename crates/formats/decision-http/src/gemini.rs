//! Gemini's Decisions ([`GeminiDecisions`]): a `POST` to
//! `/v1beta/decisions`, answered whole in JSON, its key in
//! `x-goog-api-key`.
//!
//! It is System One's shape but for three things. What the questions are
//! about is `input`, a text or a `Content` of parts, which may hold images,
//! audio, video or a PDF. Its usage counts are `total_input_tokens` and
//! `total_output_tokens`, with their `total_tokens`. Its questions are
//! `noul`, `choice` and `score` alone.
//!
//! A `Content` is the kind's state as it is: a provider of another contract
//! is sent it as JSON, its media untranslated. A state of another contract
//! that is no text and no `Content` is sent Gemini as its JSON text.

use respan_gateway_core::{Connection, Fault, Format, Provider};
use respan_gateway_decision::{Input, Output};
use respan_gateway_http::http::Method;
use respan_gateway_http::path::path_ends_with;
use respan_gateway_http::{ClientIo, Connect, KeyIn, ProviderIo, Upstream};
use serde_json::Value;

use crate::{At, Shape, bind_client, bind_provider};

const NAME: &str = "gemini-decisions";

/// Where a client sends one, and, after `/v1beta`, a provider takes one.
const CLIENT_PATH: &str = "/v1beta/decisions";
const PATH: &str = "/decisions";

const SHAPE: Shape = Shape {
    name: NAME,
    state: "input",
    usage: ["total_input_tokens", "total_output_tokens"],
    total: Some("total_tokens"),
    write_state,
};

/// A state as Gemini takes an input: a text or a `Content` as it is, any
/// other JSON as its text.
fn write_state(state: &Value) -> Value {
    match state {
        Value::String(_) => state.clone(),
        Value::Object(content) if content.contains_key("parts") => state.clone(),
        other => Value::String(other.to_string()),
    }
}

/// Gemini's Decisions, as Google serves it: a `POST` to a path ending in
/// `/v1beta/decisions`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GeminiDecisions;

impl GeminiDecisions {
    /// The format's name, `gemini-decisions`, which tags its native fields.
    pub const NAME: &'static str = NAME;
}

impl Format<ClientIo, Input, Output> for GeminiDecisions {
    fn name(&self) -> &str {
        NAME
    }

    fn matches(&self, io: &ClientIo) -> bool {
        let request = io.request();
        request.method() == Method::POST && path_ends_with(request.uri(), CLIENT_PATH)
    }

    fn bind(&self, io: ClientIo) -> Result<Connection<Input, Output>, Fault> {
        bind_client(io, SHAPE)
    }
}

/// Its spec's: the key in `x-goog-api-key`, over HTTP, beside its base URL.
impl Upstream for GeminiDecisions {
    fn name(&self) -> &str {
        NAME
    }

    fn key(&self) -> KeyIn {
        KeyIn::Header("x-goog-api-key")
    }

    fn provider(&self, name: String, connect: Connect) -> Provider {
        Provider::new(name, *self, move |x| connect(x))
    }
}

impl Format<ProviderIo, Output, Input> for GeminiDecisions {
    fn name(&self) -> &str {
        NAME
    }

    fn bind(&self, io: ProviderIo) -> Result<Connection<Output, Input>, Fault> {
        bind_provider(io, SHAPE, At(PATH))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode_request, decode_response, encode_request, encode_response};
    use respan_gateway_decision::Usage;
    use serde_json::json;

    #[test]
    fn a_request_is_read_and_written_back() {
        let raw = json!({
            "model": "gemini-decision-1",
            "input": {"parts": [{"text": "Charged twice"}, {"inline_data": {"mime_type": "image/png", "data": "AAAA"}}]},
            "questions": {
                "spam": {"type": "noul", "instructions": "Is this spam?"},
                "dept": {"type": "choice", "criteria": {"billing": "Money", "other": null}},
            },
        });
        let input = decode_request(&SHAPE, raw.to_string().as_bytes()).expect("input");
        assert_eq!(input.questions.len(), 2);
        assert_eq!(
            Value::Object(encode_request(&SHAPE, &input).expect("call")),
            raw
        );
    }

    #[test]
    fn a_state_that_is_no_content_is_sent_as_its_text() {
        let state = json!({"subject": "Refund"});
        assert_eq!(write_state(&state), json!("{\"subject\":\"Refund\"}"));
        assert_eq!(write_state(&json!("text")), json!("text"));
    }

    #[test]
    fn its_usage_is_counted_and_written_with_its_total() {
        let raw = json!({
            "model": "gemini-decision-1",
            "answers": {"spam": {"type": "noul", "noul": 0.02}},
            "usage": {"total_input_tokens": 120, "total_output_tokens": 0, "total_tokens": 120,
                "input_tokens_by_modality": [{"modality": "text", "tokens": 120}]},
        });
        let mut events = Vec::new();
        decode_response(&SHAPE, &raw, &mut events).expect("events");
        assert!(events.contains(&Output::Usage(Usage::new(120, 0))));
        let written: Value =
            serde_json::from_slice(&encode_response(&SHAPE, &events).expect("body"))
                .unwrap_or_default();
        assert_eq!(written, raw);
        // To a client of System One, the counts are System One's, and what
        // only describes Gemini's usage is left out.
        let written: Value =
            serde_json::from_slice(&encode_response(&crate::SHAPE, &events).expect("body"))
                .unwrap_or_default();
        assert_eq!(
            written["usage"],
            json!({"input_tokens": 120, "output_tokens": 0})
        );
        assert_eq!(written["answers"]["spam"]["noul"], 0.02);
    }
}
