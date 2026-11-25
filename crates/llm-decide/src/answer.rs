//! Reading a model's answer as the one JSON value it ended on: the judge's verdict and the
//! classifier's label on its two model paths.
//!
//! An answer is taken only when it is whole and has one reading. [`whole`] reads an answer given
//! as text and returns `None` for every answer that is cut off, split, mixed with something
//! else, or that a JSON reader would have to choose a reading of. [`call`] reads an answer given
//! as the arguments of a tool call and returns `None` for a call of a turn that did not stop for
//! it, for more than one call and for a call beside a refusal.

use llm_core::{Item, StopReason, TurnOutcome};
use serde::de::{DeserializeSeed, Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

/// The outcome's answer as JSON, as it arrived and with nothing repaired, or `None` when it is
/// not one whole, unambiguous JSON value:
///
/// - the turn did not end normally (the output limit, an incomplete answer);
/// - the outcome has no assistant text or more than one, or anything else beside it, such as a
///   refusal (state the provider keeps, [`Item::Opaque`], is not part of the answer; routing
///   has already failed a call whose outcome holds a tool call the request did not publish). An
///   empty or whitespace-only text item counts as one, so an answer beside it is not taken;
/// - the text is not one JSON value with nothing but JSON whitespace around it, or nests
///   deeper than [`MAX_DEPTH`];
/// - an object gives a key twice, at any depth;
/// - a number written without a fraction or an exponent is outside what `i64` and `u64` hold,
///   so it would be rounded to a double.
///
/// Every other number is read as JSON readers read it: an integer exactly, except `-0`, which
/// is the double minus zero; a number with a fraction or an exponent as the nearest double; one
/// no double holds is not read.
pub(crate) fn whole(outcome: &TurnOutcome) -> Option<Value> {
    if outcome.stop_reason != StopReason::EndTurn {
        return None;
    }
    let mut answer = None;
    for item in &outcome.items {
        match item {
            Item::AssistantText { text } if answer.is_none() => answer = Some(text.as_str()),
            Item::Opaque { .. } => {}
            _ => return None,
        }
    }
    let text = answer?;
    let mut reader = serde_json::Deserializer::from_str(text);
    reader.disable_recursion_limit();
    let value = Strict(MAX_DEPTH).deserialize(&mut reader).ok()?;
    reader.end().ok()?;
    integers_are_exact(scan::Scanner::new(text)).then_some(value)
}

/// The arguments of the outcome's one call to the tool `name`, as the protocol decoder parsed
/// them, or `None` when the outcome does not carry exactly that:
///
/// - the turn did not stop for its tool call (the output limit, an incomplete answer), so the
///   call may not be all the model meant to say;
/// - the outcome has no tool call, more than one, or one to another tool;
/// - a refusal stands beside the call.
///
/// Assistant text and state the provider keeps ([`Item::Opaque`]) beside the call are not part of
/// the answer. A key the model gave twice in the arguments is not seen here: they arrive parsed.
pub(crate) fn call<'a>(outcome: &'a TurnOutcome, name: &str) -> Option<&'a Value> {
    if outcome.stop_reason != StopReason::ToolCalls {
        return None;
    }
    let mut arguments = None;
    for item in &outcome.items {
        match item {
            Item::ToolCall(call) if arguments.is_none() && call.name.as_str() == name => {
                arguments = Some(&call.arguments);
            }
            Item::ToolCall(_) | Item::AssistantRefusal { .. } => return None,
            _ => {}
        }
    }
    arguments
}

/// The deepest an answer nests: an array or an object inside another is one level, a scalar is
/// none. The bound is this reader's own; the JSON reader's is switched off, so the two cannot
/// differ.
const MAX_DEPTH: usize = 128;

/// Reads any JSON value into a [`Value`], refusing an object that gives a key twice and an array
/// or object opened with no level left. It holds the levels that may still open.
#[derive(Clone, Copy)]
struct Strict(usize);

impl Strict {
    /// The reader for what an array or object holds, or the refusal of one opened past the bound.
    /// Refusing here, before anything inside is read, is what bounds the recursion.
    fn inside<E: serde::de::Error>(self) -> Result<Self, E> {
        self.0
            .checked_sub(1)
            .map(Self)
            .ok_or_else(|| E::custom("nested deeper than the bound"))
    }
}

impl<'de> DeserializeSeed<'de> for Strict {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("a JSON value that gives no key twice and nests no deeper than the bound")
    }
    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("a number no double holds"))
    }
    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Value, A::Error> {
        let inside = self.inside()?;
        let mut read = Vec::new();
        while let Some(item) = items.next_element_seed(inside)? {
            #[cfg(test)]
            work::charge(1);
            read.push(item);
        }
        Ok(Value::Array(read))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<Value, A::Error> {
        let inside = self.inside()?;
        let mut read = Map::new();
        while let Some(name) = members.next_key::<String>()? {
            #[cfg(test)]
            work::charge(1);
            let member = members.next_value_seed(inside)?;
            if read.insert(name, member).is_some() {
                return Err(A::Error::custom("an object gives a key twice"));
            }
        }
        Ok(Value::Object(read))
    }
}

/// Whether every number of the text `scanner` stands at the start of, written without a
/// fraction or an exponent, is one `i64` or `u64` holds. The text is JSON that was read, so
/// outside a string a digit or a minus sign starts a number. It reads the text through the
/// scanner it is handed and in no other way: it holds no other reference to the text.
fn integers_are_exact(mut scanner: scan::Scanner<'_>) -> bool {
    let starts_a_token = |each: char| each == '"' || each == '-' || each.is_ascii_digit();
    loop {
        scanner.skip_while(|each: char| !starts_a_token(each));
        // A number stands here when this skip passes anything: a string opens with a quote and
        // the text ends on nothing, and neither is a char of a number.
        let literal = scanner.skip_while(|each: char| {
            matches!(each, '-' | '+' | '.' | 'e' | 'E') || each.is_ascii_digit()
        });
        if literal.is_empty() {
            if scanner.next_char().is_none() {
                return true;
            }
            // Past the quote that opens a string: on to the quote that closes it.
            loop {
                match scanner.next_char() {
                    Some('\\') => {
                        scanner.next_char();
                    }
                    Some('"') | None => break,
                    Some(_) => {}
                }
            }
        } else {
            let is_integer = !literal.contains(['.', 'e', 'E']);
            if is_integer && literal.parse::<i64>().is_err() && literal.parse::<u64>().is_err() {
                return false;
            }
        }
    }
}

/// The one way the scan of an answer's text reads it.
///
/// The rule: the text of an answer is read through a [`Scanner`] only, and the scanner charges
/// every char it visits. The charge is made in test builds, to the counter `work`.
///
/// What the types hold:
///
/// - The scanner keeps the text it has not passed in a field private to this module and no
///   method hands that text out, so its user sees a char or a slice only once the scanner has
///   passed and charged it.
/// - `whole` builds the one scanner of an answer and hands it to `integers_are_exact`, which is
///   given no other reference to the text.
/// - A slice a skip returns borrows from the scanner and not from the text, so the next call on
///   the scanner ends it: its user cannot keep a slice for a later skip or a later turn of its
///   loop and read it again there.
///
/// What stays a rule in prose: a slice was charged once, when it was skipped, and reading it
/// again before the next call on the scanner is its user's cost and is not charged. That cost is
/// of the same order as long as the user reads each slice a fixed number of times and copies
/// none into a value of its own, which would outlive the borrow.
mod scan {
    /// The text of an answer from where the scan stands to its end.
    pub(super) struct Scanner<'a> {
        rest: &'a str,
    }

    impl<'a> Scanner<'a> {
        /// A scanner that stands at the start of `text`.
        pub(super) fn new(text: &'a str) -> Self {
            Self { rest: text }
        }

        /// Passes the chars from here on that `holds` is true of and returns them. It charges
        /// one unit for each byte it passes and one for the look that ends it, at the first
        /// char `holds` is false of or at the end of the text. What it returns borrows from the
        /// scanner, so it cannot be read after the next call on the scanner.
        pub(super) fn skip_while(&mut self, holds: impl Fn(char) -> bool) -> &str {
            let after = self.rest.trim_start_matches(holds);
            // What was passed is cut off by its length, a subtraction: a skip costs what it
            // passes and never the length of the text that follows.
            let (skipped, _) = self.rest.split_at(self.rest.len() - after.len());
            #[cfg(test)]
            super::work::charge(skipped.len() + 1);
            self.rest = after;
            skipped
        }

        /// Passes one char and returns it, or `None` at the end of the text. It charges one
        /// unit either way.
        pub(super) fn next_char(&mut self) -> Option<char> {
            #[cfg(test)]
            super::work::charge(1);
            let mut chars = self.rest.chars();
            let next = chars.next();
            self.rest = chars.as_str();
            next
        }
    }
}

/// The work of the reader in units, counted in test builds only, so that a case holds the cost
/// of reading an answer to its length without reading a clock. Two places charge it:
///
/// - the [`Strict`] visitor, one unit for each element and each member it takes;
/// - [`scan::Scanner`], for every char it visits, as each of its methods says.
///
/// [`integers_are_exact`] charges nothing itself. It is handed the scanner [`whole`] builds and
/// no other reference to the text (the rule is written at [`scan`]), so it sees no char the
/// scanner has not charged. The count is kept per thread, so a case reads what its own calls
/// were charged.
///
/// What the count sees: how often the visitor takes an element or a member, and every char the
/// scanner passes. What it does not see: work inside one visit of the visitor that grows with
/// what was read before (the array so far read again, a key compared with every key read so
/// far), work inside the scanner's two methods, a slice read again after its skip, and what
/// `serde_json` does between two calls of the visitor. A reader that is slow in one of these
/// ways gives the two cases below the same counts as one that is not.
#[cfg(test)]
mod work {
    use std::cell::Cell;

    thread_local! {
        static DONE: Cell<usize> = const { Cell::new(0) };
    }

    /// Adds `units` to the work of this thread's reader.
    pub(super) fn charge(units: usize) {
        DONE.set(DONE.get() + units);
    }

    /// What `read` returns, and the units this thread's reader was charged while it ran.
    pub(super) fn of<T>(read: impl FnOnce() -> T) -> (T, usize) {
        DONE.set(0);
        let returned = read();
        (returned, DONE.get())
    }
}

#[cfg(test)]
mod tests {
    use super::{call, whole, work};
    use llm_core::{
        CallId, Id, Item, StopReason, ToolCall, ToolName, TurnObservation, TurnOutcome,
    };
    use llm_routing::Catalog;
    use serde_json::{Value, json};

    fn arguments() -> Value {
        json!({"label": "positive"})
    }

    /// A turn that stopped for its one call, `call-1`, to the tool `name`.
    fn outcome_calling(name: &str) -> TurnOutcome {
        let catalog = Catalog::parse(include_str!("../../../examples/catalog.toml")).unwrap();
        let serving = Id::new("remote-large").unwrap();
        let provenance = catalog.binding(&serving).unwrap().provenance().clone();
        TurnOutcome {
            stop_reason: StopReason::ToolCalls,
            items: vec![Item::ToolCall(ToolCall {
                call_id: CallId::new("call-1").unwrap(),
                name: ToolName::new(name).unwrap(),
                arguments: arguments(),
            })],
            observation: TurnObservation::new(provenance),
        }
    }

    /// The routed call fails a turn that calls a tool other than the one it forced, so no
    /// classifier is handed such an outcome; only a direct reading shows that [`call`] holds the
    /// name itself.
    #[test]
    fn a_call_to_another_tool_is_not_the_answer() {
        assert_eq!(
            call(&outcome_calling("classify"), "classify"),
            Some(&arguments())
        );
        assert_eq!(call(&outcome_calling("label"), "classify"), None);
    }

    /// The routed call fails a turn that holds a call and says it ended normally, so again only
    /// a direct reading shows that [`call`] holds the stop reason itself.
    #[test]
    fn a_call_of_a_turn_that_says_it_ended_normally_is_not_the_answer() {
        let ended = TurnOutcome {
            stop_reason: StopReason::EndTurn,
            ..outcome_calling("classify")
        };
        assert_eq!(call(&ended, "classify"), None);
    }

    /// A turn that ended normally on the one assistant text `text`.
    fn outcome_saying(text: &str) -> TurnOutcome {
        TurnOutcome {
            stop_reason: StopReason::EndTurn,
            items: vec![Item::assistant(text)],
            ..outcome_calling("classify")
        }
    }

    /// The units of work [`whole`] is charged for reading the answer `text`, which it must read
    /// as `value`.
    fn work_to_read(text: &str, value: &Value) -> usize {
        let outcome = outcome_saying(text);
        let (read, units) = work::of(|| whole(&outcome));
        // Not `assert_eq`: a failure would print both values, megabytes of them.
        let read_as_written = read.as_ref() == Some(value);
        assert!(
            read_as_written,
            "an answer of {} bytes is not read as the value it writes",
            text.len()
        );
        units
    }

    /// An answer is read in time proportional to its length. The far side chooses the answer, and
    /// a stream may carry megabytes of it, so a cost that grows with the square of the length
    /// lets one answer hold the caller's thread: four times the numbers must cost less than
    /// eight times the work. The work is counted ([`work`]) and no clock is read, so a loaded
    /// host gets the same two counts.
    #[test]
    fn a_long_answer_of_numbers_is_read_in_time_proportional_to_its_length() {
        let read = |count: usize| {
            let numbers = format!("[{}1]", "1,".repeat(count - 1));
            work_to_read(&numbers, &Value::Array(vec![json!(1); count]))
        };
        let (short, long) = (read(20_000), read(80_000));
        eprintln!("20000 numbers took {short} units of work, 80000 numbers took {long}");
        assert!(
            long < short * 8,
            "20000 numbers took {short} units of work, 80000 numbers took {long}"
        );
    }

    /// An answer is read in time proportional to its length, whatever it is made of: four times
    /// the answer must cost less than eight times the work, counted as in the case above.
    #[test]
    fn an_answer_of_any_make_is_read_in_time_proportional_to_its_length() {
        // (what the answer is made of, one member as the answer writes it). The literal of an
        // `enum` is no row: only a schema tells `{"b":4.0}` from the row "objects", and this
        // case checks none.
        let makes = [
            ("integers", "-12345"),
            ("doubles", "0.5"),
            ("strings with escapes", r#""a\"b\\""#),
            ("objects", r#"{"a":1.0}"#),
            ("nested arrays", "[[[1]]]"),
            ("whitespace", " \n\t 7 \r\n "),
        ];
        for (make, member) in makes {
            let one: Value = serde_json::from_str(member).unwrap();
            let read = |count: usize| {
                let list = format!("[{}{member}]", format!("{member},").repeat(count - 1));
                work_to_read(&list, &Value::Array(vec![one.clone(); count]))
            };
            let (short, long) = (read(25_000), read(100_000));
            eprintln!("{make}: 25000 members took {short} units of work, 100000 members {long}");
            assert!(
                long < short * 8,
                "{make}: 25000 members took {short} units of work, 100000 members took {long}"
            );
        }
        // One long string and one object of many keys, the two answers that are not a list.
        let string = |length: usize| {
            let text = format!(r#""{}""#, r"a\n".repeat(length / 3));
            work_to_read(&text, &Value::String("a\n".repeat(length / 3)))
        };
        let keys = |count: usize| {
            let members: Vec<String> = (0..count)
                .map(|index| format!(r#""k{index:07}":{index}"#))
                .collect();
            let object = (0..count)
                .map(|index| (format!("k{index:07}"), Value::from(index)))
                .collect();
            let text = format!("{{{}}}", members.join(","));
            work_to_read(&text, &Value::Object(object))
        };
        for (make, short, long) in [
            ("one string", string(300_000), string(1_200_000)),
            ("one object", keys(25_000), keys(100_000)),
        ] {
            eprintln!("{make}: {short} units of work, at four times the size {long}");
            assert!(
                long < short * 8,
                "{make}: {short} units of work, at four times the size {long}"
            );
        }
    }
}
