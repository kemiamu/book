use crate::impl_stored;
use crate::model::html::HtmlWriter;
use pulldown_cmark as markdown;
use std::collections::HashSet;

// meta

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// metadata for resources
pub struct ResourceMeta {
    pub editor: String,
    pub last_modified: i64,
}

impl ResourceMeta {
    /// create new resource metadata with current timestamp
    pub fn new(editor: impl Into<String>) -> Self {
        Self {
            editor: editor.into(),
            last_modified: time::UtcDateTime::now().unix_timestamp(),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// metadata for entries
pub struct EntryMeta {
    pub title: String,
    pub tags: HashSet<String>,
}

impl EntryMeta {
    /// create new entry metadata with title and tags
    pub fn new(title: impl Into<String>, tags: HashSet<String>) -> Self {
        Self {
            title: title.into(),
            tags,
        }
    }
}

// page

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[repr(transparent)]
/// markdown content wrapper
pub struct Markdown(String);

impl Markdown {
    /// create markdown from string
    pub fn new(content: impl Into<String>) -> Self {
        Self(content.into())
    }

    /// get the raw markdown text
    pub fn into_inner(self) -> String {
        self.0
    }

    /// render markdown to html
    pub fn render(&self) -> String {
        let parser = markdown::Parser::new_ext(&self.0, markdown::Options::all());
        let mut html_output: String = Default::default();
        HtmlWriter::new(parser, &mut html_output).run().unwrap();
        html_output
    }
}

// store

impl_stored!(ResourceMeta);
impl_stored!(EntryMeta);
impl_stored!(Markdown);
