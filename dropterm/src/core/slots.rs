//! The typed-slot contract (typed-slots initiative, slot-contract
//! spec): the closed answer vocabulary every slot surface shares —
//! plain text today, styled spans as the additive upgrade — the
//! fail-loud validation rules, and the id-carrying event payload the
//! interactive slots report back with. Pure data and pure validation:
//! no toolkit dependency (the core/bin split, pinned by
//! `tests/core_is_toolkit_free.rs`), no port code — the parse at the
//! Lua boundary lives with [`crate::core::lua_host`], the render with
//! the band.
//!
//! The contract's shape is closed: unknown keys, wrong types, and
//! malformed colors each cost the answer one [`SlotError`] — the
//! caller's bug, one log line, the answer discarded — and every
//! vocabulary change is additive (new keys join as new releases;
//! existing keys never change meaning). A wrong shape never raises
//! into the terminal.

use serde::{Deserialize, Serialize};

/// The styled answer's span budget — the evolution constant capping
/// one answer's spans so a runaway script cannot bloat the band's
/// layout pass. Picked once at the contract's birth (spec ruling:
/// 16); raising it is an additive release.
pub const MAX_SPANS_PER_ANSWER: usize = 16;

/// The icon vocabulary's source note: the status band's existing
/// Lucide names — the set `segment_icon` already maps (`core/status.rs`),
/// not a new catalog. A name outside it renders nothing (the band's
/// degrade), never an error: the contract accepts any string and the
/// painter decides.
pub const ICON_VOCABULARY: &str = "the status band's Lucide names — segment_icon (core/status.rs)";

/// The interactive-slot bus event's name: one event, a `kind` field
/// in the payload (spec open-question 3's ruling) — per-slot event
/// names never join the bus.
pub const SLOT_EVENT: &str = "slot-event";

/// One slot color: a theme token name or a `#rrggbb` literal
/// (spec open-question 1's ruling). Resolution is the painter's job
/// (`styled-status-slot`), never the contract's — an unknown token
/// name degrades at render, exactly like an unknown icon name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotColor {
    /// A theme token name — the theme's own vocabulary, carried
    /// verbatim.
    Token(String),
    /// A `#rrggbb` literal, validated at parse (`#` + exactly six hex
    /// digits), carried verbatim (the painter re-parses at render).
    Hex(String),
}

impl SlotColor {
    /// Parse one raw color string: a leading `#` demands exactly
    /// `#rrggbb`; anything else is a theme token, accepted as held.
    pub fn parse(raw: &str) -> Result<Self, SlotError> {
        let malformed = raw.starts_with('#')
            && !(raw.len() == 7 && raw[1..].bytes().all(|b| b.is_ascii_hexdigit()));
        if malformed {
            return Err(SlotError::BadColor(raw.to_owned()));
        }
        Ok(if raw.starts_with('#') {
            SlotColor::Hex(raw.to_owned())
        } else {
            SlotColor::Token(raw.to_owned())
        })
    }

    /// The color's wire form: the string it was parsed from.
    pub fn as_str(&self) -> &str {
        match self {
            SlotColor::Token(token) => token,
            SlotColor::Hex(hex) => hex,
        }
    }
}

/// The color crosses the wire as its raw string — the vocabulary's
/// one-byte-per-value shape, token and hex alike.
impl Serialize for SlotColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SlotColor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        SlotColor::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// One span of styled slot content. Closed keyset: new keys join as
/// additive releases, existing keys never change meaning
/// (lua-declarative-ui-precedents, Neovim's contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// The span's text — the only part the contract guarantees to
    /// render (the styled band concatenates span texts).
    pub text: String,
    /// Theme token name or `#rrggbb`; resolution is the painter's
    /// job (`styled-status-slot`), never the contract's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<SlotColor>,
    /// A Lucide icon name from the band's existing vocabulary
    /// ([`ICON_VOCABULARY`]); rendered before the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// One slot's typed answer. `Text` is the v1 shape and stays valid
/// forever; `Styled` is the additive upgrade. Untagged serde: a text
/// answer crosses the wire as a bare string — today's exact bytes —
/// and a styled one as its `kind`/`spans` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SlotContent {
    /// The plain-string shape every existing script returns.
    Text(String),
    /// The styled shape: an ordered flat span list — flat only, no
    /// nesting (spec open-question 2's ruling).
    Styled { spans: Vec<Span> },
}

impl SlotContent {
    /// The answer's text under this module's contract: a text answer
    /// is its own text; a styled answer renders the span texts
    /// concatenated in order, icons and colors ignored — the painter
    /// upgrade (`styled-status-slot`) is theirs. The band's
    /// byte-identical pin rides this: every existing script's text
    /// answer flattens to exactly the string it returned.
    pub fn plain_text(&self) -> String {
        match self {
            SlotContent::Text(text) => text.clone(),
            SlotContent::Styled { spans } => spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
        }
    }
}

/// One slot answer's failure, typed. Every variant is a script bug
/// costing exactly one log line and the answer — never a raise into
/// the terminal (the house one-log-line culture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotError {
    /// A key outside the closed vocabulary — in a span or on the
    /// content table.
    UnknownKey(String),
    /// A known key held the wrong shape (a number for `text`, an
    /// object for `icon`, …). The payload is the key's name.
    WrongType(&'static str),
    /// The styled answer's spans array stood empty.
    EmptySpans,
    /// More spans than [`MAX_SPANS_PER_ANSWER`] allows.
    TooManySpans,
    /// A `#`-prefixed color that is not exactly `#rrggbb`.
    BadColor(String),
    /// The content table's `kind`, when present, is not `styled`.
    BadKind(String),
    /// The styled shape's `spans` key stood missing.
    MissingSpans,
}

impl std::fmt::Display for SlotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlotError::UnknownKey(key) => write!(f, "unknown key '{key}'"),
            SlotError::WrongType(key) => write!(f, "the '{key}' key has the wrong type"),
            SlotError::EmptySpans => write!(f, "the spans array is empty"),
            SlotError::TooManySpans => {
                write!(f, "more than {MAX_SPANS_PER_ANSWER} spans")
            }
            SlotError::BadColor(color) => write!(
                f,
                "the color '{color}' is neither a theme token nor #rrggbb"
            ),
            SlotError::BadKind(kind) => write!(f, "the kind '{kind}' is not 'styled'"),
            SlotError::MissingSpans => write!(f, "the spans key is missing"),
        }
    }
}

/// Parse one slot answer's typed payload from its serde form — the
/// object a script returned for a styled answer (the `kind`/`spans`
/// table, the port's own `side` word already lifted off), or a bare
/// string for a text answer. Shape errors surface typed; the parsed
/// content is validated ([`validate`]) before it returns, so a green
/// parse is a green contract.
pub fn parse_content(value: &serde_json::Value) -> Result<SlotContent, SlotError> {
    match value {
        serde_json::Value::String(text) => Ok(SlotContent::Text(text.clone())),
        serde_json::Value::Object(map) => {
            const CONTENT_KEYS: [&str; 2] = ["kind", "spans"];
            for key in map.keys() {
                if !CONTENT_KEYS.contains(&key.as_str()) {
                    return Err(SlotError::UnknownKey(key.clone()));
                }
            }
            if let Some(kind) = map.get("kind") {
                let kind = kind.as_str().ok_or(SlotError::WrongType("kind"))?;
                if kind != "styled" {
                    return Err(SlotError::BadKind(kind.to_owned()));
                }
            }
            let Some(spans) = map.get("spans") else {
                return Err(SlotError::MissingSpans);
            };
            let Some(spans) = spans.as_array() else {
                return Err(SlotError::WrongType("spans"));
            };
            let mut parsed = Vec::with_capacity(spans.len().min(MAX_SPANS_PER_ANSWER));
            for span in spans {
                parsed.push(parse_span(span)?);
            }
            let content = SlotContent::Styled { spans: parsed };
            validate(&content)?;
            Ok(content)
        }
        _ => Err(SlotError::WrongType("content")),
    }
}

/// Parse one span object against the closed keyset: `text` required
/// and string, `color`/`icon` string when present, nothing else.
fn parse_span(span: &serde_json::Value) -> Result<Span, SlotError> {
    const SPAN_KEYS: [&str; 3] = ["text", "color", "icon"];
    let Some(map) = span.as_object() else {
        return Err(SlotError::WrongType("span"));
    };
    for key in map.keys() {
        if !SPAN_KEYS.contains(&key.as_str()) {
            return Err(SlotError::UnknownKey(key.clone()));
        }
    }
    let text = map
        .get("text")
        .and_then(serde_json::Value::as_str)
        .ok_or(SlotError::WrongType("text"))?;
    let color = match map.get("color") {
        Some(raw) => Some(SlotColor::parse(
            raw.as_str().ok_or(SlotError::WrongType("color"))?,
        )?),
        None => None,
    };
    let icon = match map.get("icon") {
        Some(raw) => Some(raw.as_str().ok_or(SlotError::WrongType("icon"))?.to_owned()),
        None => None,
    };
    Ok(Span {
        text: text.to_owned(),
        color,
        icon,
    })
}

/// Validate one slot answer against the contract's semantic rules —
/// the gate every [`SlotContent`] crosses, at the port (parse-built)
/// and for hand-built content (the future Rust-side slots). Shape
/// errors cannot exist on a typed [`SlotContent`]; what remains is
/// the span budget, the empty list, and the hex form.
pub fn validate(content: &SlotContent) -> Result<(), SlotError> {
    let SlotContent::Styled { spans } = content else {
        return Ok(());
    };
    if spans.is_empty() {
        return Err(SlotError::EmptySpans);
    }
    if spans.len() > MAX_SPANS_PER_ANSWER {
        return Err(SlotError::TooManySpans);
    }
    for span in spans {
        if let Some(SlotColor::Hex(hex)) = span.color.as_ref() {
            // Re-derive through the parser: the hex form is a
            // validated shape, hand-built payloads included.
            SlotColor::parse(hex)?;
        }
    }
    Ok(())
}

/// One interactive slot's report, riding [`SLOT_EVENT`] over the bus:
/// which surface's slot, which element inside its content, which
/// interaction. The payload's four keys are closed — a handler
/// receives exactly these and nothing else (the headless proof is
/// this module's event-path test).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotEvent {
    /// The slot surface that produced the event (`"status"`,
    /// `"dialog"`, … — additive as surfaces land).
    pub surface: String,
    /// The slot's registration id — the segment name, the dialog id.
    pub slot: String,
    /// The element inside the slot's content the interaction hit.
    pub element_id: String,
    /// The interaction kind — the consumer surfaces grow their own
    /// closed sets (`press`, `dismiss`, …) as they land.
    pub kind: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::lua_host::{HostEvent, LuaHost};
    use serde_json::json;
    use std::fmt::Write as _;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    /// A per-test scratch directory under the system temp dir with a
    /// process- and test-unique name, removed (best-effort) on drop.
    /// Std only, the lua-host tests' own shape — the event fixture
    /// needs a plugins dir and nothing else.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let unique = format!(
                "dropterm-slots-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("create scratch dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    /// The bus port every fixture script opens first.
    const PORT: &str = "local d = require('dropterm')";

    /// The event fixture's handler: one registered `slot-event`
    /// listener that verifies the payload's key count and every
    /// documented value in Lua — the no-extras half the Rust-side
    /// four-keys pin cannot see — and prints its verdict, forwarded
    /// one line per print.
    const LISTENER: &str = "\
d.on('slot-event', function(p)
  local n = 0
  for _ in pairs(p) do n = n + 1 end
  if n == 4
     and p.surface == 'status'
     and p.slot == 'net'
     and p.element_id == 'toggle'
     and p.kind == 'press' then
    print('slot-event payload exact')
  else
    print('slot-event payload wrong: count=' .. n)
  end
end)";

    /// Poll the host's outbound stream via the tick's own
    /// [`LuaHost::try_event`] drain until a log line matching
    /// `wanted` lands (the match included) or the deadline passes —
    /// the query surfaces' wait-for-the-reply rule, no silence
    /// heuristic.
    fn log_until(host: &LuaHost, wanted: impl Fn(&str) -> bool, timeout: Duration) -> Vec<String> {
        let deadline = Instant::now() + timeout;
        let mut lines = Vec::new();
        loop {
            match host.try_event() {
                Some(HostEvent::Log(line)) => {
                    let hit = wanted(&line);
                    lines.push(line);
                    if hit {
                        return lines;
                    }
                }
                Some(_) => {}
                None => {
                    if Instant::now() >= deadline {
                        return lines;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    // --- the byte-compat pin ---

    #[test]
    fn a_string_answer_serializes_to_todays_exact_bytes() {
        // Today's `text: String` crossed as a bare JSON string; the
        // untagged Text variant must produce the identical bytes or
        // every future serializer of the widened field breaks the
        // wire.
        let bytes = serde_json::to_vec(&SlotContent::Text("cwd is /home/u/src".to_owned()))
            .expect("a text answer serializes");
        assert_eq!(
            bytes,
            b"\"cwd is /home/u/src\"".as_slice(),
            "the string shape serializes as a bare string, today's exact bytes"
        );
    }

    #[test]
    fn a_string_answer_round_trips_through_serde() {
        let content = SlotContent::Text("branch ok".to_owned());
        let bytes = serde_json::to_vec(&content).expect("serializes");
        let back: SlotContent =
            serde_json::from_slice(&bytes).expect("the bare string parses back");
        assert_eq!(back, content);
    }

    #[test]
    fn a_styled_answer_round_trips_through_serde() {
        let content = SlotContent::Styled {
            spans: vec![
                Span {
                    text: " 1.2 MB/s".to_owned(),
                    color: Some(SlotColor::parse("accent").expect("a token parses")),
                    icon: Some("arrow-down".to_owned()),
                },
                Span {
                    text: "↑".to_owned(),
                    color: Some(SlotColor::parse("#1f9e7b").expect("a hex parses")),
                    icon: None,
                },
            ],
        };
        let bytes = serde_json::to_vec(&content).expect("serializes");
        let back: SlotContent =
            serde_json::from_slice(&bytes).expect("the styled shape parses back");
        assert_eq!(back, content);
    }

    #[test]
    fn absent_span_keys_serialize_absent_not_null() {
        // The Lua vocabulary's shape: an unset color/icon is an
        // absent key, never a null — the handler-facing shape the
        // docs pin.
        let bytes = serde_json::to_vec(&Span {
            text: "x".to_owned(),
            color: None,
            icon: None,
        })
        .expect("a bare span serializes");
        assert_eq!(bytes, br#"{"text":"x"}"#.as_slice());
    }

    // --- SlotColor ---

    #[test]
    fn a_theme_token_and_a_hex_color_both_parse() {
        assert_eq!(
            SlotColor::parse("accent").expect("a token parses"),
            SlotColor::Token("accent".to_owned())
        );
        assert_eq!(
            SlotColor::parse("#1f9e7b").expect("a hex parses"),
            SlotColor::Hex("#1f9e7b".to_owned())
        );
    }

    #[test]
    fn a_malformed_hex_is_rejected() {
        // Only `#`-prefixed strings carry a syntax rule: exactly
        // `#rrggbb`. Anything else is a theme token, carried verbatim
        // — an unknown token degrades at the painter, like an
        // unknown icon name.
        for raw in ["#gg", "#12", "#12345", "#1234567", "#"] {
            let error = SlotColor::parse(raw).expect_err("a malformed color rejects");
            assert_eq!(error, SlotError::BadColor(raw.to_owned()));
        }
    }

    #[test]
    fn a_color_round_trips_as_its_raw_string() {
        for raw in ["accent", "#1f9e7b"] {
            let color = SlotColor::parse(raw).expect("parses");
            let bytes = serde_json::to_vec(&color).expect("serializes");
            assert_eq!(
                bytes,
                format!("\"{raw}\"").into_bytes().as_slice(),
                "the color crosses as its own string"
            );
        }
    }

    // --- the closed keyset ---

    #[test]
    fn an_unknown_span_key_yields_unknown_key() {
        let error = parse_content(&json!({
            "kind": "styled",
            "spans": [{ "text": "x", "bogus": 1 }],
        }))
        .expect_err("an unknown span key rejects");
        assert_eq!(error, SlotError::UnknownKey("bogus".to_owned()));
    }

    #[test]
    fn an_unknown_content_key_yields_unknown_key() {
        let error = parse_content(&json!({
            "kind": "styled",
            "spans": [{ "text": "x" }],
            "bogus": true,
        }))
        .expect_err("an unknown content key rejects");
        assert_eq!(error, SlotError::UnknownKey("bogus".to_owned()));
    }

    #[test]
    fn a_non_string_text_yields_wrong_type() {
        let error = parse_content(&json!({
            "kind": "styled",
            "spans": [{ "text": 3 }],
        }))
        .expect_err("a numeric text rejects");
        assert_eq!(error, SlotError::WrongType("text"));
    }

    #[test]
    fn a_wrong_shaped_color_and_icon_yield_wrong_type() {
        let error = parse_content(&json!({
            "kind": "styled",
            "spans": [{ "text": "x", "color": ["accent"] }],
        }))
        .expect_err("a non-string color rejects");
        assert_eq!(error, SlotError::WrongType("color"));
        let error = parse_content(&json!({
            "kind": "styled",
            "spans": [{ "text": "x", "icon": { "name": "x" } }],
        }))
        .expect_err("a non-string icon rejects");
        assert_eq!(error, SlotError::WrongType("icon"));
    }

    #[test]
    fn a_span_that_is_not_an_object_yields_wrong_type() {
        let error = parse_content(&json!({
            "kind": "styled",
            "spans": ["plain"],
        }))
        .expect_err("a non-object span rejects");
        assert_eq!(error, SlotError::WrongType("span"));
    }

    #[test]
    fn a_missing_or_wrong_shaped_spans_array_yields_wrong_type() {
        let error =
            parse_content(&json!({ "kind": "styled" })).expect_err("a missing spans key rejects");
        assert_eq!(error, SlotError::MissingSpans);
        let error = parse_content(&json!({ "spans": "x" })).expect_err("a non-array spans rejects");
        assert_eq!(error, SlotError::WrongType("spans"));
    }

    #[test]
    fn a_non_string_non_object_content_yields_wrong_type() {
        for raw in [json!(42), json!(true), json!([1, 2])] {
            let error = parse_content(&raw).expect_err("a non-content shape rejects");
            assert_eq!(error, SlotError::WrongType("content"), "for {raw}");
        }
    }

    // --- the semantic rules ---

    #[test]
    fn empty_spans_yield_empty_spans() {
        let error = validate(&SlotContent::Styled { spans: vec![] })
            .expect_err("an empty spans list rejects");
        assert_eq!(error, SlotError::EmptySpans);
        let error = parse_content(&json!({ "spans": [] })).expect_err("and at the parse");
        assert_eq!(error, SlotError::EmptySpans);
    }

    #[test]
    fn too_many_spans_yield_too_many_spans() {
        assert_eq!(MAX_SPANS_PER_ANSWER, 16, "the picked budget stands");
        let spans: Vec<Span> = (0..MAX_SPANS_PER_ANSWER as u64)
            .map(|n| Span {
                text: n.to_string(),
                color: None,
                icon: None,
            })
            .collect();
        assert!(
            validate(&SlotContent::Styled {
                spans: spans.clone()
            })
            .is_ok(),
            "exactly the budget validates"
        );
        let error = validate(&SlotContent::Styled {
            spans: spans
                .into_iter()
                .chain(std::iter::once(Span {
                    text: "over".to_owned(),
                    color: None,
                    icon: None,
                }))
                .collect(),
        })
        .expect_err("one past the budget rejects");
        assert_eq!(error, SlotError::TooManySpans);
    }

    #[test]
    fn a_bad_hex_on_a_hand_built_span_yields_bad_color() {
        // Hex is a validated form, but the enum's payloads stay
        // constructible — validate is the gate that catches a hand
        // built bad literal.
        let content = SlotContent::Styled {
            spans: vec![Span {
                text: "x".to_owned(),
                color: Some(SlotColor::Hex("#gg".to_owned())),
                icon: None,
            }],
        };
        let error = validate(&content).expect_err("a bad hex rejects");
        assert_eq!(error, SlotError::BadColor("#gg".to_owned()));
    }

    #[test]
    fn a_bad_kind_yields_bad_kind() {
        let error = parse_content(&json!({
            "kind": "fancy",
            "spans": [{ "text": "x" }],
        }))
        .expect_err("an unknown kind rejects");
        assert_eq!(error, SlotError::BadKind("fancy".to_owned()));
    }

    // --- the valid shapes ---

    #[test]
    fn the_both_shapes_validate() {
        assert_eq!(
            validate(&SlotContent::Text("fine".to_owned())),
            Ok(()),
            "a text answer is always valid"
        );
        assert_eq!(
            validate(&SlotContent::Styled {
                spans: vec![Span {
                    text: " 1.2 MB/s".to_owned(),
                    color: Some(SlotColor::parse("accent").expect("parses")),
                    icon: Some("arrow-down".to_owned()),
                }]
            }),
            Ok(()),
            "a well-formed styled answer validates"
        );
    }

    #[test]
    fn a_bare_string_parses_as_a_text_answer() {
        assert_eq!(
            parse_content(&json!("cwd is /home/u/src")).expect("parses"),
            SlotContent::Text("cwd is /home/u/src".to_owned())
        );
    }

    #[test]
    fn a_styled_payload_parses_to_the_typed_content() {
        let content = parse_content(&json!({
            "kind": "styled",
            "spans": [
                { "icon": "arrow-down", "text": " 1.2 MB/s", "color": "accent" },
            ],
        }))
        .expect("the spec's example parses");
        assert_eq!(
            content,
            SlotContent::Styled {
                spans: vec![Span {
                    text: " 1.2 MB/s".to_owned(),
                    color: Some(SlotColor::Token("accent".to_owned())),
                    icon: Some("arrow-down".to_owned()),
                }]
            }
        );
    }

    // --- the event path (the headless proof, slot-contract SL3) ---

    /// The documented payload crosses the existing emit arm: a
    /// registered `dropterm.on('slot-event', …)` handler receives
    /// exactly the four documented keys, no extras, values intact —
    /// proven in Lua, where the key count is observable. The first
    /// producer is `dialog-slot`; this task ships the contract's
    /// proof, nothing UI-side, no new inbound host arm.
    #[test]
    fn the_slot_event_crosses_the_bus_with_exactly_its_documented_payload() {
        let dir = TempDir::new("slot-event");
        fs::write(dir.0.join("listener.lua"), format!("{PORT}\n{LISTENER}"))
            .expect("the fixture writes");
        let host = LuaHost::new(&dir.0);
        let event = SlotEvent {
            surface: "status".to_owned(),
            slot: "net".to_owned(),
            element_id: "toggle".to_owned(),
            kind: "press".to_owned(),
        };
        // The existing LuaHostHandle::emit arm — the event-hooks
        // precedent, no new inbound host command.
        host.handle().emit(
            SLOT_EVENT,
            serde_json::to_value(&event).expect("the payload serializes"),
        );
        let lines = log_until(
            &host,
            |line| line.contains("slot-event payload"),
            Duration::from_secs(5),
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("slot-event payload exact")),
            "the handler must receive exactly the four documented keys with their values: {lines:?}"
        );
    }

    // --- the render's flatten (the band's byte-compat pin) ---

    #[test]
    fn plain_text_concatenates_the_span_texts_and_keeps_a_text_whole() {
        assert_eq!(
            SlotContent::Text("cwd is /home/u/src".to_owned()).plain_text(),
            "cwd is /home/u/src",
            "a text answer is its own text"
        );
        assert_eq!(
            SlotContent::Styled {
                spans: vec![
                    Span {
                        text: "▼".to_owned(),
                        color: Some(SlotColor::Token("accent".to_owned())),
                        icon: Some("arrow-down".to_owned()),
                    },
                    Span {
                        text: " 1.2 MB/s".to_owned(),
                        color: None,
                        icon: None,
                    },
                ],
            }
            .plain_text(),
            "▼ 1.2 MB/s",
            "the span texts concatenate in order; icons and colors wait for the painter"
        );
    }

    // --- the event payload ---

    #[test]
    fn the_slot_event_serializes_exactly_its_four_keys() {
        let event = SlotEvent {
            surface: "status".to_owned(),
            slot: "net".to_owned(),
            element_id: "toggle".to_owned(),
            kind: "press".to_owned(),
        };
        let value = serde_json::to_value(&event).expect("serializes");
        let keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["surface", "slot", "element_id", "kind"]);
        let back: SlotEvent = serde_json::from_value(value).expect("round-trips");
        assert_eq!(back, event);
    }

    // --- the vocabulary pin (the every_config_key_is_documented
    // pattern, slot-contract SL4) ---

    /// The contract's documented vocabulary: every key the Rust
    /// types declare, both `SlotColor` forms, and the `slot-event`
    /// name — a key added without its docs row fails the pin until
    /// the row follows. The backticked form is required, so a bare
    /// word elsewhere in the prose cannot satisfy the check.
    const DOCUMENTED_KEYS: [&str; 12] = [
        // the answer table's words
        "kind",
        "spans",
        "side",
        // the span's words
        "text",
        "color",
        "icon",
        // the color's two forms: a theme token's example and the hex escape
        "accent",
        "#rrggbb",
        // the slot-event's name and payload words
        "slot-event",
        "surface",
        "slot",
        "element_id",
    ];

    /// The typed-answers section of docs/plugins.md, split to its own
    /// H2 — a key named like prose elsewhere in the file cannot
    /// satisfy the check from outside the contract's own section.
    fn typed_answers_section() -> &'static str {
        let docs = include_str!("../../../docs/plugins.md");
        let (_, rest) = docs
            .split_once("## Typed answers")
            .expect("docs/plugins.md carries the typed-answers section");
        rest.split_once("\n## ")
            .map_or(rest, |(section, _)| section)
    }

    /// The first documented key the section lacks, if any.
    fn undocumented_key(section: &str) -> Option<&'static str> {
        DOCUMENTED_KEYS
            .iter()
            .find(|key| !section.contains(&format!("`{key}`")))
            .copied()
    }

    #[test]
    fn every_slot_vocabulary_key_is_documented() {
        let section = typed_answers_section();
        assert_eq!(
            undocumented_key(section),
            None,
            "a vocabulary key ships undocumented in docs/plugins.md's typed-answers section"
        );
    }

    /// The spec's own Lua example, verbatim — the docs teach the
    /// shape the contract parses, and the pin holds the two together.
    const SPEC_EXAMPLE: &str = "\
dropterm.status_segment('net', function(ctx)
  return { kind = 'styled', spans = {
    { icon = 'arrow-down', text = ' 1.2 MB/s', color = 'accent' },
  } }
end)";

    #[test]
    fn the_docs_carry_the_spec_example_verbatim() {
        assert!(
            typed_answers_section().contains(SPEC_EXAMPLE),
            "the spec's Lua example must appear verbatim in the typed-answers section"
        );
    }

    #[test]
    fn the_pin_catches_an_undocumented_key() {
        // The pin's own teeth, demonstrated once on a fixture: a
        // section carrying every key but `icon` fails the same
        // checker the real docs pass.
        let mut fixture = String::new();
        for key in DOCUMENTED_KEYS {
            if key != "icon" {
                let _ = writeln!(fixture, "`{key}` row");
            }
        }
        assert_eq!(
            undocumented_key(&fixture),
            Some("icon"),
            "the checker must find a key the fixture omits"
        );
    }
}
