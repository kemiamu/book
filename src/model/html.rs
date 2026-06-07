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

    #[inline]
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<(), fmt::Error> {
        self.writer.write_fmt(args)
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
            Event::Start(tag) => {
                self.start_tag(tag)?;
            }
            Event::End(tag) => {
                self.end_tag(tag)?;
            }
            Event::Text(text) => {
                if !self.in_non_writing_block {
                    self.write_str(&encode_safe(&text))?;
                }
            }
            Event::Code(text) => {
                self.write_fmt(format_args!("<code>{}</code>", encode_safe(&text),))?;
            }
            Event::InlineMath(text) => {
                self.write_fmt(format_args!(
                    "<span class=\"math math-inline\">{}</span>",
                    encode_safe(&text),
                ))?;
            }
            Event::DisplayMath(text) => {
                self.write_fmt(format_args!(
                    "<span class=\"math math-display\">{}</span>",
                    encode_safe(&text),
                ))?;
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                self.write_str(&html)?;
            }
            Event::SoftBreak => {
                self.write_str(" ")?;
            }
            Event::HardBreak => {
                self.write_str("<br />")?;
            }
            Event::Rule => {
                self.write_str("<hr />")?;
            }
            Event::FootnoteReference(name) => {
                self.write_fmt(format_args!(
                    "<sup class=\"footnote-reference\"><a href=\"#{}\">",
                    encode_safe(&name)
                ))?;
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name).or_insert(len);
                self.write_fmt(format_args!("[{number}]</a></sup>"))?;
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

    fn start_tag(&mut self, tag: Tag<'a>) -> Result<(), fmt::Error> {
        match tag {
            Tag::HtmlBlock => {}
            Tag::Paragraph => {
                self.write_str("<p>")?;
            }
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => {
                self.write_fmt(format_args!("<{}>", level))?;
                if let Some(id) = id {
                    self.write_fmt(format_args!(" id=\"{}\"", encode_safe(&id),))?;
                }
                let mut classes = classes.iter();
                if let Some(class) = classes.next() {
                    self.write_str(" class=\"")?;
                    self.write_str(&encode_safe(class))?;
                    for class in classes {
                        self.write_fmt(format_args!(" {}", encode_safe(class)))?;
                    }
                    self.write_str("\"")?;
                }
                for (attr, value) in attrs {
                    self.write_str(" ")?;
                    self.write_str(&encode_safe(&attr))?;
                    match value {
                        Some(val) => self.write_fmt(format_args!("=\"{}\"", encode_safe(&val),))?,
                        None => self.write_str("=\"\"")?,
                    }
                }
                self.write_str(">")?;
            }
            Tag::Table(alignments) => {
                self.table_alignments = alignments;
                self.write_str("<table class=\"is-fullwidth\">")?;
            }
            Tag::TableHead => {
                self.table_state = TableState::Head;
                self.table_cell_index = 0;
                self.write_str("<thead><tr>")?;
            }
            Tag::TableRow => {
                self.table_cell_index = 0;
                self.write_str("<tr>")?;
            }
            Tag::TableCell => {
                let tag = match self.table_state {
                    TableState::Head => "th",
                    TableState::Body => "td",
                };
                let styles = match self.table_alignments.get(self.table_cell_index) {
                    Some(&pulldown_cmark::Alignment::Left) => "style=\"text-align: left\"",
                    Some(&pulldown_cmark::Alignment::Center) => "style=\"text-align: center\"",
                    Some(&pulldown_cmark::Alignment::Right) => "style=\"text-align: right\"",
                    _ => "",
                };
                self.write_fmt(format_args!("<{tag} {styles}>"))?;
            }
            Tag::BlockQuote(kind) => {
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
                self.write_fmt(format_args!("<div class=\"notification{}\">", suffix))?;
            }
            Tag::CodeBlock(info) => {
                let lang = match info {
                    CodeBlockKind::Fenced(info) => info.split(' ').next().unwrap().to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                match lang.is_empty() {
                    true => self.write_str("<pre><code>")?,
                    false => self.write_fmt(format_args!(
                        "<pre><code class=\"language-{}\">",
                        encode_safe(&lang),
                    ))?,
                }
            }
            Tag::List(Some(start)) => {
                self.write_fmt(format_args!("<ol start=\"{}\">", start))?;
            }
            Tag::List(None) => {
                self.write_str("<ul>")?;
            }
            Tag::Item => {
                self.write_str("<li>")?;
            }
            Tag::DefinitionList => {
                self.write_str("<dl>")?;
            }
            Tag::DefinitionListTitle => {
                self.write_str("<dt>")?;
            }
            Tag::DefinitionListDefinition => {
                self.write_str("<dd>")?;
            }
            Tag::Subscript => {
                self.write_str("<sub>")?;
            }
            Tag::Superscript => {
                self.write_str("<sup>")?;
            }
            Tag::Emphasis => {
                self.write_str("<em>")?;
            }
            Tag::Strong => {
                self.write_str("<strong>")?;
            }
            Tag::Strikethrough => {
                self.write_str("<del>")?;
            }
            Tag::Link {
                link_type: pulldown_cmark::LinkType::Email,
                dest_url,
                title,
                id: _,
            } => {
                if title.is_empty() {
                    self.write_fmt(format_args!(
                        "<a href=\"mailto:{}\">",
                        encode_safe(&dest_url),
                    ))?;
                } else {
                    self.write_fmt(format_args!(
                        "<a href=\"mailto:{}\" title=\"{}\">",
                        encode_safe(&dest_url),
                        encode_safe(&title),
                    ))?;
                }
            }
            Tag::Link {
                link_type: _,
                dest_url,
                title,
                id: _,
            } => {
                if title.is_empty() {
                    self.write_fmt(format_args!("<a href=\"{}\">", encode_safe(&dest_url),))?;
                } else {
                    self.write_fmt(format_args!(
                        "<a href=\"{}\" title=\"{}\">",
                        encode_safe(&dest_url),
                        encode_safe(&title),
                    ))?;
                }
            }
            Tag::Image {
                link_type: _,
                dest_url,
                title,
                id: _,
            } => {
                let alt_text = self.raw_text()?;
                if !title.is_empty() {
                    self.write_fmt(format_args!(
                        "<img src=\"{}\" alt=\"{}\" title=\"{}\" />",
                        encode_safe(&dest_url),
                        alt_text,
                        encode_safe(&title),
                    ))?;
                } else {
                    self.write_fmt(format_args!(
                        "<img src=\"{}\" alt=\"{}\" />",
                        encode_safe(&dest_url),
                        alt_text,
                    ))?;
                }
            }
            Tag::FootnoteDefinition(name) => {
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name.clone()).or_insert(len);
                self.write_fmt(format_args!(
                    "<div class=\"footnote-definition\" id=\"{}\">",
                    encode_safe(&name),
                ))?;
                self.write_fmt(format_args!(
                    "<strong class=\"footnote-definition-label\">{}:</strong> ",
                    number,
                ))?;
            }
            Tag::MetadataBlock(_) => {
                self.in_non_writing_block = true;
            }
        }
        Ok(())
    }

    fn end_tag(&mut self, tag: TagEnd) -> Result<(), fmt::Error> {
        match tag {
            TagEnd::HtmlBlock => {}
            TagEnd::Paragraph => {
                self.write_str("</p>")?;
            }
            TagEnd::Heading(level) => {
                self.write_fmt(format_args!("</{level}>"))?;
            }
            TagEnd::Table => {
                self.write_str("</tbody></table>")?;
            }
            TagEnd::TableHead => {
                self.write_str("</tr></thead><tbody>")?;
                self.table_state = TableState::Body;
            }
            TagEnd::TableRow => {
                self.write_str("</tr>")?;
            }
            TagEnd::TableCell => {
                let tag = match self.table_state {
                    TableState::Head => "th",
                    TableState::Body => "td",
                };
                self.write_fmt(format_args!("</{tag}>"))?;
                self.table_cell_index += 1;
            }
            TagEnd::BlockQuote(_) => {
                self.write_str("</div>")?;
            }
            TagEnd::CodeBlock => {
                self.write_str("</code></pre>")?;
            }
            TagEnd::List(true) => {
                self.write_str("</ol>")?;
            }
            TagEnd::List(false) => {
                self.write_str("</ul>")?;
            }
            TagEnd::Item => {
                self.write_str("</li>")?;
            }
            TagEnd::DefinitionList => {
                self.write_str("</dl>")?;
            }
            TagEnd::DefinitionListTitle => {
                self.write_str("</dt>")?;
            }
            TagEnd::DefinitionListDefinition => {
                self.write_str("</dd>")?;
            }
            TagEnd::Emphasis => {
                self.write_str("</em>")?;
            }
            TagEnd::Superscript => {
                self.write_str("</sup>")?;
            }
            TagEnd::Subscript => {
                self.write_str("</sub>")?;
            }
            TagEnd::Strong => {
                self.write_str("</strong>")?;
            }
            TagEnd::Strikethrough => {
                self.write_str("</del>")?;
            }
            TagEnd::Link => {
                self.write_str("</a>")?;
            }
            TagEnd::Image => {}
            TagEnd::FootnoteDefinition => {
                self.write_str("</div>")?;
            }
            TagEnd::MetadataBlock(_) => {
                self.in_non_writing_block = false;
            }
        }
        Ok(())
    }
}
