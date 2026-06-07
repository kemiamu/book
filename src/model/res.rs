use crate::impl_stored;
use crate::model::html::HtmlWriter;
use pulldown_cmark as markdown;
use std::collections::HashSet;

// meta

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// metadata for a resource
pub struct ResourceMeta {
    pub title: String,
    pub creator: String,
    pub tags: HashSet<String>,
    date: i64,
}

impl ResourceMeta {
    /// get the unix timestamp of last modification
    pub fn date(&self) -> i64 {
        self.date
    }

    /// create new resource metadata with current timestamp
    pub fn new(
        title: impl Into<String>,
        creator: impl Into<String>,
        tags: HashSet<String>,
    ) -> Self {
        Self {
            title: title.into(),
            creator: creator.into(),
            tags,
            date: time::UtcDateTime::now().unix_timestamp(),
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
impl_stored!(Markdown);
