//! Prime Intellect: decisions in its dialect of System One, its Evaluations
//! API.

use respan_gateway_core::Fault;
use respan_gateway_decision::Input;
use respan_gateway_decision_http::{DialectedSystemOne, SystemOneDialect, within};
use serde_json::{Map, Value};

use respan_gateway_vendor_kit::Vendor;

/// Where Prime Inference is.
const BASE_URL: &str = "https://api.pinference.ai/api/v1";

/// Prime Intellect.
pub const VENDOR: Vendor = Vendor {
    id: "primeintellect",
    key_env: &["PRIME_API_KEY"],
    formats: &[(&DialectedSystemOne::new(PrimeEvaluations), Some(BASE_URL))],
    ..Vendor::DEFAULT
};

/// Prime Intellect's evaluations, System One in its own names:
///
/// - A call goes to `/evaluations`.
/// - A yes or no is a `boolean`, asked and answered; its probability of
///   yes is read from `noul`, `boolean` or `probability`, whichever it
///   says.
/// - Its usage counts are `inputTokens` and `outputTokens`.
/// - It answers at most 64 questions a call; more is refused, 400, before
///   the call.
#[derive(Debug, Clone, Copy)]
pub struct PrimeEvaluations;

impl SystemOneDialect for PrimeEvaluations {
    fn path(&self) -> &'static str {
        "/evaluations"
    }

    fn call(&self, body: &mut Map<String, Value>, input: &Input) -> Result<(), Fault> {
        within(input, 64, usize::MAX, "Prime Intellect")?;
        let questions = body.get_mut("questions").and_then(Value::as_object_mut);
        for q in questions.into_iter().flat_map(|qs| qs.values_mut()) {
            if q["type"] == "noul" {
                q["type"] = "boolean".into();
            }
        }
        Ok(())
    }

    fn answer(&self, body: &mut Value) {
        let answers = body.get_mut("answers").and_then(Value::as_object_mut);
        for a in answers.into_iter().flat_map(|a| a.values_mut()) {
            let Some(answer) = a
                .as_object_mut()
                .filter(|a| a.get("type") == Some(&"boolean".into()))
            else {
                continue;
            };
            answer.insert("type".into(), "noul".into());
            let said = ["boolean", "probability"].into_iter();
            let mut said = said.filter_map(|key| answer.remove(key));
            let yes = said.find(Value::is_number);
            if let Some(yes) = yes.filter(|_| !answer.contains_key("noul")) {
                answer.insert("noul".into(), yes);
            }
        }
        let Some(usage) = body.get_mut("usage").and_then(Value::as_object_mut) else {
            return;
        };
        for (theirs, ours) in [
            ("inputTokens", "input_tokens"),
            ("outputTokens", "output_tokens"),
        ] {
            if let Some(count) = usage.remove(theirs) {
                usage.insert(ours.into(), count);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_yes_or_no_and_its_usage_are_in_its_names() {
        let mut body = json!({"questions": {
            "spam": {"type": "noul"}, "tone": {"type": "choice", "criteria": {"a": null}}}});
        let call = body.as_object_mut().expect("an object");
        PrimeEvaluations
            .call(call, &Input::new("s", Vec::new()))
            .expect("call");
        assert_eq!(body["questions"]["spam"]["type"], "boolean");
        assert_eq!(body["questions"]["tone"]["type"], "choice");
        let mut answer = json!({
            "answers": {"spam": {"type": "boolean", "probability": 0.9, "confidence": 0.8}},
            "usage": {"inputTokens": 380, "outputTokens": 0},
        });
        PrimeEvaluations.answer(&mut answer);
        assert_eq!(
            answer,
            json!({
                "answers": {"spam": {"type": "noul", "noul": 0.9, "confidence": 0.8}},
                "usage": {"input_tokens": 380, "output_tokens": 0},
            })
        );
    }
}
