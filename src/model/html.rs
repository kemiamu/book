use html_escape::encode_safe;
use pulldown_cmark::{CodeBlockKind, Event, Tag, TagEnd};
use std::collections::HashMap;
use std::default::Default;
use std::fmt;

enum TableState {
    Head,
    Body,
}

pub(crate) struct HtmlWriter<'a, I, W> {
    // pipe
    iter: I,
    writer: W,

    // metadata
    in_non_writing_block: bool,

    // table
    table_state: TableState,
    table_alignments: Vec<pulldown_cmark::Alignment>,
    table_cell_index: usize,

    // numbers
    numbers: HashMap<pulldown_cmark::CowStr<'a>, usize>,
}

impl<'a, I, W> HtmlWriter<'a, I, W>
where
    I: Iterator<Item = pulldown_cmark::Event<'a>>,
    W: fmt::Write,
{
    pub(crate) fn new(iter: I, writer: W) -> Self {
        Self {
            iter,
            writer,
            in_non_writing_block: false,
            table_state: TableState::Head,
            table_alignments: Default::default(),
            table_cell_index: 0,
            numbers: HashMap::new(),
        }
    }

    pub(crate) fn run(mut self) -> Result<(), fmt::Error> {
        while let Some(event) = self.iter.next() {
            self.handle_event(event)?;
        }
        Ok(())
    }

    #[inline]
    fn write_str(&mut self, s: &str) -> Result<(), fmt::Error> {
        self.writer.write_str(s)
    }

    fn raw_text(&mut self) -> Result<String, fmt::Error> {
        let mut buf = String::new();
        let mut nest: usize = Default::default();
        while let Some(event) = self.iter.next() {
            match event {
                Event::Start(_) => nest += 1,
                Event::End(_) => match nest {
                    0 => break,
                    _ => nest -= 1,
                },
                Event::Html(text)
                | Event::InlineHtml(text)
                | Event::Code(text)
                | Event::Text(text)
                | Event::InlineMath(text)
                | Event::DisplayMath(text)
                | Event::FootnoteReference(text) => {
                    buf.push_str(&encode_safe(&text));
                }
                Event::SoftBreak | Event::HardBreak | Event::Rule => {
                    buf.push(' ');
                }
                Event::TaskListMarker(true) => buf.push_str("[x]"),
                Event::TaskListMarker(false) => buf.push_str("[ ]"),
            }
        }
        Ok(buf)
    }

    fn handle_event(&mut self, event: Event<'a>) -> Result<(), fmt::Error> {
        match event {
            // paragraph
            Event::Start(Tag::Paragraph) => self.write_str("<p>")?,
            Event::End(TagEnd::Paragraph) => self.write_str("</p>")?,

            // heading
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            }) => {
                write!(self.writer, "<{level}")?;
                if let Some(id) = id {
                    write!(self.writer, " id=\"{}\"", encode_safe(&id))?;
                }
                let mut class_iter = classes.iter();
                if let Some(class) = class_iter.next() {
                    write!(self.writer, " class=\"{}\"", encode_safe(class))?;
                    for class in class_iter {
                        write!(self.writer, " {}", encode_safe(class))?;
                    }
                }
                for (attr, value) in attrs {
                    write!(self.writer, " {}", encode_safe(&attr))?;
                    match value {
                        Some(val) => write!(self.writer, "=\"{}\"", encode_safe(&val))?,
                        None => write!(self.writer, "=\"\"")?,
                    }
                }
                self.write_str(">")?;
            }
            Event::End(TagEnd::Heading(level)) => {
                write!(self.writer, "</{level}>")?;
            }

            // table
            Event::Start(Tag::Table(alignments)) => {
                self.table_alignments = alignments;
                self.write_str("<table class=\"is-fullwidth\">")?;
            }
            Event::End(TagEnd::Table) => self.write_str("</tbody></table>")?,
            Event::Start(Tag::TableHead) => {
                self.table_state = TableState::Head;
                self.table_cell_index = 0;
                self.write_str("<thead><tr>")?;
            }
            Event::End(TagEnd::TableHead) => {
                self.write_str("</tr></thead><tbody>")?;
                self.table_state = TableState::Body;
            }
            Event::Start(Tag::TableRow) => {
                self.table_cell_index = 0;
                self.write_str("<tr>")?;
            }
            Event::End(TagEnd::TableRow) => self.write_str("</tr>")?,
            Event::Start(Tag::TableCell) => {
                let tag = match self.table_state {
                    TableState::Head => "th",
                    TableState::Body => "td",
                };
                let style = match self.table_alignments.get(self.table_cell_index) {
                    Some(&pulldown_cmark::Alignment::Left) => "style=\"text-align: left\"",
                    Some(&pulldown_cmark::Alignment::Center) => "style=\"text-align: center\"",
                    Some(&pulldown_cmark::Alignment::Right) => "style=\"text-align: right\"",
                    _ => "",
                };
                write!(self.writer, "<{tag} {style}>")?;
            }
            Event::End(TagEnd::TableCell) => {
                let tag = match self.table_state {
                    TableState::Head => "th",
                    TableState::Body => "td",
                };
                write!(self.writer, "</{tag}>")?;
                self.table_cell_index += 1;
            }

            // blockquote
            Event::Start(Tag::BlockQuote(kind)) => {
                let suffix = match kind {
                    None => "",
                    Some(kind) => match kind {
                        pulldown_cmark::BlockQuoteKind::Note => " is-info",
                        pulldown_cmark::BlockQuoteKind::Tip => " is-success",
                        pulldown_cmark::BlockQuoteKind::Important => " is-primary",
                        pulldown_cmark::BlockQuoteKind::Warning => " is-warning",
                        pulldown_cmark::BlockQuoteKind::Caution => " is-danger",
                    },
                };
                write!(self.writer, "<div class=\"notification{suffix}\">")?;
            }
            Event::End(TagEnd::BlockQuote(_)) => self.write_str("</div>")?,

            // codeblock
            Event::Start(Tag::CodeBlock(info)) => {
                let lang = match info {
                    CodeBlockKind::Fenced(info) => info.split(' ').next().unwrap().to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                if lang.is_empty() {
                    self.write_str("<pre><code>")?;
                } else {
                    write!(
                        self.writer,
                        "<pre><code class=\"language-{}\">",
                        encode_safe(&lang)
                    )?;
                }
            }
            Event::End(TagEnd::CodeBlock) => self.write_str("</code></pre>")?,

            // list
            Event::Start(Tag::List(Some(start))) => {
                write!(self.writer, "<ol start=\"{start}\">")?;
            }
            Event::End(TagEnd::List(true)) => self.write_str("</ol>")?,
            Event::Start(Tag::List(None)) => self.write_str("<ul>")?,
            Event::End(TagEnd::List(false)) => self.write_str("</ul>")?,

            // item
            Event::Start(Tag::Item) => self.write_str("<li>")?,
            Event::End(TagEnd::Item) => self.write_str("</li>")?,

            // definition list
            Event::Start(Tag::DefinitionList) => self.write_str("<dl>")?,
            Event::End(TagEnd::DefinitionList) => self.write_str("</dl>")?,
            Event::Start(Tag::DefinitionListTitle) => self.write_str("<dt>")?,
            Event::End(TagEnd::DefinitionListTitle) => self.write_str("</dt>")?,
            Event::Start(Tag::DefinitionListDefinition) => self.write_str("<dd>")?,
            Event::End(TagEnd::DefinitionListDefinition) => self.write_str("</dd>")?,

            // subscript / superscript
            Event::Start(Tag::Subscript) => self.write_str("<sub>")?,
            Event::End(TagEnd::Subscript) => self.write_str("</sub>")?,
            Event::Start(Tag::Superscript) => self.write_str("<sup>")?,
            Event::End(TagEnd::Superscript) => self.write_str("</sup>")?,

            // inline styles
            Event::Start(Tag::Emphasis) => self.write_str("<em>")?,
            Event::End(TagEnd::Emphasis) => self.write_str("</em>")?,
            Event::Start(Tag::Strong) => self.write_str("<strong>")?,
            Event::End(TagEnd::Strong) => self.write_str("</strong>")?,
            Event::Start(Tag::Strikethrough) => self.write_str("<del>")?,
            Event::End(TagEnd::Strikethrough) => self.write_str("</del>")?,

            // link
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id: _,
            }) => match link_type {
                pulldown_cmark::LinkType::Email => {
                    if title.is_empty() {
                        write!(
                            self.writer,
                            "<a href=\"mailto:{}\">",
                            encode_safe(&dest_url)
                        )?;
                    } else {
                        write!(
                            self.writer,
                            "<a href=\"mailto:{}\" title=\"{}\">",
                            encode_safe(&dest_url),
                            encode_safe(&title)
                        )?;
                    }
                }
                _ => {
                    if title.is_empty() {
                        write!(self.writer, "<a href=\"{}\">", encode_safe(&dest_url))?;
                    } else {
                        write!(
                            self.writer,
                            "<a href=\"{}\" title=\"{}\">",
                            encode_safe(&dest_url),
                            encode_safe(&title)
                        )?;
                    }
                }
            },
            Event::End(TagEnd::Link) => self.write_str("</a>")?,

            // image
            Event::Start(Tag::Image {
                link_type: _,
                dest_url,
                title,
                id: _,
            }) => {
                let alt_text = self.raw_text()?;
                let caption = match title.is_empty() {
                    true => alt_text.clone(),
                    false => format!("{alt_text} ({})", encode_safe(&title)),
                };
                write!(
                    self.writer,
                    "<figure><img alt=\"{}\" src=\"{}\" /><figcaption>{caption}</figcaption></figure>",
                    encode_safe(&alt_text),
                    encode_safe(&dest_url),
                )?;
            }
            Event::End(TagEnd::Image) => {}

            // footnote
            Event::Start(Tag::FootnoteDefinition(name)) => {
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name.clone()).or_insert(len);
                write!(
                    self.writer,
                    "<div class=\"footnote-definition\" id=\"{}\"><strong class=\"footnote-definition-label\">{}:</strong> ",
                    encode_safe(&name),
                    number
                )?;
            }
            Event::End(TagEnd::FootnoteDefinition) => self.write_str("</div>")?,

            // metadatablock
            Event::Start(Tag::MetadataBlock(_)) => self.in_non_writing_block = true,
            Event::End(TagEnd::MetadataBlock(_)) => self.in_non_writing_block = false,

            // htmlblock
            Event::Start(Tag::HtmlBlock) => {}
            Event::End(TagEnd::HtmlBlock) => {}

            // non-tag events
            Event::Text(text) => {
                if !self.in_non_writing_block {
                    self.write_str(&encode_safe(&text))?;
                }
            }
            Event::Code(text) => {
                write!(self.writer, "<code>{}</code>", encode_safe(&text))?;
            }
            Event::InlineMath(text) => {
                write!(
                    self.writer,
                    "<span class=\"math math-inline\">{}</span>",
                    encode_safe(&text)
                )?;
            }
            Event::DisplayMath(text) => {
                write!(
                    self.writer,
                    "<span class=\"math math-display\">{}</span>",
                    encode_safe(&text)
                )?;
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                self.write_str(&html)?;
            }
            Event::SoftBreak => self.write_str(" ")?,
            Event::HardBreak => self.write_str("<br />")?,
            Event::Rule => self.write_str("<hr />")?,
            Event::FootnoteReference(name) => {
                write!(
                    self.writer,
                    "<sup class=\"footnote-reference\"><a href=\"#{}\">",
                    encode_safe(&name)
                )?;
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name).or_insert(len);
                write!(self.writer, "[{number}]</a></sup>")?;
            }
            Event::TaskListMarker(true) => {
                self.write_str("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>")?;
            }
            Event::TaskListMarker(false) => {
                self.write_str("<input disabled=\"\" type=\"checkbox\"/>")?;
            }
        }
        Ok(())
    }
}
