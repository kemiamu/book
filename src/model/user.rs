use crate::model::Mac;
use redb::TableDefinition;
use rkyv::{Archive, Deserialize, Serialize, rancor};

pub const USERS: TableDefinition<&str, User> = TableDefinition::new("users");

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub struct User {
    password: Mac,
    pub parent: String,
}

impl User {
    pub fn new(password: impl AsRef<[u8]>, key: impl AsRef<[u8]>, parent: String) -> Self {
        const TAG: &str = "password";
        let password = Mac::new(password, key, TAG);
        Self { password, parent }
    }
}

// invitation

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub struct Invitation {
    pub inviter: String,
    pub expires_at: i64,
}

impl Invitation {
    pub fn new(inviter: String) -> Self {
        const EXPIRY_SECS: i64 = 7 * 24 * 60 * 60;
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            inviter,
            expires_at: now + EXPIRY_SECS,
        }
    }

    fn sign(data: &[u8], key: impl AsRef<[u8]>) -> Mac {
        const TAG: &str = "invitation";
        Mac::new(data, key, TAG)
    }

    /// Parse from hex(rkyv_bytes).hex(signature)
    pub fn parse(s: &str, key: impl AsRef<[u8]>) -> Option<Self> {
        let (hex, sig) = s.rsplit_once('.')?;
        let data = hex::decode(hex).ok()?;
        let archived = rkyv::access::<ArchivedInvitation, rancor::Error>(&data).ok()?;
        match Self::sign(&data, key).to_string() == sig
            && archived.expires_at >= time::UtcDateTime::now().unix_timestamp()
        {
            true => rkyv::deserialize::<Invitation, rancor::Error>(archived).ok(),
            false => None,
        }
    }

    /// hex(rkyv_bytes).hex(signature)
    pub fn generate(&self, key: impl AsRef<[u8]>) -> String {
        let data = rkyv::to_bytes::<rancor::Error>(self).unwrap();
        let sign = Invitation::sign(&data, key);
        format!("{}.{}", hex::encode(&data), sign)
    }
}

// storage

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
