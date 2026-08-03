use crate::crypto::{Signable, Signed};
use crate::model::{CategoryKey, EntryKey, FileKey, Slug, UserKey};

fn user(name: &str) -> Slug<UserKey> {
    Slug::new(name.to_string()).unwrap()
}

fn category(name: &str) -> Slug<CategoryKey> {
    Slug::new(name.to_string()).unwrap()
}

#[test]
fn slug_normalize() {
    let entry = |s: &str| Slug::<EntryKey>::normalize(s);
    assert_eq!(entry("My Entry").unwrap().as_ref(), "My-Entry");
    assert_eq!(entry("  spaced  ").unwrap().as_ref(), "spaced");
    assert_eq!(entry("a/b/c").unwrap().as_ref(), "a-b-c");
    assert_eq!(entry("keep--dashes").unwrap().as_ref(), "keep--dashes");
    assert_eq!(entry("tab\there").unwrap().as_ref(), "tab-here");
    assert!(entry("").is_none());
    assert!(entry("///").is_none());
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
    assert!(Slug::<CategoryKey>::new("a.b").is_err());
    assert!(Slug::<EntryKey>::new("a.b").is_err());
    assert!(Slug::<FileKey>::new("a.b").is_ok());
    assert!(Slug::<UserKey>::new("a.b").is_err());

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
    let meta = crate::model::EntryMeta::new("Hello", user("alice"));

    assert_eq!(meta.title, "Hello");
    assert_eq!(meta.editor.as_ref(), "alice");
    assert!(meta.created_at > 0);
    assert!(meta.last_modified > 0);
}

#[test]
fn entry_meta_update_keeps_created_at() {
    let meta = crate::model::EntryMeta::new("Hello", user("alice"));
    let updated = meta.update("World", user("bob"));

    assert_eq!(updated.title, "World");
    assert_eq!(updated.editor.as_ref(), "bob");
    assert_eq!(updated.created_at, meta.created_at);
    assert!(updated.last_modified >= meta.last_modified);
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
    let sig_hex = hex::encode(sig_bytes);
    let tampered = format!("{data_hex}.{sig_hex}");

    let parsed = Signed::<crate::model::Passkey>::parse(&tampered, secret);
    assert!(parsed.is_none());
}

#[test]
fn category_slug_is_valid() {
    assert!(category("notes").as_ref() == "notes");
    assert!(Slug::<CategoryKey>::new("a.b").is_err());
}

fn cfg(base_url: &str) -> crate::config::Config {
    crate::config::Config {
        server_addr: String::new(),
        site_root: String::new(),
        base_url: base_url.into(),
        site_title: String::new(),
        secret: String::new(),
    }
}

#[test]
fn config_base_path() {
    assert_eq!(cfg("https://kemya.net/book").base_path(), "/book");
    assert_eq!(cfg("https://kemya.net/book/").base_path(), "/book/");
    assert_eq!(cfg("http://localhost:3000").base_path(), "");
    assert_eq!(cfg("https://kemya.net").base_path(), "");
    assert_eq!(cfg("https://kemya.net:8080/book").base_path(), "/book");
}
