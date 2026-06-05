use rkyv::Archive;

// mac

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
/// message authentication code
pub struct Mac([u8; 32]);

impl Mac {
    /// create a mac from input with secret and tag
    pub fn new(input: impl AsRef<[u8]>, secret: impl AsRef<[u8]>, tag: impl AsRef<[u8]>) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(input);
        hasher.update(secret);
        hasher.update(tag);
        Self(hasher.finalize().into())
    }
}

impl std::fmt::Display for Mac {
    /// format as hex string
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

// signable

/// signable token trait
pub trait Signable: Sized {
    /// type tag string
    fn tag() -> &'static str;
    /// serialize to bytes
    fn serialize(&self) -> Vec<u8>;
    /// deserialize from bytes
    fn deserialize(bytes: &[u8]) -> Option<Self>;
    /// check if the value is still valid
    fn is_valid(&self) -> bool;
}

// signed

/// signed wrapper with mac
pub struct Signed<T: Signable> {
    pub inner: T,
}

impl<T: Signable> Signed<T> {
    /// wrap a value
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// parse a signed string
    pub fn parse(s: impl AsRef<str>, secret: impl AsRef<[u8]>) -> Option<Self> {
        let (hex, sig) = s.as_ref().rsplit_once('.')?;
        let data = hex::decode(hex).ok()?;
        let inner = T::deserialize(&data)?;
        let expected = Mac::new(&data, secret, T::tag()).to_string();
        (sig == expected && inner.is_valid()).then_some(Self { inner })
    }

    /// generate a signed string
    pub fn generate(&self, secret: impl AsRef<[u8]>) -> String {
        let data = self.inner.serialize();
        let sig = Mac::new(&data, secret, T::tag());
        format!("{}.{}", hex::encode(&data), sig)
    }
}
