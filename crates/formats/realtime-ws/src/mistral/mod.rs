//! Mistral's realtime transcription over WebSocket.

mod input;
mod output;
mod session;
#[cfg(test)]
mod tests;

use respan_gateway_core::Provider;
use respan_gateway_core::{Connection, Fault, Format};
use respan_gateway_http::ProviderIo;
use respan_gateway_http::bytes::Bytes;
use respan_gateway_http::raw::Original;
use respan_gateway_http::{Connect, Reach, Upstream};
use respan_gateway_realtime::{Input, Output};
use respan_gateway_ws::WsTransport;

use crate::session::{Spec, model_query};
use crate::{SessionModel, requested_model};

const NAME: &str = SPEC.name;

const SPEC: Spec = Spec {
    name: "mistral-realtime",
    path: "/audio/transcriptions/realtime",
    protocol: None,
    binary_text: false,
};

/// Mistral's realtime transcription: a session at `/audio/transcriptions/realtime`
/// after the base URL its API takes (`wss://api.mistral.ai/v1`), the model
/// at `?model=`, the key a bearer token. A provider format alone: no client
/// speaks it.
///
/// Its events are the kind's where the kind has a form for them: the
/// session and its audio format, the user's audio, a flush as a commit, the
/// transcript's deltas and its end, with the usage the end reports, and
/// errors. What the kind has no form for, such as a detected language or a
/// segment, stays native. It transcribes and says nothing else: a message,
/// a tool's answer, a response to make or cancel, and a session that asks
/// for speech, instructions or tools are refused. The end of the client's
/// events tells it the audio ended.
///
/// A call dials the model routing chose ([`SessionModel`](crate::SessionModel)),
/// else the one at the client's `?model=`; with neither it is refused,
/// `bad_request`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MistralRealtime;

impl MistralRealtime {
    /// The format's name, `mistral-realtime`, which tags its native events.
    pub const NAME: &'static str = SPEC.name;
    /// The path it is sent to.
    pub const PATH: &'static str = SPEC.path;

    /// A transport to its provider at `base_url` ([`WsTransport::new`]).
    pub fn transport(&self, base_url: impl Into<String>) -> Result<WsTransport, Fault> {
        SPEC.transport(base_url)
    }
}

/// Its spec's: a bearer key, as a WebSocket session.
impl Upstream for MistralRealtime {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn reach(&self) -> Reach {
        Reach::Session {
            binary_text: SPEC.binary_text,
        }
    }

    fn provider(&self, name: String, connect: Connect) -> Provider {
        Provider::new(name, *self, move |x| connect(x))
    }
}

impl Format<ProviderIo, Output, Input> for MistralRealtime {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn bind(&self, io: ProviderIo) -> Result<Connection<Output, Input>, Fault> {
        // The model routing chose, else the one the client's handshake asks for.
        let chosen = io.extensions().get::<SessionModel>().map(|m| m.0.clone());
        let asked = || requested_model(io.extensions().get::<Original>()?);
        let model = chosen.or_else(asked).ok_or_else(|| {
            Fault::bad_request(
                "Mistral realtime takes its model at connect: name it with ?model= on the session's URL",
            )
        })?;
        let query = Some(model_query(&model));
        let write = Box::new(input::Provider::new(model));
        let mut provider = output::Provider;
        let read = move |raw: Bytes, out: &mut Vec<Output>| provider.read(raw, out);
        Ok(SPEC.provider(io, query, write, Box::new(read), None))
    }
}
