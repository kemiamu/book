use crate::model::Mac;
use redb::TableDefinition;
use rkyv::{Archive, rancor};

pub const USERS: TableDefinition<&str, User> = TableDefinition::new("users");

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
pub struct User {
    password: Mac,
    pub parent: String,
}

impl User {
    const PASSWD_TAG: &str = "password";

    pub fn new(password: impl AsRef<[u8]>, key: impl AsRef<[u8]>, parent: String) -> Self {
        let password = Mac::new(password, key, Self::PASSWD_TAG);
        Self { password, parent }
    }

    pub fn verify(&self, password: impl AsRef<[u8]>, key: impl AsRef<[u8]>) -> bool {
        let expected = Mac::new(password, key, Self::PASSWD_TAG);
        self.password == expected
    }
}

// signature

pub struct Signed<T: Signable> {
    pub inner: T,
}

impl<T: Signable> Signed<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn parse(s: &str, key: impl AsRef<[u8]>) -> Option<Self> {
        let (hex, sig) = s.rsplit_once('.')?;
        let data = hex::decode(hex).ok()?;
        let inner = T::deserialize(&data)?;
        let expected = Mac::new(&data, key, T::tag()).to_string();
        (sig == expected && inner.is_valid()).then_some(Self { inner })
    }

    pub fn generate(&self, key: impl AsRef<[u8]>) -> String {
        let data = self.inner.serialize();
        let sig = Mac::new(&data, key, T::tag());
        format!("{}.{}", hex::encode(&data), sig)
    }
}

pub trait Signable: Sized {
    fn tag() -> &'static str;
    fn serialize(&self) -> Vec<u8>;
    fn deserialize(bytes: &[u8]) -> Option<Self>;
    fn is_valid(&self) -> bool;
}

// invite

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
pub struct Invitation {
    pub inviter: String,
    pub expires_at: i64,
}

impl Invitation {
    pub const EXPIRY_SECS: i64 = 7 * 24 * 60 * 60;

    pub fn new(inviter: String) -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            inviter,
            expires_at: now + Self::EXPIRY_SECS,
        }
    }
}

impl Signable for Invitation {
    fn tag() -> &'static str {
        "invitation"
    }
    fn is_valid(&self) -> bool {
        self.expires_at >= time::UtcDateTime::now().unix_timestamp()
    }
    fn serialize(&self) -> Vec<u8> {
        rkyv::to_bytes::<rancor::Error>(self).unwrap().into_vec()
    }
    fn deserialize(bytes: &[u8]) -> Option<Self> {
        rkyv::from_bytes::<Invitation, rancor::Error>(bytes).ok()
    }
}

// session

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
pub struct Session {
    pub user: String,
    pub expires_at: i64,
}

impl Session {
    pub const EXPIRY_SECS: i64 = 90 * 24 * 60 * 60;

    pub fn new(user: String) -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            user,
            expires_at: now + Self::EXPIRY_SECS,
        }
    }
}

impl Signable for Session {
    fn tag() -> &'static str {
        "session"
    }
    fn is_valid(&self) -> bool {
        self.expires_at >= time::UtcDateTime::now().unix_timestamp()
    }
    fn serialize(&self) -> Vec<u8> {
        rkyv::to_bytes::<rancor::Error>(self).unwrap().into_vec()
    }
    fn deserialize(bytes: &[u8]) -> Option<Self> {
        rkyv::from_bytes::<Session, rancor::Error>(bytes).ok()
    }
}

// store

impl redb::Value for User {
    type SelfType<'a>
        = User
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn type_name() -> redb::TypeName {
        redb::TypeName::new("User")
    }

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        rkyv::from_bytes::<User, rancor::Error>(data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rkyv::to_bytes::<rancor::Error>(value).unwrap().to_vec()
    }
}
