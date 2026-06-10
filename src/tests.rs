use crate::crypto::{Signable, Signed};
use std::collections::HashSet;

#[test]
fn entry_meta_basics() {
    let mut tags = HashSet::new();
    tags.insert("rust".to_string());

    let meta = crate::model::EntryMeta::new("Hello", "alice", tags);

    assert_eq!(meta.title, "Hello");
    assert_eq!(meta.editor, "alice");
    assert!(meta.tags.contains("rust"));
    assert!(meta.last_modified > 0);
}

#[test]
fn markdown_renders_html() {
    let md = crate::model::Markdown::new("# Title");
    let html = md.render();
    assert!(html.contains("<h1>"));
    assert!(html.contains("Title"));
}

#[test]
fn user_password_verify() {
    let user = crate::model::User::new("mypass", "mysecret", "admin");
    assert!(user.verify("mypass", "mysecret"));
    assert!(!user.verify("wrong", "mysecret"));
    assert!(!user.verify("mypass", "wrong"));
}

#[test]
fn session_expiry() {
    let s = crate::model::Session::new("alice");
    assert_eq!(s.user, "alice");
    assert!(s.expires_at > time::UtcDateTime::now().unix_timestamp());
    assert!(s.is_valid());
}

#[test]
fn passkey_roundtrip() {
    let pk = crate::model::Passkey::new("alice");
    let bytes = pk.serialize();
    let restored = crate::model::Passkey::deserialize(&bytes).unwrap();
    assert_eq!(restored.creator, "alice");
    assert_eq!(restored.expires_at, pk.expires_at);
}

#[test]
fn signed_generate_and_parse() {
    let pk = crate::model::Passkey::new("bob");
    let secret = "test-secret";

    let signed = Signed::new(pk);
    let token = signed.generate(secret);

    let parsed = Signed::<crate::model::Passkey>::parse(&token, secret);
    assert!(parsed.is_some());
    assert_eq!(parsed.unwrap().inner.creator, "bob");
}

#[test]
fn heading_attributes_parsed() {
    let md = crate::model::Markdown::new("# Hello { #my-id .my-class custom=val }");
    let html = md.render();
    assert!(html.contains("id=\"my-id\""));
    assert!(html.contains("class=\"my-class\""));
    assert!(html.contains("custom=\"val\""));
}

#[test]
fn signed_tampered_token_fails() {
    let pk = crate::model::Passkey::new("bob");
    let secret = "test-secret";

    let signed = Signed::new(pk);
    let token = signed.generate(secret);

    let (data_hex, sig_hex) = token.rsplit_once('.').unwrap();
    let mut sig_bytes = hex::decode(sig_hex).unwrap();
    sig_bytes[0] ^= 0x01;
    let tampered = format!("{}.{}", data_hex, hex::encode(sig_bytes));

    let parsed = Signed::<crate::model::Passkey>::parse(&tampered, secret);
    assert!(parsed.is_none());
}
