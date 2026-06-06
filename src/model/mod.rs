use redb::TableDefinition;

pub mod error;
pub mod res;
pub mod user;

/// pages table definition
pub const PAGES: TableDefinition<&str, res::ResourceMeta> = TableDefinition::new("pages");
/// page bodies table definition
pub const PAGE_BODIES: TableDefinition<&str, res::Markdown> = TableDefinition::new("page_bodies");
/// files table definition
pub const FILES: TableDefinition<&str, res::ResourceMeta> = TableDefinition::new("files");
/// file blobs table definition
pub const FILE_BLOBS: TableDefinition<&str, Vec<u8>> = TableDefinition::new("file_blobs");

/// users table definition
pub const USERS: TableDefinition<&str, user::User> = TableDefinition::new("users");

// state

/// application state
pub struct AppState {
    pub db: redb::Database,
}

// context

/// page render context
pub struct PageContext(tera::Context);

impl PageContext {
    /// create a new context
    pub fn new() -> Self {
        let mut ctx = tera::Context::new();
        ctx.insert("site_title", &crate::CONFIG.site_title);
        ctx.insert("base_url", &crate::CONFIG.base_url);
        Self(ctx)
    }

    /// insert a template variable
    pub fn insert<T: serde::Serialize + ?Sized>(mut self, key: &str, val: &T) -> Self {
        self.0.insert(key, val);
        self
    }

    /// render the template to string
    pub fn render(self, template: &str) -> Result<String, tera::Error> {
        crate::TEMPLATES.render(template, &self.0)
    }
}

// store

/// implement redb::Value via postcard for a serde type
#[macro_export]
macro_rules! impl_stored {
    ($ty:ty) => {
        impl redb::Value for $ty {
            type SelfType<'a>
                = $ty
            where
                Self: 'a;
            type AsBytes<'a>
                = Vec<u8>
            where
                Self: 'a;

            fn type_name() -> redb::TypeName {
                redb::TypeName::new(stringify!($ty))
            }

            fn fixed_width() -> Option<usize> {
                None
            }

            fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
            where
                Self: 'a,
            {
                postcard::from_bytes(data).unwrap()
            }

            fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
            where
                Self: 'b,
            {
                postcard::to_stdvec(value).unwrap()
            }
        }
    };
}
