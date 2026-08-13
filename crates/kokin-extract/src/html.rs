//! The HTML parsing core: bytes in, observations out.
//!
//! Deliberately free of database and filesystem access, so the hostile corpus
//! can be pointed straight at it. Everything that makes extraction dangerous —
//! unbounded documents, malformed markup, content that is trying to look like
//! markup it is not — is a property of this module, and is tested here.
//!
//! # Why streaming
//!
//! A DOM parser turns a 500 MB document into several gigabytes of nodes before
//! the first value can be read. `lol_html` is a streaming tokeniser with
//! CSS-selector handlers, so memory stays bounded by the largest single token
//! rather than by the document. That is the difference between "we refuse
//! documents over some size" and "we can read them", and evidence that is
//! refused for being large is evidence an investigation does not get.
//!
//! # Why byte ranges are the locator
//!
//! ADR-0006 requires every observation to point at where it came from. A byte
//! range into the artifact is the strongest available form: it is exact,
//! stable for the lifetime of the artifact (which is immutable and
//! content-addressed), and needs no interpretation to verify. A CSS path would
//! depend on the reader reconstructing the same tree we did.

use lol_html::html_content::{Element, TextChunk, TextType};
use lol_html::{element, text, HtmlRewriter, MemorySettings, Settings};
use std::cell::RefCell;
use std::rc::Rc;

use crate::{ExtractError, Result};

/// Observation kinds this extractor can emit.
///
/// Namespaced strings rather than an enum, because `observation.kind` is a
/// queried column shared with future extractors, and a closed Rust enum would
/// not be able to represent a kind written by an older version of the code.
pub const KIND_TITLE: &str = "page.title";
pub const KIND_LINK: &str = "link.href";
pub const KIND_EMAIL: &str = "email.address";
pub const KIND_DOMAIN: &str = "domain.mention";
/// The visible prose of the document, in chunks. See [`Limits::max_text_chunk_bytes`].
pub const KIND_TEXT: &str = "page.text";
/// Meta tags are `meta:<key>`, where key is the `property` or `name` attribute.
pub const KIND_META_PREFIX: &str = "meta:";

/// Bounds on what one document may produce.
///
/// These exist because the input is hostile by assumption. Every one of them
/// has a matching test in the hostile corpus.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Refuse a document that would produce more observations than this.
    ///
    /// Refusing rather than truncating: a silently truncated extraction is
    /// indistinguishable from a complete one, and an analyst would have no way
    /// to know the page held more than they were shown.
    pub max_observations: usize,
    /// Longest value that may be recorded, in bytes.
    pub max_value_bytes: usize,
    /// Ceiling on the parser's internal buffering.
    ///
    /// `lol_html` defaults this to `usize::MAX`. That default is fine for a
    /// CDN rewriting its customers' pages and wrong for us: a single
    /// unterminated tag would otherwise buffer the entire document.
    pub max_parser_memory_bytes: usize,
    /// Roughly how much prose goes into one `page.text` observation.
    ///
    /// A whole page cannot be one observation: it would exceed
    /// [`Limits::max_value_bytes`] on anything substantial, and a locator
    /// spanning the entire document would route a search hit to "somewhere in
    /// here", which is not a route at all. Chunking gives each piece of prose
    /// its own byte range.
    ///
    /// The trade-off is that a phrase straddling a chunk boundary is not
    /// findable as a phrase. Larger chunks make that rarer and locators
    /// coarser. 4 KiB is a few paragraphs.
    pub max_text_chunk_bytes: usize,
    /// Total prose indexed from one document, in bytes.
    ///
    /// Needed because prose is the one thing here whose volume is unbounded by
    /// the document's *structure*: half a gigabyte of paragraphs holds no more
    /// links or titles than a small page, but it holds half a gigabyte of
    /// text. Without this, chunking it would produce hundreds of thousands of
    /// observations and trip [`Limits::max_observations`], and a document that
    /// used to be read would start being refused outright.
    ///
    /// Prose past this point is **counted and reported**, never dropped in
    /// silence — see [`Extraction::text_truncated_bytes`]. It is truncated
    /// rather than refused, unlike the observation cap, because the two
    /// failures are not alike: too many discrete facts means a truncated fact
    /// list that reads as complete, whereas prose is a search aid, and
    /// refusing the document over it would also throw away every link, title
    /// and address in it.
    pub max_text_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_observations: 10_000,
            max_value_bytes: 8 * 1024,
            max_parser_memory_bytes: 4 * 1024 * 1024,
            max_text_chunk_bytes: 4 * 1024,
            // At 4 KiB chunks this is at most ~1024 text observations, leaving
            // most of the observation cap for the document's actual structure.
            max_text_bytes: 4 * 1024 * 1024,
        }
    }
}

/// Where in the artifact an observation came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locator {
    /// The selector that matched, so a reader knows what was being looked for.
    pub selector: &'static str,
    /// Byte offset of the start of the matching token, in the artifact.
    pub start: usize,
    /// Byte offset of the end of the matching token.
    pub end: usize,
}

impl Locator {
    /// The stored form. Tagged with a kind so a later locator scheme (a page
    /// and bounding box, a frame offset) can be told apart from this one
    /// without guessing from the shape of the object.
    pub fn to_json(&self) -> String {
        format!(
            r#"{{"kind":"html_byte_range","selector":"{}","start":{},"end":{}}}"#,
            self.selector, self.start, self.end
        )
    }
}

/// One extracted value, before it has an id or a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawObservation {
    pub kind: String,
    pub value: String,
    pub locator: Locator,
}

/// The result of reading one document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extraction {
    pub observations: Vec<RawObservation>,
    /// Values that exceeded [`Limits::max_value_bytes`] and were not recorded.
    ///
    /// Counted rather than dropped in silence: the count reaches the audit log
    /// and the caller, so "this page had more in it" is always visible even
    /// though the value itself is not stored.
    pub skipped_oversize: usize,
    /// Bytes of prose past [`Limits::max_text_bytes`] that were not indexed.
    ///
    /// Non-zero means this document holds text that search cannot find. That
    /// is the most dangerous kind of gap in an investigation tool - a search
    /// returns nothing and the analyst concludes the term is not there - so it
    /// is reported to the caller and written to the audit log rather than
    /// being a silent property of a large page.
    pub text_truncated_bytes: usize,
}

#[derive(Default)]
struct State {
    observations: Vec<RawObservation>,
    skipped_oversize: usize,
    title: Option<TextSpan>,
    title_emitted: bool,
    /// The text node currently arriving, still in source form.
    text_node: Option<TextSpan>,
    /// Decoded prose accumulated across text nodes, waiting to be emitted.
    pending_text: Option<TextSpan>,
    text_indexed_bytes: usize,
    text_truncated_bytes: usize,
    limit_hit: bool,
}

/// Text that arrives in several chunks, accumulated with the byte range it
/// spans in the artifact.
struct TextSpan {
    text: String,
    start: usize,
    end: usize,
}

impl State {
    fn push(&mut self, limits: &Limits, kind: String, value: String, locator: Locator) {
        if value.is_empty() {
            return;
        }
        if value.len() > limits.max_value_bytes {
            self.skipped_oversize += 1;
            return;
        }
        if self.observations.len() >= limits.max_observations {
            self.limit_hit = true;
            return;
        }
        self.observations.push(RawObservation {
            kind,
            value,
            locator,
        });
    }
}

/// Sink for the rewriter's output, which we do not want.
///
/// Extraction is read-only: the artifact is immutable and already stored, so
/// there is nothing to write back. Discarding the output is what keeps this a
/// single pass with no copy of the document.
struct DiscardOutput;

impl lol_html::OutputSink for DiscardOutput {
    fn handle_chunk(&mut self, _chunk: &[u8]) {}
}

/// A push-based HTML extractor.
///
/// Push rather than pull so it can be handed straight to
/// [`kokin_blob::BlobStore::read_to`], which streams and verifies a blob
/// chunk by chunk. Pulling would mean either buffering the document or running
/// the decryption on another thread.
pub struct HtmlExtractor {
    rewriter: HtmlRewriter<'static, DiscardOutput>,
    state: Rc<RefCell<State>>,
    limits: Limits,
}

impl HtmlExtractor {
    pub fn new(limits: Limits) -> Self {
        let state = Rc::new(RefCell::new(State::default()));

        let settings = Settings::new()
            .with_memory_settings(
                MemorySettings::new().with_max_allowed_memory_usage(limits.max_parser_memory_bytes),
            )
            // The document is decoded as UTF-8 and a `<meta charset>` partway
            // through does not change that. Honouring it would mean the byte
            // offsets in our locators referred to a re-decoded document rather
            // than the artifact's actual bytes, which would make every locator
            // unverifiable. Non-UTF-8 documents are a documented limitation.
            .with_adjust_charset_on_meta_tag(false)
            .append_element_content_handler(text!("title", {
                let state = Rc::clone(&state);
                move |chunk: &mut TextChunk<'_>| {
                    on_title_text(&state, &limits, chunk);
                    Ok(())
                }
            }))
            .append_element_content_handler(element!("meta", {
                let state = Rc::clone(&state);
                move |el: &mut Element<'_, '_>| {
                    on_meta(&state, &limits, el);
                    check_limit(&state)
                }
            }))
            .append_element_content_handler(element!("a[href]", {
                let state = Rc::clone(&state);
                move |el: &mut Element<'_, '_>| {
                    on_anchor(&state, &limits, el);
                    check_limit(&state)
                }
            }))
            // `*` rather than `body`, because a document with no explicit
            // <body> tag matches nothing at all for that selector - and saved
            // fragments routinely have no <body>. `lol_html` fires a text
            // handler once per chunk however deeply the text is nested, so
            // this does not double-count text inside nested elements.
            .append_element_content_handler(text!("*", {
                let state = Rc::clone(&state);
                move |chunk: &mut TextChunk<'_>| {
                    on_body_text(&state, &limits, chunk);
                    check_limit(&state)
                }
            }));

        Self {
            rewriter: HtmlRewriter::new(settings, DiscardOutput),
            state,
            limits,
        }
    }

    /// Feed the next chunk of the document.
    ///
    /// Chunk boundaries do not have to fall anywhere in particular; a token
    /// split across two calls is buffered by the parser, bounded by
    /// [`Limits::max_parser_memory_bytes`].
    pub fn write(&mut self, chunk: &[u8]) -> Result<()> {
        self.rewriter.write(chunk).map_err(|e| self.map_error(e))
    }

    /// Finish the document and take what was found.
    pub fn finish(self) -> Result<Extraction> {
        let Self {
            rewriter,
            state,
            limits,
        } = self;

        if let Err(e) = rewriter.end() {
            return Err(map_rewriting_error(e, &state, &limits));
        }

        let mut state = state.borrow_mut();

        // The last chunk of prose is usually shorter than the chunk size, so
        // without this the end of every document is silently dropped.
        emit_pending_text(&mut state, &limits);

        if state.limit_hit {
            return Err(ExtractError::TooManyObservations {
                limit: limits.max_observations,
            });
        }

        Ok(Extraction {
            observations: std::mem::take(&mut state.observations),
            skipped_oversize: state.skipped_oversize,
            text_truncated_bytes: state.text_truncated_bytes,
        })
    }

    fn map_error(&self, e: lol_html::errors::RewritingError) -> ExtractError {
        map_rewriting_error(e, &self.state, &self.limits)
    }
}

impl std::io::Write for HtmlExtractor {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        HtmlExtractor::write(self, buf).map_err(std::io::Error::other)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Translate a parser failure into an error code that means something.
///
/// The observation limit is reported through a handler error, so a bare
/// `ContentHandlerError` has to be checked against our own flag before being
/// called malformed input — otherwise hitting a deliberate cap would be
/// recorded as the page being broken.
fn map_rewriting_error(
    e: lol_html::errors::RewritingError,
    state: &Rc<RefCell<State>>,
    limits: &Limits,
) -> ExtractError {
    if state.borrow().limit_hit {
        return ExtractError::TooManyObservations {
            limit: limits.max_observations,
        };
    }
    match e {
        lol_html::errors::RewritingError::MemoryLimitExceeded(_) => ExtractError::ParserMemory {
            limit: limits.max_parser_memory_bytes,
        },
        other => ExtractError::Malformed {
            reason: other.to_string(),
        },
    }
}

/// Stop the parse as soon as the cap is reached.
///
/// Returning an error out of the handler is how `lol_html` is told to stop; it
/// means a 500 MB document full of links costs the time to reach the limit,
/// not the time to tokenise the whole file.
fn check_limit(state: &Rc<RefCell<State>>) -> lol_html::HandlerResult {
    if state.borrow().limit_hit {
        return Err("observation limit reached".into());
    }
    Ok(())
}

fn on_title_text(state: &Rc<RefCell<State>>, limits: &Limits, chunk: &mut TextChunk<'_>) {
    // A `<title>` holds RCDATA: entities are decoded, but markup inside it is
    // text. Anything else arriving here would mean the selector matched
    // something that is not a title element.
    if chunk.text_type() != TextType::RCData {
        return;
    }

    let mut state = state.borrow_mut();
    if state.title_emitted {
        return;
    }

    let span = chunk.source_location().bytes();
    let text = chunk.as_str();

    match state.title.as_mut() {
        Some(buffer) => {
            // Bounded by the value cap: a `<title>` holding a megabyte of text
            // must not be accumulated in full just to be rejected afterwards.
            if buffer.text.len() <= limits.max_value_bytes {
                buffer.text.push_str(text);
            }
            buffer.end = span.end;
        }
        None => {
            state.title = Some(TextSpan {
                text: text.to_string(),
                start: span.start,
                end: span.end,
            });
        }
    }

    if chunk.last_in_text_node() {
        if let Some(buffer) = state.title.take() {
            state.title_emitted = true;
            let locator = Locator {
                selector: "title",
                start: buffer.start,
                end: buffer.end,
            };
            // Decoded only once the whole text node has arrived. An entity can
            // straddle a chunk boundary ("&am" then "p;"), so decoding chunk by
            // chunk would produce a different value depending on how the blob
            // happened to be split.
            let value = decode(&buffer.text).trim().to_string();
            state.push(limits, KIND_TITLE.to_string(), value, locator);
        }
    }
}

/// Accumulate the document's visible prose and emit it in chunks.
///
/// Only [`TextType::Data`] is prose. A script body arrives as `ScriptData` and
/// a stylesheet as `RawText`; indexing either would fill a case's search index
/// with source code nobody is looking for, and would make every page match
/// terms like `function` or `color`. A `<title>` arrives as `RCData` and is
/// already recorded as `page.title`, so taking it here as well would index it
/// twice and let one page outrank another on nothing but its title.
fn on_body_text(state: &Rc<RefCell<State>>, limits: &Limits, chunk: &mut TextChunk<'_>) {
    if chunk.text_type() != TextType::Data {
        return;
    }

    let span = chunk.source_location().bytes();
    let mut state = state.borrow_mut();

    match state.text_node.as_mut() {
        Some(buffer) => {
            // Bounded for the same reason the title buffer is: one enormous
            // text node must not be accumulated in full before it is cut up.
            if buffer.text.len() <= limits.max_text_chunk_bytes {
                buffer.text.push_str(chunk.as_str());
            }
            buffer.end = span.end;
        }
        None => {
            state.text_node = Some(TextSpan {
                text: chunk.as_str().to_string(),
                start: span.start,
                end: span.end,
            });
        }
    }

    if !chunk.last_in_text_node() {
        return;
    }

    // Decoded only once the whole node has arrived, for the reason given on
    // `decode`: an entity can straddle a write() boundary.
    let Some(node) = state.text_node.take() else {
        return;
    };
    let text = collapse_whitespace(&decode(&node.text));
    if text.is_empty() {
        return;
    }

    // Past the budget the prose is counted and discarded. Flushing whatever
    // is pending first means the boundary falls on a chunk edge rather than
    // mid-sentence.
    if state.text_indexed_bytes + text.len() > limits.max_text_bytes {
        state.text_truncated_bytes += text.len();
        emit_pending_text(&mut state, limits);
        return;
    }
    state.text_indexed_bytes += text.len();

    match state.pending_text.as_mut() {
        Some(pending) => {
            // A space between text nodes, always. Without one, `<p>A</p><p>B</p>`
            // becomes "AB" and invents a word the page does not contain, which
            // is a worse error than the one this costs: `<b>Hold</b><i>ings</i>`
            // becomes "Hold ings". Separate blocks are common and mid-word
            // formatting is rare.
            pending.text.push(' ');
            pending.text.push_str(&text);
            pending.end = node.end;
        }
        None => {
            state.pending_text = Some(TextSpan {
                text,
                start: node.start,
                end: node.end,
            });
        }
    }

    let full = state
        .pending_text
        .as_ref()
        .is_some_and(|p| p.text.len() >= limits.max_text_chunk_bytes);
    if full {
        emit_pending_text(&mut state, limits);
    }
}

/// Emit whatever prose has accumulated, if any.
fn emit_pending_text(state: &mut State, limits: &Limits) {
    let Some(pending) = state.pending_text.take() else {
        return;
    };
    let locator = Locator {
        selector: "*",
        start: pending.start,
        end: pending.end,
    };
    state.push(limits, KIND_TEXT.to_string(), pending.text, locator);
}

/// Collapse runs of whitespace to single spaces and trim.
///
/// HTML prose is full of the indentation of the document that carries it.
/// Storing that verbatim would spend most of a chunk's byte budget on layout
/// and would make phrase matching depend on how the page was formatted.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Turn source text into the value it represents.
///
/// `lol_html` is a rewriter, so it hands back the document's bytes rather than
/// their meaning: `content="&#13;=1+1"` arrives as the literal seven characters
/// `&#13;=`, not as a carriage return followed by a formula. Storing that would
/// be storing syntax instead of content, and it would defeat controls that look
/// at what a value *starts with* — the CSV-formula escape at export (A-008)
/// would see `&` and pass a payload straight through.
///
/// Decoding is reading the document as specified, not sanitising it. Nothing is
/// lost: the artifact is immutable and the locator points at the original
/// bytes, so the encoded form is always recoverable.
fn decode(raw: &str) -> String {
    html_escape::decode_html_entities(raw).into_owned()
}

fn on_meta(state: &Rc<RefCell<State>>, limits: &Limits, el: &Element<'_, '_>) {
    // `property` is the Open Graph / RDFa spelling, `name` the HTML one. A tag
    // carrying both is recorded under `property`, which is the more specific
    // claim.
    let key = decode(
        &el.get_attribute("property")
            .or_else(|| el.get_attribute("name"))
            .unwrap_or_default(),
    )
    .trim()
    .to_ascii_lowercase();

    let Some(content) = el.get_attribute("content").map(|c| decode(&c)) else {
        return;
    };

    if key.is_empty() {
        return;
    }

    // A key is part of the kind, which is a queried column. An absurd key is
    // the same problem as an absurd value and is counted the same way.
    if key.len() > MAX_META_KEY_BYTES {
        state.borrow_mut().skipped_oversize += 1;
        return;
    }

    let span = el.source_location().bytes();
    state.borrow_mut().push(
        limits,
        format!("{KIND_META_PREFIX}{key}"),
        content,
        Locator {
            selector: "meta",
            start: span.start,
            end: span.end,
        },
    );
}

/// Longest `name`/`property` attribute that may become part of an observation
/// kind.
const MAX_META_KEY_BYTES: usize = 128;

fn on_anchor(state: &Rc<RefCell<State>>, limits: &Limits, el: &Element<'_, '_>) {
    let Some(href) = el.get_attribute("href").map(|h| decode(&h)) else {
        return;
    };

    let span = el.source_location().bytes();
    let locator = Locator {
        selector: "a[href]",
        start: span.start,
        end: span.end,
    };

    // The href is recorded exactly as written, before any interpretation. What
    // the page said is the evidence; what it resolves to is a conclusion.
    state
        .borrow_mut()
        .push(limits, KIND_LINK.to_string(), href.clone(), locator.clone());

    let trimmed = href.trim();
    if let Some(address) = mailto_address(trimmed) {
        state
            .borrow_mut()
            .push(limits, KIND_EMAIL.to_string(), address, locator.clone());
        return;
    }

    if let Some(host) = http_host(trimmed) {
        state
            .borrow_mut()
            .push(limits, KIND_DOMAIN.to_string(), host, locator);
    }
}

/// The address out of a `mailto:` URL.
///
/// Parsed with the `url` crate rather than by splitting on a colon, because
/// `mailto:` accepts headers (`?subject=`) and multiple comma-separated
/// recipients, and a hand-rolled split gets those wrong in ways that end up
/// stored as evidence.
fn mailto_address(href: &str) -> Option<String> {
    let parsed = url::Url::parse(href).ok()?;
    if parsed.scheme() != "mailto" {
        return None;
    }
    let path = parsed.path();
    // Only the first recipient. A multi-recipient mailto is rare enough that
    // splitting it here would be guessing at intent; see the crate limitations.
    let first = path.split(',').next()?.trim();
    if first.is_empty() || !first.contains('@') {
        return None;
    }
    Some(first.to_string())
}

/// The host of an absolute http(s) URL.
///
/// Relative links have no host and are not resolved: resolving them needs the
/// artifact's own URL, which is provenance this function does not have. A
/// guessed base would produce domain mentions for domains the page never named.
fn http_host(href: &str) -> Option<String> {
    let parsed = url::Url::parse(href).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.host_str().map(|h| h.to_ascii_lowercase())
}
