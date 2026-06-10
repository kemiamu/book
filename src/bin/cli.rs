use book::crypto::Signed;
use book::model::{ENTRIES, ENTRY_HTML, ENTRY_RAW, FILE_BLOB, FILES, USERS};
use book::model::{Passkey, User};
use clap::Parser;
use time::OffsetDateTime;
use time::format_description::well_known::Iso8601;

#[derive(Parser)]
#[command(name = "cli")]
enum Cli {
    /// Initialize database tables
    InitTables,
    /// Create a new user
    InitUser {
        /// Username for the new user
        username: String,
        /// Password for the new user
        password: String,
    },
    /// Generate a passkey
    GenPasskey,
}

fn main() {
    match Cli::parse() {
        Cli::InitTables => init_tables(),
        Cli::InitUser { username, password } => init_user(&username, &password),
        Cli::GenPasskey => gen_passkey(""),
    }
}

fn gen_passkey(creator: &str) {
    let passkey = Passkey::new(creator);
    let signed = Signed::new(passkey.clone());
    let code = signed.generate(&book::CONFIG.secret);

    let expires_at = OffsetDateTime::from_unix_timestamp(passkey.expires_at)
        .ok()
        .and_then(|d| d.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let url = format!("{}/auth?passkey={}", book::CONFIG.base_url, code);
    println!("Passkey for '{}':", creator);
    println!("  Code:    {}", &code[..32]);
    println!("  URL:     {url}");
    println!("  Expires: {expires_at}");
}

fn init_tables() {
    let db = redb::Database::create("data.redb").unwrap();

    let tx = db.begin_write().unwrap();
    {
        tx.open_table(ENTRIES).unwrap();
        tx.open_table(ENTRY_RAW).unwrap();
        tx.open_table(ENTRY_HTML).unwrap();
        tx.open_table(FILES).unwrap();
        tx.open_table(FILE_BLOB).unwrap();
        tx.open_table(USERS).unwrap();
    }
    tx.commit().unwrap();

    println!("tables initialized");
}

fn init_user(username: &str, password: &str) {
    let db = redb::Database::create("data.redb").unwrap();

    let tx = db.begin_write().unwrap();
    {
        let mut users = tx.open_table(USERS).unwrap();
        let user = User::new(password, &book::CONFIG.secret, username);
        users.insert(username, user).unwrap();
    }
    tx.commit().unwrap();

    println!("user created: {username}");
}
