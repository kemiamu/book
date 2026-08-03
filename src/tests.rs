use crate::crypto::{Signable, Signed};
use crate::model::{EntryKey, FileKey, Slug, TagKey, UserKey};
use std::collections::HashSet;

fn user(name: &str) -> Slug<UserKey> {
    Slug::new(name.to_string()).unwrap()
}

fn tag(name: &str) -> Slug<TagKey> {
    Slug::new(name.to_string()).unwrap()
}

#[test]
fn slug_validation() {
    assert!(Slug::<EntryKey>::new("hello-world").is_ok());
    assert!(Slug::<EntryKey>::new("Hello_World-2").is_ok());
    assert!(Slug::<EntryKey>::new("hello/world").is_err());
    assert!(Slug::<EntryKey>::new("hello world").is_err());
    assert!(Slug::<EntryKey>::new("hello.world").is_err());
    assert!(Slug::<EntryKey>::new("").is_err());
    assert_eq!(Slug::<EntryKey>::new("hello").unwrap().as_ref(), "hello");
    assert_eq!(
        format!("{}", Slug::<EntryKey>::new("rust").unwrap()),
        "rust"
    );
}

#[test]
fn slug_rules_differ() {
    // dots are only allowed for files (filenames)
    assert!(Slug::<EntryKey>::new("a.b").is_err());
    assert!(Slug::<FileKey>::new("a.b").is_ok());
    assert!(Slug::<UserKey>::new("a.b").is_err());
    assert!(Slug::<TagKey>::new("a.b").is_err());

    // length limits
    assert!(Slug::<UserKey>::new("a".repeat(32)).is_ok());
    assert!(Slug::<UserKey>::new("a".repeat(33)).is_err());
    assert!(Slug::<EntryKey>::new("a".repeat(255)).is_ok());
    assert!(Slug::<EntryKey>::new("a".repeat(256)).is_err());
}

#[test]
fn slug_split() {
    let slugs: Vec<String> = Slug::<EntryKey>::split("Hello, World! foo_bar 123")
        .map(|s| s.to_string())
        .collect();
    assert_eq!(slugs, ["Hello", "World", "foo_bar", "123"]);
    // entries split on dots, files keep them
    assert_eq!(Slug::<EntryKey>::split("a-b_c.d/e f").count(), 4);
    assert_eq!(Slug::<FileKey>::split("a-b_c.d/e f").count(), 3);
    assert_eq!(Slug::<EntryKey>::split("   ").count(), 0);
    assert_eq!(Slug::<EntryKey>::split("").count(), 0);
}

#[test]
fn entry_meta_basics() {
    let mut tags = HashSet::new();
    tags.insert(tag("rust"));

    let meta = crate::model::EntryMeta::new("Hello", user("alice"), tags);

    assert_eq!(meta.title, "Hello");
    assert_eq!(meta.editor.as_ref(), "alice");
    assert!(meta.tags.contains(&tag("rust")));
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
    let user = crate::model::User::new("mypass", "mysecret", Some(user("admin")));
    assert!(user.verify("mypass", "mysecret"));
    assert!(!user.verify("wrong", "mysecret"));
    assert!(!user.verify("mypass", "wrong"));
}

#[test]
fn session_expiry() {
    let session = crate::model::Session::new(user("alice"));
    assert_eq!(session.user.as_ref(), "alice");
    assert!(session.expires_at > time::UtcDateTime::now().unix_timestamp());
    assert!(session.is_valid());
}

#[test]
fn passkey_roundtrip() {
    let passkey = crate::model::Passkey::new(user("alice"));
    let bytes = passkey.serialize();
    let restored = crate::model::Passkey::deserialize(&bytes).unwrap();
    assert_eq!(restored.creator().map(|u| u.as_ref()), Some("alice"));
    assert_eq!(restored.expires_at(), passkey.expires_at());
}

#[test]
fn signed_generate_and_parse() {
    let passkey = crate::model::Passkey::new(user("bob"));
    let secret = "test-secret";

    let signed = Signed::new(passkey);
    let token = signed.generate(secret);

    let parsed = Signed::<crate::model::Passkey>::parse(&token, secret);
    assert!(parsed.is_some());
    assert_eq!(
        parsed.unwrap().inner.creator().map(|u| u.as_ref()),
        Some("bob")
    );
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
    let passkey = crate::model::Passkey::new(user("bob"));
    let secret = "test-secret";

    let signed = Signed::new(passkey);
    let token = signed.generate(secret);

    let (data_hex, sig_hex) = token.rsplit_once('.').unwrap();
    let mut sig_bytes = hex::decode(sig_hex).unwrap();
    sig_bytes[0] ^= 0x01;
    let tampered = format!("{}.{}", data_hex, hex::encode(sig_bytes));

    let parsed = Signed::<crate::model::Passkey>::parse(&tampered, secret);
    assert!(parsed.is_none());
}
