use crate::crypto::{Signable, Signed};
use std::collections::HashSet;

#[test]
fn resource_meta_basics() {
    let mut tags = HashSet::new();
    tags.insert("rust".to_string());

    let meta = crate::model::res::ResourceMeta::new("Hello", "alice", tags);

    assert_eq!(meta.title, "Hello");
    assert_eq!(meta.creator, "alice");
    assert!(meta.tags.contains("rust"));
    assert!(meta.date > 0);
}

#[test]
fn markdown_renders_html() {
    let md = crate::model::res::Markdown("# Title".to_string());
    let html = md.render();
    assert!(html.contains("<h1>"));
    assert!(html.contains("Title"));
}

#[test]
fn user_password_verify() {
    let user = crate::model::user::User::new("mypass", "mysecret", "admin");
    assert!(user.verify("mypass", "mysecret"));
    assert!(!user.verify("wrong", "mysecret"));
    assert!(!user.verify("mypass", "wrong"));
}

#[test]
fn session_expiry() {
    let s = crate::model::user::Session::new("alice");
    assert_eq!(s.user, "alice");
    assert!(s.expires_at > time::UtcDateTime::now().unix_timestamp());
    assert!(s.is_valid());
}

#[test]
fn invitation_roundtrip() {
    let inv = crate::model::user::Invitation::new("alice");
    let bytes = inv.serialize();
    let restored = crate::model::user::Invitation::deserialize(&bytes).unwrap();
    assert_eq!(restored.inviter, "alice");
    assert_eq!(restored.expires_at, inv.expires_at);
}

#[test]
fn signed_generate_and_parse() {
    let inv = crate::model::user::Invitation::new("bob");
    let secret = "test-secret";

    let signed = Signed::new(inv);
    let token = signed.generate(secret);

    let parsed = Signed::<crate::model::user::Invitation>::parse(&token, secret);
    assert!(parsed.is_some());
    assert_eq!(parsed.unwrap().inner.inviter, "bob");
}

#[test]
fn signed_tampered_token_fails() {
    let inv = crate::model::user::Invitation::new("bob");
    let secret = "test-secret";

    let signed = Signed::new(inv);
    let token = signed.generate(secret);

    let (data_hex, sig_hex) = token.rsplit_once('.').unwrap();
    let mut sig_bytes = hex::decode(sig_hex).unwrap();
    sig_bytes[0] ^= 0x01;
    let tampered = format!("{}.{}", data_hex, hex::encode(sig_bytes));

    let parsed = Signed::<crate::model::user::Invitation>::parse(&tampered, secret);
    assert!(parsed.is_none());
}
