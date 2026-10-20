# Decision formats

`respan-gateway-decision-http` is the decision kind over HTTP.

| Format | Endpoint | Served by |
| --- | --- | --- |
| `typesafe-systemone` (`SystemOne`) | `POST /v1/systemone` | TypeSafe (`vendor = "typesafe"`), or any compatible base URL |
| `decisions` (`Decisions`) | `POST /v1/decisions` | Any base URL that serves the Decisions API |

Both carry the same body, so a request read by one is written by the other.
`response_body` gives the JSON body either format answers a client with for a
call's events, for a host that logs it.

A request is `{"model", "state", "questions": {id: question}}`, each question
`{"type", "instructions", "criteria"}`: `noul` with criteria
`{"true", "false"}`, `choice` with `{option: description or null}`, `score`
with a list of levels. A response is `{"model", "answers": {id: answer},
"usage": {"input_tokens", "output_tokens"}}`, answered whole in JSON.

A question of another type is refused, 400, before any provider is called.
An answer of a type the kind has no form for is kept and written back where
it was. A refusal is `{"error": {"message", "type", "code"}}`, whose message
TypeSafe's SDKs read.
