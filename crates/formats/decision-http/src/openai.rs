//! OpenAI's Decisions ([`OpenAiDecisions`]): a `POST` to `/decisions`,
//! answered whole in JSON.
//!
//! A request is `{"model", "input", "questions": [question]}`: `input` a
//! text or a list of messages, each question `{"type", "name",
//! "instructions"}`, a `predicate`, a `choice` with `choices`, each
//! `{"value", "description"}`, or a `score` with `levels`, each `{"label",
//! "description"}`. A response is `{"model", "answers": [answer], "usage"}`,
//! the answers in the questions' order, each `{"type", "name", ..}`: a
//! `predicate` with its `probability`, a `choice` with `choice`,
//! `confidence` and `probabilities`, each `{"value", "probability"}`, a
//! `score` with `score` and `probabilities` by level, or a `refusal`.
//!
//! Between it and the kind:
//!
//! - A `predicate` is the kind's yes or no. A yes or no whose sides say
//!   what they mean, as System One's may, is asked with them written after
//!   its instructions, as a predicate has nowhere else to say them.
//! - A question is its answer's id by its `name`; one with none is named by
//!   its place, `"0"` for the first.
//! - A list of messages is the kind's state as it is: a provider of another
//!   contract is sent it as JSON, its images untranslated. A state of
//!   another contract that is no text and no list of messages is sent
//!   OpenAI as its JSON text.
//! - A level that is a text is sent as a level of that `label` and
//!   `description`.
//! - A choice whose values are not all texts keeps them for OpenAI alone: a
//!   provider of another contract is refused it, 400. A `refusal` is
//!   OpenAI's alone too, and fails the response of a client of another
//!   contract.
//! - Of its usage, the kind counts input, output and cached input tokens.

use respan_gateway_conversation_http::fields::{
    invalid, kept, merge_missing, native_of, rest_of, set,
};
use respan_gateway_conversation_http::read_provider_error;
use respan_gateway_core::{Connection, Fault, Format, Provider};
use respan_gateway_decision::{Answer, Ask, Input, Output, Question, Start, Usage, Verdict};
use respan_gateway_http::bytes::Bytes;
use respan_gateway_http::http::Method;
use respan_gateway_http::http::header::CONTENT_TYPE;
use respan_gateway_http::path::path_ends_with;
use respan_gateway_http::{All, ClientIo, Connect, Decoder, KeyIn, ProviderIo, Upstream, Whole};
use serde_json::{Map, Value, json};

use crate::{count, foreign, json_type, model_of, own, post, refusal, to_bytes};

const NAME: &str = "openai-decisions";
const PATH: &str = "/decisions";

/// OpenAI's Decisions, as OpenAI or a compatible vendor serves it: a `POST`
/// to a path ending in `/decisions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenAiDecisions {
    key: KeyIn,
}

impl OpenAiDecisions {
    /// The format's name, `openai-decisions`, which tags its native fields.
    pub const NAME: &'static str = NAME;

    /// OpenAI's Decisions as its spec has it: its key a bearer token.
    pub const fn new() -> Self {
        Self { key: KeyIn::Bearer }
    }

    /// OpenAI's Decisions as a vendor takes its key: alone, in `header`
    /// (lower case), as Azure does in `api-key`.
    pub const fn key_in(header: &'static str) -> Self {
        Self {
            key: KeyIn::Header(header),
        }
    }
}

impl Default for OpenAiDecisions {
    fn default() -> Self {
        Self::new()
    }
}

impl Format<ClientIo, Input, Output> for OpenAiDecisions {
    fn name(&self) -> &str {
        NAME
    }

    /// Any `/decisions` but Gemini's, which is its own format's.
    fn matches(&self, io: &ClientIo) -> bool {
        let request = io.request();
        request.method() == Method::POST
            && path_ends_with(request.uri(), PATH)
            && !path_ends_with(request.uri(), "/v1beta/decisions")
    }

    fn bind(&self, io: ClientIo) -> Result<Connection<Input, Output>, Fault> {
        Ok(io.with_error(refusal).bind(
            |_| {
                Box::new(|raw: Bytes, out: &mut Vec<Input>| {
                    out.push(decode_request(&raw)?);
                    Ok(())
                })
            },
            |reply| {
                let mut head = reply.head();
                head.headers.insert(CONTENT_TYPE, json_type());
                Ok(reply.write(head, Box::new(All::new(|e: &[Output]| encode_response(e)))))
            },
        ))
    }
}

/// Its spec's: a bearer key, over HTTP, beside its base URL; a vendor's key
/// where it takes it ([`OpenAiDecisions::key_in`]).
impl Upstream for OpenAiDecisions {
    fn name(&self) -> &str {
        NAME
    }

    fn key(&self) -> KeyIn {
        self.key
    }

    fn provider(&self, name: String, connect: Connect) -> Provider {
        Provider::new(name, *self, move |x| connect(x))
    }
}

impl Format<ProviderIo, Output, Input> for OpenAiDecisions {
    fn name(&self) -> &str {
        NAME
    }

    fn bind(&self, io: ProviderIo) -> Result<Connection<Output, Input>, Fault> {
        Ok(io.with_refusal(read_provider_error).bind(
            |head, input: Option<&Input>| {
                let input = input.ok_or_else(|| {
                    Fault::config(format!("{NAME} does not accept streaming inputs"))
                })?;
                post(head, PATH, &encode_request(input)?)
            },
            |_, _| Some(Box::new(Whole::new(decode_response)) as Box<dyn Decoder<Output>>),
        ))
    }
}

/// A client's request.
fn decode_request(raw: &[u8]) -> Result<Input, Fault> {
    let Ok(Value::Object(mut map)) = serde_json::from_slice(raw) else {
        return Err(invalid("body", "must be a JSON object"));
    };
    let state = map
        .remove("input")
        .ok_or_else(|| invalid("input", "is required"))?;
    if !(state.is_string() || state.is_array()) {
        return Err(invalid("input", "must be a string or a list of messages"));
    }
    let Some(Value::Array(asked)) = map.remove("questions") else {
        return Err(invalid("questions", "must be a list of questions"));
    };
    let mut questions: Vec<Question> = Vec::with_capacity(asked.len());
    for (i, q) in asked.into_iter().enumerate() {
        let question = decode_question(i, q)?;
        if questions.iter().any(|q| q.id == question.id) {
            let param = format!("questions[{i}].name");
            return Err(invalid(&param, "names a question already asked"));
        }
        questions.push(question);
    }
    let mut input = Input::new(state, questions);
    input.model = model_of(&mut map)?;
    input.native = native_of(NAME, map);
    Ok(input)
}

/// The question at `i`, as `v` says it. What of it the kind's question
/// would not be written back as is kept as OpenAI's own.
fn decode_question(i: usize, v: Value) -> Result<Question, Fault> {
    let param = format!("questions[{i}]");
    let Value::Object(q) = v else {
        return Err(invalid(&param, "must be an object"));
    };
    let id = match &q.get("name") {
        None | Some(Value::Null) => i.to_string(),
        Some(Value::String(name)) => name.clone(),
        Some(_) => return Err(invalid(&format!("{param}.name"), "must be a string")),
    };
    let ask = match q.get("type").and_then(Value::as_str) {
        Some("predicate") => Ask::Noul {
            yes: None,
            no: None,
        },
        Some("choice") => {
            let Some(choices) = q.get("choices").and_then(Value::as_array) else {
                return Err(invalid(&format!("{param}.choices"), "must be a list"));
            };
            let option = |c: &Value| {
                let value = c.as_object()?.get("value").filter(|v| !v.is_null())?;
                let about = c.get("description").filter(|v| !v.is_null()).cloned();
                Some((text_of(value), about))
            };
            let options: Option<Vec<_>> = choices.iter().map(option).collect();
            let options = options.ok_or_else(|| {
                invalid(
                    &format!("{param}.choices"),
                    "must each be an object with a value",
                )
            })?;
            Ask::Choice { options }
        }
        Some("score") => match q.get("levels") {
            Some(Value::Array(levels)) => Ask::Score {
                levels: levels.clone(),
            },
            _ => return Err(invalid(&format!("{param}.levels"), "must be a list")),
        },
        _ => {
            return Err(invalid(
                &param,
                "has a type other than predicate, choice or score",
            ));
        }
    };
    let mut question = Question::new(id, ask);
    question.instructions = q.get("instructions").filter(|v| !v.is_null()).cloned();
    let written = encode_question(&question)?;
    question.native = native_of(NAME, unwritten(&q, &written));
    Ok(question)
}

/// The fields of `said` that `written` does not say as it does: what only
/// the format that read them can write back. A null says nothing.
fn unwritten(said: &Map<String, Value>, written: &Map<String, Value>) -> Map<String, Value> {
    let differs = |(k, v): &(&String, &Value)| !v.is_null() && written.get(*k) != Some(*v);
    let kept = said.iter().filter(differs);
    kept.map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// `v` as a text: itself if it is one, else its JSON.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// A question as OpenAI asks it, by the kind's alone.
fn encode_question(q: &Question) -> Result<Map<String, Value>, Fault> {
    let mut v = Map::new();
    let mut instructions = q.instructions.as_ref().map(text_of);
    match &q.ask {
        Ask::Noul { yes, no } => {
            v.insert("type".into(), "predicate".into());
            for (side, means) in [("True", yes), ("False", no)] {
                if let Some(means) = means {
                    let said = instructions.get_or_insert_default();
                    if !said.is_empty() {
                        said.push('\n');
                    }
                    *said += &format!("{side}: {}", text_of(means));
                }
            }
        }
        Ask::Choice { options } => {
            v.insert("type".into(), "choice".into());
            let choice = |(value, about): &(String, Option<Value>)| {
                let mut c = Map::new();
                c.insert("value".into(), value.clone().into());
                set(&mut c, "description", about.as_ref().map(text_of));
                Value::Object(c)
            };
            v.insert("choices".into(), options.iter().map(choice).collect());
        }
        Ask::Score { levels } => {
            v.insert("type".into(), "score".into());
            let level = |l: &Value| match l {
                Value::Object(level) if level.contains_key("label") => l.clone(),
                other => json!({"label": text_of(other), "description": text_of(other)}),
            };
            v.insert("levels".into(), levels.iter().map(level).collect());
        }
        _ => {
            return Err(Fault::bad_request(format!(
                "{NAME} cannot ask question '{}'",
                q.id
            )));
        }
    }
    v.insert("name".into(), q.id.clone().into());
    set(&mut v, "instructions", instructions);
    Ok(v)
}

/// The call of `input`. A question's own fields are written as OpenAI said
/// them, over the kind's.
fn encode_request(input: &Input) -> Result<Map<String, Value>, Fault> {
    let mut questions = Vec::with_capacity(input.questions.len());
    for q in &input.questions {
        let mut v = encode_question(q)?;
        v.extend(kept(NAME, &q.native)?);
        questions.push(Value::Object(v));
    }
    let messages = |list: &[Value]| !list.is_empty() && list.iter().all(Value::is_object);
    let state = match &input.state {
        Value::String(_) => input.state.clone(),
        Value::Array(list) if messages(list) => input.state.clone(),
        other => Value::String(other.to_string()),
    };
    let mut v = Map::new();
    set(&mut v, "model", input.model.clone());
    v.insert("input".into(), state);
    v.insert("questions".into(), questions.into());
    merge_missing(&mut v, &kept(NAME, &input.native)?);
    Ok(v)
}

/// The events of a provider's whole answer.
fn decode_response(raw: Bytes, out: &mut Vec<Output>) -> Result<(), Fault> {
    let v: Value = serde_json::from_slice(&raw).unwrap_or_default();
    let Some(answers) = v["answers"].as_array() else {
        return Err(Fault::upstream(format!(
            "the provider's success is not a response of {NAME}"
        )));
    };
    let mut start = Start::new(v["model"].as_str().map(Into::into));
    start.native = native_of(NAME, rest_of(&v, &["model", "answers", "usage"]));
    out.push(Output::Start(start));
    for (i, a) in answers.iter().enumerate() {
        match decode_answer(i, a) {
            Some(answer) => out.push(Output::Answer(answer)),
            None => out.push(Output::native(NAME, json!({ "answers": [a] }))),
        }
    }
    let usage = &v["usage"];
    if usage.is_object() {
        let mut counted = Usage::new(
            count(usage, "input_tokens")?,
            count(usage, "output_tokens")?,
        );
        counted.cached_input_tokens = count(&usage["input_tokens_details"], "cached_tokens")?;
        out.push(Output::Usage(counted));
        let extras = unwritten(
            usage.as_object().unwrap_or(&Map::new()),
            &usage_body(&counted),
        );
        if !extras.is_empty() {
            out.push(Output::native(NAME, json!({ "usage": extras })));
        }
    }
    Ok(())
}

/// The answer at `i`, if `a` is of a type the kind has a form for. What of
/// it the kind's answer would not be written back as is kept as OpenAI's
/// own.
fn decode_answer(i: usize, a: &Value) -> Option<Answer> {
    let said = a.as_object()?;
    let probability = |p: &Value| p["probability"].as_f64();
    let verdict = match a["type"].as_str()? {
        "predicate" => Verdict::Noul(a["probability"].as_f64()?),
        "choice" => Verdict::Choice {
            choice: text_of(said.get("choice").filter(|v| !v.is_null())?),
            probabilities: match &a["probabilities"] {
                Value::Array(each) => each
                    .iter()
                    .map(|p| Some((text_of(p.get("value")?), probability(p)?)))
                    .collect::<Option<_>>()?,
                Value::Object(by_value) => by_value
                    .iter()
                    .map(|(value, p)| Some((value.clone(), p.as_f64()?)))
                    .collect::<Option<_>>()?,
                _ => return None,
            },
        },
        "score" => Verdict::Score {
            score: a["score"].as_f64()?,
            probabilities: match &a["probabilities"] {
                Value::Array(each) => each.iter().map(probability).collect::<Option<_>>()?,
                Value::Object(by_level) => (0..by_level.len())
                    .map(|i| by_level.get(&i.to_string())?.as_f64())
                    .collect::<Option<_>>()?,
                _ => return None,
            },
        },
        _ => return None,
    };
    let id = match &a["name"] {
        Value::String(name) => name.clone(),
        _ => i.to_string(),
    };
    let mut answer = Answer::new(id, verdict);
    answer.confidence = a["confidence"].as_f64();
    let written = encode_answer(&answer).ok()?;
    answer.native = native_of(NAME, unwritten(said, &written));
    Some(answer)
}

/// An answer as OpenAI writes it, by the kind's alone.
fn encode_answer(answer: &Answer) -> Result<Map<String, Value>, Fault> {
    let mut v = Map::new();
    v.insert("name".into(), answer.id.clone().into());
    match &answer.verdict {
        Verdict::Noul(p) => {
            v.insert("type".into(), "predicate".into());
            v.insert("probability".into(), (*p).into());
        }
        Verdict::Choice {
            choice,
            probabilities,
        } => {
            v.insert("type".into(), "choice".into());
            v.insert("choice".into(), choice.clone().into());
            let each = probabilities
                .iter()
                .map(|(value, p)| json!({"value": value, "probability": p}));
            v.insert("probabilities".into(), each.collect());
        }
        Verdict::Score {
            score,
            probabilities,
        } => {
            v.insert("type".into(), "score".into());
            v.insert("score".into(), (*score).into());
            let each = probabilities.iter().enumerate();
            let each = each.map(|(level, p)| json!({"value": level, "probability": p}));
            v.insert("probabilities".into(), each.collect());
        }
        _ => {
            return Err(Fault::new(format!(
                "{NAME} cannot write answer '{}'",
                answer.id
            )));
        }
    }
    set(&mut v, "confidence", answer.confidence);
    Ok(v)
}

/// The kind's usage as OpenAI counts it.
fn usage_body(usage: &Usage) -> Map<String, Value> {
    let total = usage.input_tokens.saturating_add(usage.output_tokens);
    let body = json!({
        "input_tokens": usage.input_tokens,
        "input_tokens_details": {"cached_tokens": usage.cached_input_tokens},
        "output_tokens": usage.output_tokens,
        "total_tokens": total,
    });
    body.as_object().cloned().unwrap_or_default()
}

/// A whole response of `events`. An answer's own fields, and its usage's,
/// are written as OpenAI said them, over the kind's.
fn encode_response(events: &[Output]) -> Result<Bytes, Fault> {
    let (mut body, mut answers, mut usage, mut extras) = (Map::new(), Vec::new(), None, Map::new());
    for event in events {
        match event {
            Output::Start(start) => {
                set(&mut body, "model", start.model.clone());
                merge_missing(&mut body, &own(NAME, &start.native));
            }
            Output::Answer(answer) => {
                let mut v = encode_answer(answer)?;
                v.extend(own(NAME, &answer.native));
                answers.push(Value::Object(v));
            }
            Output::Usage(u) => *usage.get_or_insert_with(Usage::default) += u,
            Output::Native(n) if &*n.format != NAME => foreign(NAME, n)?,
            Output::Native(n) => {
                if let Some(more) = n.value["answers"].as_array() {
                    answers.extend(more.iter().cloned());
                }
                if let Some(more) = n.value["usage"].as_object() {
                    extras.extend(more.clone());
                }
            }
            _ => {}
        }
    }
    body.insert("answers".into(), answers.into());
    if let Some(usage) = usage {
        let mut u = usage_body(&usage);
        u.extend(extras);
        body.insert("usage".into(), u.into());
    }
    to_bytes(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use respan_gateway_core::FaultKind;

    fn written(events: &[Output]) -> Value {
        serde_json::from_slice(&encode_response(events).expect("body")).unwrap_or_default()
    }

    #[test]
    fn a_request_of_each_question_type_is_read_and_written_back() {
        let raw = json!({
            "model": "gpt-6-luna",
            "input": [{"role": "user", "content": [
                {"type": "input_text", "text": "Charged twice"},
                {"type": "input_image", "image_url": "data:image/png;base64,AAAA"},
            ]}],
            "questions": [
                {"type": "predicate", "name": "spam", "instructions": "Is this spam?"},
                {"type": "choice", "name": "dept", "choices": [
                    {"value": "billing", "description": "Money"}, {"value": "other"}]},
                {"type": "score", "name": "urgency", "instructions": "How urgent?", "levels": [
                    {"label": "Low", "description": "Can wait"}, {"label": "High", "description": "Today"}]},
            ],
            "metadata": {"ticket": "1"},
        });
        let input = decode_request(raw.to_string().as_bytes()).expect("input");
        assert_eq!(input.model(), Some("gpt-6-luna"));
        let ids: Vec<&str> = input.questions.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, ["spam", "dept", "urgency"]);
        assert!(input.questions.iter().all(|q| q.native.is_none()));
        assert!(matches!(&input.questions[1].ask,
            Ask::Choice { options } if options[0] == ("billing".into(), Some("Money".into()))
                && options[1].1.is_none()));
        assert_eq!(Value::Object(encode_request(&input).expect("call")), raw);
    }

    #[test]
    fn a_choice_of_typed_values_is_kept_as_openai_said_it() {
        let raw = json!({"input": "s", "questions": [
            {"type": "choice", "choices": [{"value": true}, {"value": "true"}]}]});
        let input = decode_request(raw.to_string().as_bytes()).expect("input");
        assert_eq!(input.questions[0].id, "0");
        assert!(input.questions[0].native.is_some());
        let call = encode_request(&input).expect("call");
        assert_eq!(
            call["questions"][0]["choices"],
            raw["questions"][0]["choices"]
        );
        assert_eq!(call["questions"][0]["name"], "0");
    }

    #[test]
    fn a_request_that_is_not_one_is_refused() {
        let refused = |raw: Value| {
            decode_request(raw.to_string().as_bytes()).is_err_and(|f| f.is(FaultKind::Client))
        };
        assert!(refused(json!({"questions": []})));
        assert!(refused(json!({"input": {"x": 1}, "questions": []})));
        assert!(refused(json!({"input": "s", "questions": {}})));
        assert!(refused(
            json!({"input": "s", "questions": [{"type": "noul"}]})
        ));
        assert!(refused(
            json!({"input": "s", "questions": [{"type": "choice", "choices": ["a"]}]})
        ));
        assert!(refused(json!({"input": "s", "questions": [
            {"type": "predicate", "name": "a"}, {"type": "predicate", "name": "a"}]})));
    }

    #[test]
    fn what_system_one_asks_is_asked_as_openai_can() {
        let mut noul = Question::new(
            "spam",
            Ask::Noul {
                yes: Some("Unsolicited".into()),
                no: None,
            },
        );
        noul.instructions = Some("Is this spam?".into());
        let score = Question::new(
            "urgency",
            Ask::Score {
                levels: vec!["Low".into(), json!({"label": "High"})],
            },
        );
        let input = Input::new(json!({"subject": "Refund"}), vec![noul, score]);
        let call = Value::Object(encode_request(&input).expect("call"));
        assert_eq!(
            call,
            json!({
                "input": "{\"subject\":\"Refund\"}",
                "questions": [
                    {"type": "predicate", "name": "spam",
                        "instructions": "Is this spam?\nTrue: Unsolicited"},
                    {"type": "score", "name": "urgency", "levels": [
                        {"label": "Low", "description": "Low"}, {"label": "High"}]},
                ],
            })
        );
    }

    #[test]
    fn a_response_is_written_back_as_it_came() {
        let raw = json!({
            "id": "dec_1",
            "object": "decision",
            "model": "gpt-6-luna",
            "answers": [
                {"type": "predicate", "name": "spam", "probability": 0.98},
                {"type": "refusal", "name": "dept"},
                {"type": "choice", "name": "tone", "choice": "angry", "confidence": 0.9,
                    "probabilities": [{"value": "angry", "probability": 0.8}, {"value": "calm", "probability": 0.2}]},
                {"type": "score", "name": "urgency", "score": 1.1, "probabilities": [
                    {"value": 0, "label": "Low", "probability": 0.1},
                    {"value": 1, "label": "Mid", "probability": 0.7},
                    {"value": 2, "label": "High", "probability": 0.2}]},
            ],
            "usage": {"input_tokens": 42, "input_tokens_details": {"cached_tokens": 8, "cache_write_tokens": 0},
                "output_tokens": 0, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 42},
        });
        let mut events = Vec::new();
        decode_response(Bytes::from(raw.to_string()), &mut events).expect("events");
        let usage = events.iter().find_map(|e| match e {
            Output::Usage(u) => Some(*u),
            _ => None,
        });
        assert_eq!(
            usage.map(|u| (u.input_tokens, u.cached_input_tokens)),
            Some((42, 8))
        );
        assert!(events.iter().any(|e| matches!(e,
            Output::Answer(Answer { verdict: Verdict::Score { probabilities, .. }, .. })
                if probabilities == &[0.1, 0.7, 0.2])));
        assert_eq!(written(&events), raw);
    }

    #[test]
    fn another_contracts_answers_are_written_as_openais() {
        let mut choice = Answer::new(
            "tone",
            Verdict::Choice {
                choice: "angry".into(),
                probabilities: vec![("angry".into(), 0.8), ("calm".into(), 0.2)],
            },
        );
        choice.confidence = Some(0.9);
        choice.native = native_of("typesafe-systemone", rest_of(&json!({"x": 1}), &[]));
        let events = [
            Output::Start(Start::new(Some("jev-1.13.0".into()))),
            Output::Answer(Answer::new("spam", Verdict::Noul(0.98))),
            Output::Answer(choice),
            Output::Usage(Usage::new(42, 0)),
            Output::native("typesafe-systemone", json!({"usage": {"cached": 1}})),
        ];
        assert_eq!(
            written(&events),
            json!({
                "model": "jev-1.13.0",
                "answers": [
                    {"type": "predicate", "name": "spam", "probability": 0.98},
                    {"type": "choice", "name": "tone", "choice": "angry", "confidence": 0.9,
                        "probabilities": [{"value": "angry", "probability": 0.8}, {"value": "calm", "probability": 0.2}]},
                ],
                "usage": {"input_tokens": 42, "input_tokens_details": {"cached_tokens": 0},
                    "output_tokens": 0, "total_tokens": 42},
            })
        );
        let odd = Output::native(
            "typesafe-systemone",
            json!({"answers": {"q": {"type": "box"}}}),
        );
        assert!(encode_response(&[odd]).is_err());
    }
}
