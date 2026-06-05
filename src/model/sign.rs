// signature

pub struct Signed<T: Signable> {
    pub inner: T,
}

impl<T: Signable> Signed<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn parse(s: impl AsRef<str>, secret: impl AsRef<[u8]>) -> Option<Self> {
        let (hex, sig) = s.as_ref().rsplit_once('.')?;
        let data = hex::decode(hex).ok()?;
        let inner = T::deserialize(&data)?;
        let expected = crate::model::Mac::new(&data, secret, T::tag()).to_string();
        (sig == expected && inner.is_valid()).then_some(Self { inner })
    }

    pub fn generate(&self, secret: impl AsRef<[u8]>) -> String {
        let data = self.inner.serialize();
        let sig = crate::model::Mac::new(&data, secret, T::tag());
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

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
pub struct Invitation {
    pub inviter: String,
    pub expires_at: i64,
}

impl Invitation {
    pub const EXPIRY_SECS: i64 = 7 * 24 * 60 * 60;

    pub fn new(inviter: impl Into<String>) -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            inviter: inviter.into(),
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
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .unwrap()
            .into_vec()
    }
    fn deserialize(bytes: &[u8]) -> Option<Self> {
        rkyv::from_bytes::<Invitation, rkyv::rancor::Error>(bytes).ok()
    }
}

// session

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
pub struct Session {
    pub user: String,
    pub expires_at: i64,
}

impl Session {
    pub const EXPIRY_SECS: i64 = 90 * 24 * 60 * 60;

    pub fn new(user: impl Into<String>) -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            user: user.into(),
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
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .unwrap()
            .into_vec()
    }
    fn deserialize(bytes: &[u8]) -> Option<Self> {
        rkyv::from_bytes::<Session, rkyv::rancor::Error>(bytes).ok()
    }
}
