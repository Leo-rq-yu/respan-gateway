use respan_gateway_core::{Fault, NativeFields};
use respan_gateway_http::Encoder;
use respan_gateway_http::bytes::Bytes;
use respan_gateway_realtime::{Audio, AudioFormat, Cause, Input, Output, Session, Turns, Vad};
use serde_json::{Value, json};

use super::{NAME, input, output};

/// What `events` are written as, to a provider.
fn sent(events: &[Input]) -> Result<Vec<Value>, Fault> {
    let mut provider = input::Provider::new("voxtral".into());
    let mut frames = Vec::new();
    for event in events {
        provider.write(event, &mut frames)?;
    }
    Ok(frames
        .iter()
        .map(|f| serde_json::from_slice(f).expect("JSON"))
        .collect())
}

fn outputs(frames: &[Value]) -> Vec<Output> {
    let mut provider = output::Provider;
    let mut events = Vec::new();
    for frame in frames {
        provider
            .read(Bytes::from(frame.to_string()), &mut events)
            .expect("read");
    }
    events
}

fn audio(data: Vec<u8>, rate: u32) -> Input {
    Input::Audio {
        audio: Audio {
            data: Bytes::from(data),
            format: AudioFormat::pcm16(rate),
        },
        native: None,
    }
}

#[test]
fn a_session_names_its_audio_format() {
    let mut session = Session::default();
    session.model = Some("voxtral".into());
    session.input_audio = Some(AudioFormat::pcm16(24_000));
    session.input_transcription = Some(true);
    session.output_transcription = Some(true);
    let mut turns = Vad::default();
    turns.silence_ms = Some(500);
    session.turns = Some(Turns::Server(turns));
    let frames = sent(&[Input::Session(session)]).expect("sent");
    assert_eq!(
        frames,
        [json!({"type": "session.update", "session": {
            "audio_format": {"encoding": "pcm_s16le", "sample_rate": 24000}}})]
    );
}

#[test]
fn a_session_that_says_nothing_is_no_frame() {
    let mut session = Session::default();
    session.model = Some("voxtral".into());
    assert_eq!(
        sent(&[Input::Session(session)]).expect("sent"),
        Vec::<Value>::new()
    );
}

#[test]
fn a_sessions_own_fields_are_kept() {
    let mut session = Session::default();
    let native = json!({"session": {"target_streaming_delay_ms": 240}});
    session.native = Some(NativeFields::new(
        NAME,
        native.as_object().cloned().expect("object"),
    ));
    let frames = sent(&[Input::Session(session)]).expect("sent");
    assert_eq!(
        frames,
        [json!({"type": "session.update", "session": {"target_streaming_delay_ms": 240}})]
    );
}

#[test]
fn what_it_cannot_do_is_refused() {
    let refused = |edit: fn(&mut Session)| {
        let mut session = Session::default();
        edit(&mut session);
        sent(&[Input::Session(session)]).expect_err("refused")
    };
    refused(|s| s.instructions = Some("Be brief.".into()));
    refused(|s| s.voice = Some("alloy".into()));
    refused(|s| s.tools = Some(Vec::new()));
    refused(|s| s.input_transcription = Some(false));
    let mut other = Session::default();
    other.model = Some("other".into());
    let opening = Input::Session(Session::default());
    sent(&[opening, Input::Session(other)]).expect_err("a model is taken at connect");
    let fault = sent(&[Input::Respond { native: None }]).expect_err("refused");
    assert_eq!(fault.code(), "bad_request");
}

#[test]
fn audio_is_appended_in_the_sessions_format_and_in_parts() {
    // Until a session says otherwise, it takes 16 kHz: 24 kHz is resampled.
    let frames = sent(&[audio(vec![0; 4800], 24_000)]).expect("sent");
    assert_eq!(frames[0]["type"], "input_audio.append");
    let decoded = |v: &Value| {
        let data = crate::json::unbase64(&v["audio"]).expect("base64");
        data.len()
    };
    assert!((1..=3200).contains(&decoded(&frames[0])));

    let mut session = Session::default();
    session.input_audio = Some(AudioFormat::pcm16(24_000));
    let big = audio(vec![0; 262_144 + 10], 24_000);
    let frames = sent(&[Input::Session(session), big]).expect("sent");
    let sizes: Vec<_> = frames[1..].iter().map(decoded).collect();
    assert_eq!(sizes, [262_144, 10]);
}

#[test]
fn a_commit_flushes_and_the_end_of_events_ends_the_audio() {
    assert_eq!(
        sent(&[Input::Commit { native: None }]).expect("sent"),
        [json!({"type": "input_audio.flush"})]
    );
    let mut provider = input::Provider::new("voxtral".into());
    let mut frames = Vec::new();
    provider.end(Ok(()), &mut frames).expect("end");
    assert_eq!(
        frames,
        [Bytes::from_static(br#"{"type":"input_audio.end"}"#)]
    );
    let mut frames = Vec::new();
    provider
        .end(Err(Fault::upstream("broke")), &mut frames)
        .expect_err("failed");
    assert!(frames.is_empty());
}

#[test]
fn another_formats_events_are_dropped_or_refused() {
    let dropped = Input::native(
        "openai-realtime",
        json!({"type": "conversation.item.truncate"}),
    );
    assert_eq!(sent(&[dropped]).expect("dropped"), Vec::<Value>::new());
    let refused = Input::native("openai-realtime", json!({"type": "response.create"}));
    assert_eq!(sent(&[refused]).expect_err("refused").code(), "bad_request");
    let own = json!({"type": "input_audio.flush"});
    assert_eq!(
        sent(&[Input::native(NAME, own.clone())]).expect("own"),
        [own]
    );
}

#[test]
fn a_session_is_opened_with_its_model_and_format() {
    let events = outputs(&[json!({"type": "session.created", "session": {
        "request_id": "r1", "model": "voxtral",
        "audio_format": {"encoding": "pcm_s16le", "sample_rate": 16000},
        "target_streaming_delay_ms": 480}})]);
    let [Output::Opened(session)] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(session.model.as_deref(), Some("voxtral"));
    assert_eq!(session.input_audio, Some(AudioFormat::pcm16(16_000)));
    assert_eq!(session.input_transcription, Some(true));
    let kept = session.native.as_ref().expect("native");
    assert_eq!(
        serde_json::Value::Object(kept.fields.clone()),
        json!({"session": {"request_id": "r1", "target_streaming_delay_ms": 480}})
    );

    let updated = outputs(&[json!({"type": "session.updated", "session": {
        "request_id": "r1", "model": "voxtral",
        "audio_format": {"encoding": "pcm_mulaw", "sample_rate": 8000}}})]);
    let [Output::Updated(session)] = updated.as_slice() else {
        panic!("{updated:?}");
    };
    assert_eq!(session.input_audio, Some(AudioFormat::MULAW));
}

#[test]
fn deltas_and_the_end_are_the_users_transcript() {
    let events = outputs(&[
        json!({"type": "transcription.text.delta", "text": "Hel"}),
        json!({"type": "transcription.text.delta", "text": "lo"}),
        json!({"type": "transcription.done", "model": "voxtral", "text": "Hello",
            "language": "en", "usage": {"prompt_tokens": 12, "completion_tokens": 3,
            "total_tokens": 15, "prompt_audio_seconds": 2}}),
    ]);
    let text = |e: &Output| match e {
        Output::InputTranscript { text, complete, .. } => Some((text.clone(), *complete)),
        _ => None,
    };
    assert_eq!(text(&events[0]), Some(("Hel".into(), false)));
    assert_eq!(text(&events[1]), Some(("lo".into(), false)));
    assert_eq!(text(&events[2]), Some(("Hello".into(), true)));
    let Output::Usage(usage) = &events[3] else {
        panic!("{events:?}");
    };
    assert_eq!(usage.tokens.input_tokens, 12);
    assert_eq!(usage.tokens.output_tokens, 3);
    assert_eq!(usage.input_audio_tokens, 12);
    let Output::InputTranscript {
        native: Some(rest), ..
    } = &events[2]
    else {
        panic!("{events:?}");
    };
    assert_eq!(
        Value::Object(rest.fields.clone()),
        json!({"model": "voxtral", "language": "en", "usage": {"prompt_audio_seconds": 2}})
    );
}

#[test]
fn a_done_without_usage_reports_none() {
    let events = outputs(&[json!({"type": "transcription.done", "text": "Hi", "usage": {}})]);
    assert_eq!(events.len(), 1);
}

#[test]
fn an_error_says_its_cause() {
    let events = outputs(&[
        json!({"type": "error", "error": {"message": "bad audio", "code": 422}}),
        json!({"type": "error", "error": {"message": {"detail": "x"}, "code": 500}}),
    ]);
    let [
        Output::Error {
            cause,
            code,
            message,
            ..
        },
        Output::Error {
            cause: second,
            message: object,
            ..
        },
    ] = events.as_slice()
    else {
        panic!("{events:?}");
    };
    assert_eq!(*cause, Cause::Invalid);
    assert_eq!(code.as_deref(), Some("422"));
    assert_eq!(message, "bad audio");
    assert_eq!(*second, Cause::Provider);
    assert_eq!(object, r#"{"detail":"x"}"#);
}

#[test]
fn what_the_kind_has_no_form_for_stays_native() {
    let language = json!({"type": "transcription.language", "audio_language": "en"});
    let segment = json!({"type": "transcription.segment", "text": "a", "start": 0.0, "end": 1.0});
    let events = outputs(&[
        language.clone(),
        segment.clone(),
        json!({"type": "unheard"}),
    ]);
    assert_eq!(events[0], Output::native(NAME, language));
    assert_eq!(events[1], Output::native(NAME, segment));
    assert!(matches!(events[2], Output::Native(_)));
}

#[test]
fn a_frame_that_is_not_json_fails_upstream() {
    let mut events = Vec::new();
    let fault = output::Provider
        .read(Bytes::from_static(b"nope"), &mut events)
        .expect_err("not JSON");
    assert_eq!(fault.code(), "upstream");
}
