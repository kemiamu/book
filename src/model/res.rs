use pulldown_cmark as markdown;
use redb::{TableDefinition, TypeName, Value};
use rkyv::rancor;
use rkyv::{Archive, Deserialize, Serialize};
use std::collections::HashSet;

pub const PAGES: TableDefinition<&str, ResourceMeta> = TableDefinition::new("pages");
pub const PAGE_BODIES: TableDefinition<&str, Markdown> = TableDefinition::new("page_bodies");

pub const FILES: TableDefinition<&str, ResourceMeta> = TableDefinition::new("files");
pub const FILE_BLOBS: TableDefinition<&str, FileBlob> = TableDefinition::new("file_blobs");

// meta

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub struct ResourceMeta {
    pub title: String,
    pub creator: String,
    pub date: i64,
    pub tags: HashSet<String>,
}

impl ResourceMeta {
    pub fn new(
        title: impl Into<String>,
        creator: impl Into<String>,
        tags: HashSet<String>,
    ) -> Self {
        Self {
            title: title.into(),
            creator: creator.into(),
            date: time::UtcDateTime::now().unix_timestamp(),
            tags,
        }
    }
}

// page

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[repr(transparent)]
pub struct Markdown(pub String);

impl Markdown {
    pub fn render(&self) -> String {
        let parser = markdown::Parser::new_ext(&self.0, markdown::Options::all());
        let mut html_output = String::new();
        markdown::html::push_html(&mut html_output, parser);
        html_output
    }
}

// file

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[repr(transparent)]
pub struct FileBlob(pub Vec<u8>);

// store

macro_rules! impl_value {
    ($ty:ty) => {
        impl Value for $ty {
            type SelfType<'a>
                = $ty
            where
                Self: 'a;

            type AsBytes<'a>
                = Vec<u8>
            where
                Self: 'a;

            fn type_name() -> TypeName {
                TypeName::new(stringify!($ty))
            }

            fn fixed_width() -> Option<usize> {
                None
            }

            fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
            where
                Self: 'a,
            {
                rkyv::from_bytes::<$ty, rancor::Error>(data).unwrap()
            }

            fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
            where
                Self: 'b,
            {
                rkyv::to_bytes::<rancor::Error>(value).unwrap().to_vec()
            }
        }
    };
}

impl_value!(ResourceMeta);
impl_value!(Markdown);
impl_value!(FileBlob);
