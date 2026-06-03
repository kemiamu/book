use pulldown_cmark as markdown;
use redb::{TableDefinition, TypeName, Value};
use rkyv::api::high::{HighDeserializer, HighSerializer, HighValidator};
use rkyv::bytecheck::CheckBytes;
use rkyv::rancor;
use rkyv::ser::allocator::ArenaHandle;
use rkyv::util::AlignedVec;
use rkyv::{Archive, Deserialize, Serialize};
use std::collections::HashSet;

pub const PAGES: TableDefinition<&str, Resource<Page>> = TableDefinition::new("pages");
pub const FILES: TableDefinition<&str, Resource<File>> = TableDefinition::new("files");

// resource

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub struct Resource<T> {
    pub data: T,
    pub title: String,
    pub creator: String,
    pub date: i64,
    pub tags: HashSet<String>,
}

impl<T> Resource<T> {
    pub fn new(data: T, title: String, creator: String, tags: HashSet<String>) -> Self {
        Self {
            data,
            title,
            creator,
            date: time::UtcDateTime::now().unix_timestamp(),
            tags,
        }
    }
}

// page

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub struct Page {
    pub content: String,
}

impl Page {
    pub fn new(content: String) -> Self {
        Self { content }
    }

    pub fn render(&self) -> String {
        let parser = markdown::Parser::new_ext(&self.content, markdown::Options::all());
        let mut html_output = String::new();
        markdown::html::push_html(&mut html_output, parser);
        html_output
    }
}

// file

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub struct File {
    pub data: Vec<u8>,
}

impl File {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

// storage

impl<T> Value for Resource<T>
where
    T: Archive
        + std::fmt::Debug
        + for<'a> Serialize<HighSerializer<AlignedVec, ArenaHandle<'a>, rancor::Error>>,
    T::Archived: for<'a> CheckBytes<HighValidator<'a, rancor::Error>>
        + Deserialize<T, HighDeserializer<rancor::Error>>,
{
    type SelfType<'a>
        = Resource<T>
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn type_name() -> TypeName {
        TypeName::new(&format!("Resource<{}>", std::any::type_name::<T>()))
    }

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rkyv::from_bytes::<Resource<T>, rancor::Error>(data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rkyv::to_bytes::<rancor::Error>(value).unwrap().to_vec()
    }
}
