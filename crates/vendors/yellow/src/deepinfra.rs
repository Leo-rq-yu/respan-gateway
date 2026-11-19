//! DeepInfra: Chat Completions, images and embeddings, as OpenAI serves
//! them, and decisions in its dialect of System One.

use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_core::Fault;
use respan_gateway_decision::Input;
use respan_gateway_decision_http::{DialectedSystemOne, SystemOneDialect, within};
use respan_gateway_media_http::openai::{OpenAiEmbeddings, OpenAiImages};
use serde_json::{Map, Value};

use respan_gateway_vendor_kit::Vendor;

/// Where its OpenAI-compatible API is.
const BASE_URL: &str = "https://api.deepinfra.com/v1/openai";

/// Where its decisions are: beside its OpenAI-compatible API, not under it.
const DECISIONS: &str = "https://api.deepinfra.com/v1";

/// DeepInfra. An edit is OpenAI's form, as it takes one.
pub const VENDOR: Vendor = Vendor {
    id: "deepinfra",
    key_env: &["DEEPINFRA_API_KEY"],
    formats: &[
        (&OpenAiChat, Some(BASE_URL)),
        (&OpenAiImages, Some(BASE_URL)),
        (&OpenAiEmbeddings, Some(BASE_URL)),
        (
            &DialectedSystemOne::new(DeepInfraDecisions),
            Some(DECISIONS),
        ),
    ],
    ..Vendor::DEFAULT
};

/// DeepInfra's System One: at `/decisions`, of at most 64 questions a call
/// and 52 options a choice, fewer than other vendors take. More is refused,
/// 400, before the call, naming the limit.
#[derive(Debug, Clone, Copy)]
pub struct DeepInfraDecisions;

impl SystemOneDialect for DeepInfraDecisions {
    fn path(&self) -> &'static str {
        "/decisions"
    }

    fn call(&self, _: &mut Map<String, Value>, input: &Input) -> Result<(), Fault> {
        within(input, 64, 52, "DeepInfra")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use respan_gateway_decision::{Ask, Question};

    #[test]
    fn more_than_it_takes_is_refused_before_the_call() {
        let noul = |i: usize| {
            let ask = Ask::Noul {
                yes: None,
                no: None,
            };
            Question::new(i.to_string(), ask)
        };
        let call = |input: &Input| DeepInfraDecisions.call(&mut Map::new(), input);
        assert!(call(&Input::new("s", (0..64).map(noul).collect::<Vec<_>>())).is_ok());
        assert!(call(&Input::new("s", (0..65).map(noul).collect::<Vec<_>>())).is_err());
        let options = (0..53).map(|i| (i.to_string(), None)).collect();
        let choice = Question::new("c", Ask::Choice { options });
        assert!(call(&Input::new("s", vec![choice])).is_err());
    }
}
